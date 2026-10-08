//! Скины профилей (D34): ely.by — для ely-аккаунтов, официальный Mojang
//! session server — для MSA. Offline-профилю скина нет — UI рисует честный
//! дефолт. Скин используется ТОЛЬКО как картинка в лаунчере: в игру ничего
//! не добавляется (для ely скин в игре даёт их authlib-injector, не мы).
//!
//! Источники жёстко allowlist-ены: skinsystem.ely.by и textures.minecraft.net.
//! Скачанное обязано быть PNG (magic bytes) и влезать в лимит — как иконки.

use crate::auth::{Account, AccountKind};
use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use std::time::{Duration, SystemTime};

/// Модель игрока (геометрия рук): classic — широкие (Steve), slim — узкие
/// (Alex). Так её называют Mojang (`variant`) и ely.by (`model`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkinModel {
    Classic,
    Slim,
}

impl SkinModel {
    /// Значение `model`/`variant` для API ely.by («classic» | «slim»).
    pub fn variant(self) -> &'static str {
        match self {
            SkinModel::Classic => "classic",
            SkinModel::Slim => "slim",
        }
    }

    /// Значение `variant` для Mojang skins API («wide» | «slim»). Mojang
    /// называет classic «wide» — «classic» он не знает и отвергает запрос
    /// (D64: смена скина MSA всегда падала с HTTP 400).
    pub fn mojang_variant(self) -> &'static str {
        match self {
            SkinModel::Classic => "wide",
            SkinModel::Slim => "slim",
        }
    }
}

/// Скин для показа в UI (camelCase — зеркало src/api/types.ts).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinInfo {
    pub source: SkinSource,
    pub data_url: String,
    /// D59: id в библиотеке дефолтных или каталоге пользовательских скинов
    /// (None — скин аккаунта, id у него не запрашивается).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// D59: модель для превью/применения. Библиотека задаёт её точно;
    /// у скина аккаунта и пользовательского PNG она неизвестна — решает фронт.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<SkinModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SkinSource {
    Ely,
    Mojang,
    /// D54: классический Стив, извлечённый из клиент-jar стора (тот же файл,
    /// что игра использует по умолчанию). Зеркало — src/api/types.ts.
    Default,
    /// D59: пользовательский PNG из каталога skins/ в данных лаунчера.
    /// Зеркало src/api/types.ts пополняется фронтом (вне этой полосы).
    User,
}

/// Откуда искать скин для аккаунта (без сети — тестируется юнитами).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// У профиля нет источника скина (offline) — честный дефолт UI.
    None,
    Ely(String),
    Msa(String),
}

const SKINSYSTEM: &str = "https://skinsystem.ely.by";
const SESSIONSERVER: &str = "https://sessionserver.mojang.com/session/minecraft/profile";
/// Хэш-CDN Mojang — единственный допустимый host текстур MSA.
const MOJANG_TEXTURES_HOST: &str = "textures.minecraft.net";
/// Скин — картинка для UI: лимит как у иконок (300 КБ), с запасом на 128×128 HD.
const MAX_SKIN_BYTES: usize = 300 * 1024;
/// Дисковый кэш скинов живёт час: скин меняют редко, а sessionserver у Mojang
/// ограничен по частоте запросов на профиль.
const CACHE_TTL: Duration = Duration::from_secs(3600);

fn target(account: &Account) -> Target {
    match account.kind {
        AccountKind::Offline => Target::None,
        AccountKind::Ely => Target::Ely(account.name.clone()),
        AccountKind::Msa => Target::Msa(account.uuid.clone()),
        // Свой authlib-сервер (F13): скин из чужого skinsystem не берём —
        // адрес скинов не allowlist-ен. Честный дефолт UI, как у offline.
        AccountKind::Authlib => Target::None,
    }
}

/// URL скина ely.by: ник в пути, поэтому допускаем только [A-Za-z0-9_-]
/// (латиница ely-ников; прочее — InvalidInput на IPC-границе).
fn ely_skin_url(name: &str) -> Result<String> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(LauncherError::InvalidInput(format!("некорректный ник для скина: {name}")));
    }
    Ok(format!("{SKINSYSTEM}/skins/{name}.png"))
}

fn mojang_profile_url(uuid: &str) -> Result<String> {
    if uuid.len() != 32 || !uuid.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(LauncherError::InvalidInput(format!("некорректный UUID: {uuid}")));
    }
    Ok(format!("{SESSIONSERVER}/{uuid}"))
}

/// Профиль session server: интересует только textures-свойство.
#[derive(Debug, Deserialize)]
struct SessionProfile {
    #[serde(default)]
    properties: Vec<SessionProperty>,
}

#[derive(Debug, Deserialize)]
struct SessionProperty {
    name: String,
    value: String,
}

/// base64(payload), payload = {"textures":{"SKIN":{"url":"..."}}}.
#[derive(Debug, Deserialize)]
struct TexturesPayload {
    #[serde(rename = "textures")]
    fields: TexturesFields,
}

#[derive(Debug, Deserialize)]
struct TexturesFields {
    #[serde(rename = "SKIN")]
    skin: Option<SkinTexture>,
}

#[derive(Debug, Deserialize)]
struct SkinTexture {
    url: String,
}

/// Достать URL скина из профиля; host обязан быть официальным CDN Mojang.
/// ely-текстуры через профиль не запрашиваем — у нас прямой эндпоинт скинов.
fn extract_mojang_skin_url(profile: &SessionProfile) -> Result<Option<String>> {
    let Some(prop) = profile.properties.iter().find(|p| p.name == "textures") else {
        return Ok(None);
    };
    let payload = STANDARD
        .decode(&prop.value)
        .map_err(|e| LauncherError::Internal(format!("session server: base64: {e}")))?;
    let parsed: TexturesPayload = serde_json::from_slice(&payload)
        .map_err(|e| LauncherError::Internal(format!("session server: textures JSON: {e}")))?;
    let Some(skin) = parsed.fields.skin else {
        return Ok(None);
    };
    // Официальный CDN отдаёт http://-ссылки — поднимаем до https, host после
    // этого обязан совпасть с allowlist (никаких редиректов на чужие host'ы).
    let url = skin.url.replacen("http://", "https://", 1);
    let host = reqwest::Url::parse(&url)
        .map_err(|e| LauncherError::Internal(format!("skin url: {e}")))?
        .host_str()
        .unwrap_or_default()
        .to_owned();
    if host != MOJANG_TEXTURES_HOST {
        return Err(LauncherError::InvalidInput(format!(
            "skin url host не Mojang CDN: {host}"
        )));
    }
    Ok(Some(url))
}

/// PNG-проверка: magic bytes + лимит размера. Фейковый «скин» не показываем.
fn validate_png(bytes: &[u8]) -> Result<()> {
    const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 8 || &bytes[..8] != PNG_MAGIC {
        return Err(LauncherError::InvalidInput("скин не является PNG".into()));
    }
    if bytes.len() > MAX_SKIN_BYTES {
        return Err(LauncherError::InvalidInput(format!(
            "скин больше лимита {} байт",
            bytes.len()
        )));
    }
    Ok(())
}

fn cache_path(paths: &Paths, account: &Account) -> std::path::PathBuf {
    paths.root().join("cache").join("skins").join(format!("{}.png", account.id))
}

fn cache_fresh(path: &std::path::Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age > CACHE_TTL {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    validate_png(&bytes).ok()?;
    Some(bytes)
}

/// Скин аккаунта: None — скина нет (offline, 404 у ely, пустой профиль Mojang);
/// Err — сетевая/протокольная проблема. Сначала дисковый кэш (TTL 1 час).
pub async fn fetch_skin(
    http: &HttpClient,
    paths: &Paths,
    account: &Account,
) -> Result<Option<SkinInfo>> {
    let (source, url) = match target(account) {
        // Офлайн/свой-без-профиля: настоящий Стив из клиент-jar (D54); нет
        // jar в сторе — None, UI рисует процедурный запасной вариант.
        Target::None => return Ok(default_skin(paths)),
        Target::Ely(name) => (SkinSource::Ely, ely_skin_url(&name)?),
        Target::Msa(uuid) => {
            let profile: SessionProfile = http.get_json_retry(&mojang_profile_url(&uuid)?).await?;
            match extract_mojang_skin_url(&profile)? {
                Some(url) => (SkinSource::Mojang, url),
                None => return Ok(None),
            }
        }
    };

    let cache = cache_path(paths, account);
    let bytes = match cache_fresh(&cache) {
        Some(bytes) => bytes,
        None => {
            // D62: «сырой» GET под предохранителями — send 30 с (TTFB), тело
            // 60 с (скины — сотни КБ, запас есть): молчащий CDN больше не
            // держит загрузку профиля навечно.
            let resp = http.send_timed(http.raw().get(&url)).await?;
            let status = resp.status();
            if status.as_u16() == 404 || status.as_u16() == 204 {
                // Скин сняли — кэш чистим, UI покажет дефолт.
                let _ = std::fs::remove_file(&cache);
                return Ok(None);
            }
            if !status.is_success() {
                return Err(LauncherError::network(format!("skin: HTTP {status}")));
            }
            let bytes = http.body_timed(resp).await?;
            validate_png(&bytes)?;
            if let Some(dir) = cache.parent() {
                std::fs::create_dir_all(dir)?;
            }
            crate::util::fs::atomic_write(&cache, &bytes)?;
            bytes
        }
    };

    let data_url = format!("data:image/png;base64,{}", STANDARD.encode(&bytes));
    Ok(Some(SkinInfo {
        source,
        data_url,
        id: None,
        model: None,
    }))
}

/// Настоящий Стив (D54, просьба владельца: «нельзя просто PNG лица и полного
/// стива?»). Классическая текстура берётся из клиент-jar, уже лежащего в
/// сторе: файл официальный, скачан по хэшу с сервера Mojang (спека §3),
/// лаунчер только читает его локально — ничего не докачивает и не
/// распространяет. Извлечённое кэшируется в cache/steve.png навсегда
/// (текстура в игре не меняется). Нет ни кэша, ни jar — None.
pub fn default_skin(paths: &Paths) -> Option<SkinInfo> {
    let cache = paths.cache_dir().join("steve.png");
    let bytes = match std::fs::read(crate::util::fs::long_path(&cache)) {
        Ok(bytes) => bytes,
        Err(_) => extract_steve_from_store(paths)?,
    };
    validate_png(&bytes).ok()?;
    Some(SkinInfo {
        source: SkinSource::Default,
        data_url: format!("data:image/png;base64,{}", STANDARD.encode(&bytes)),
        id: None,
        model: None,
    })
}

/// Ищем любой клиент-jar в сторе и вытаскиваем из него
/// assets/minecraft/textures/entity/steve.png (extract_zip_filtered читает
/// только нужную запись). Все ванильные jar содержат одну и ту же текстуру.
fn extract_steve_from_store(paths: &Paths) -> Option<Vec<u8>> {
    const STEVE_ENTRY: &str = "assets/minecraft/textures/entity/steve.png";
    let store = crate::util::fs::long_path(&paths.clients_store());
    let mut jars: Vec<std::path::PathBuf> = std::fs::read_dir(&store)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jar"))
        .collect();
    jars.sort();
    let staging = crate::util::fs::long_path(&paths.cache_dir().join(".steve-staging"));
    for jar in jars {
        let file = match std::fs::File::open(&jar) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let _ = std::fs::remove_dir_all(&staging);
        let extracted = crate::util::zip::extract_zip_filtered(
            file,
            &staging,
            |name| name == STEVE_ENTRY,
            |_| {},
        );
        let png = staging.join(STEVE_ENTRY);
        let bytes = extracted.ok().and_then(|_| std::fs::read(&png).ok());
        let _ = std::fs::remove_dir_all(&staging);
        if let Some(bytes) = bytes {
            if validate_png(&bytes).is_ok() {
                let _ = std::fs::create_dir_all(paths.cache_dir());
                let _ = std::fs::write(
                    crate::util::fs::long_path(&paths.cache_dir().join("steve.png")),
                    &bytes,
                );
                return Some(bytes);
            }
        }
    }
    None
}

// ---------- библиотека дефолтных скинов (D59) ----------

/// Описание скина библиотеки: id для UI, запись в клиент-jar (та же схема
/// источников, что D54: файл официальный, скачан по хэшу, лаунчер только
/// читает его локально — спеке §3 не противоречит) и модель игрока.
struct LibSkinDef {
    id: &'static str,
    entry: &'static str,
    model: SkinModel,
}

/// Библиотека (D59): Steve/Alex × classic/slim. Пути — layout 1.19.3+
/// (`textures/entity/player/{wide,slim}`); в более старых jar таких записей
/// нет, и списке этих скинов честно нет (докачивать их запрещено — спека §3).
const LIBRARY_SKINS: &[LibSkinDef] = &[
    LibSkinDef {
        id: "steve-classic",
        entry: "assets/minecraft/textures/entity/player/wide/steve.png",
        model: SkinModel::Classic,
    },
    LibSkinDef {
        id: "alex-classic",
        entry: "assets/minecraft/textures/entity/player/wide/alex.png",
        model: SkinModel::Classic,
    },
    LibSkinDef {
        id: "steve-slim",
        entry: "assets/minecraft/textures/entity/player/slim/steve.png",
        model: SkinModel::Slim,
    },
    LibSkinDef {
        id: "alex-slim",
        entry: "assets/minecraft/textures/entity/player/slim/alex.png",
        model: SkinModel::Slim,
    },
];

/// Постоянный кэш извлечённых библиотечных скинов (текстура в jar не меняется).
fn library_cache_dir(paths: &Paths) -> std::path::PathBuf {
    paths.cache_dir().join("skins").join("library")
}

/// Библиотечные скины в порядке LIBRARY_SKINS: сперва кэш, недостающее —
/// одним проходом по jar стора. Нет ни кэша, ни jar с нужными записями —
/// список честно пуст (D54: ничего не докачиваем).
pub fn list_library_skins(paths: &Paths) -> Vec<SkinInfo> {
    let cache = crate::util::fs::long_path(&library_cache_dir(paths));
    let mut found: Vec<(&'static str, SkinModel, Vec<u8>)> = Vec::new();
    let mut missing: Vec<&LibSkinDef> = Vec::new();
    for def in LIBRARY_SKINS {
        match std::fs::read(cache.join(format!("{}.png", def.id)))
            .ok()
            .filter(|bytes| validate_png(bytes).is_ok())
        {
            Some(bytes) => found.push((def.id, def.model, bytes)),
            None => missing.push(def),
        }
    }
    if !missing.is_empty() {
        if let Some(extracted) = extract_library_from_store(paths, &missing) {
            for (def, bytes) in extracted {
                let _ = std::fs::create_dir_all(&cache);
                let _ = std::fs::write(cache.join(format!("{}.png", def.id)), &bytes);
                found.push((def.id, def.model, bytes));
            }
        }
    }
    found.sort_by_key(|(id, _, _)| {
        LIBRARY_SKINS
            .iter()
            .position(|def| def.id == *id)
            .unwrap_or(usize::MAX)
    });
    found
        .into_iter()
        .map(|(id, model, bytes)| SkinInfo {
            source: SkinSource::Default,
            id: Some(id.into()),
            model: Some(model),
            data_url: format!("data:image/png;base64,{}", STANDARD.encode(&bytes)),
        })
        .collect()
}

/// Ищем любой клиент-jar в сторе и вытаскиваем из него нужные записи
/// (extract_zip_filtered читает только их). Первый jar, где нашлась хоть одна
/// текстура, — источник для всех: текстуры из разных jar не смешиваем.
fn extract_library_from_store<'a>(
    paths: &Paths,
    wanted: &[&'a LibSkinDef],
) -> Option<Vec<(&'a LibSkinDef, Vec<u8>)>> {
    let store = crate::util::fs::long_path(&paths.clients_store());
    let mut jars: Vec<std::path::PathBuf> = std::fs::read_dir(&store)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jar"))
        .collect();
    jars.sort();
    let staging = crate::util::fs::long_path(&paths.cache_dir().join(".skins-library-staging"));
    for jar in jars {
        let file = match std::fs::File::open(&jar) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let _ = std::fs::remove_dir_all(&staging);
        let extracted = crate::util::zip::extract_zip_filtered(
            file,
            &staging,
            |name| wanted.iter().any(|def| def.entry == name),
            |_| {},
        );
        let found: Vec<(&LibSkinDef, Vec<u8>)> = match extracted {
            Ok(_) => wanted
                .iter()
                .filter_map(|def| match std::fs::read(staging.join(def.entry)) {
                    Ok(bytes) if validate_png(&bytes).is_ok() => Some((*def, bytes)),
                    _ => None,
                })
                .collect(),
            // Битый jar — пробуем следующий (как в D54).
            Err(_) => Vec::new(),
        };
        let _ = std::fs::remove_dir_all(&staging);
        if !found.is_empty() {
            return Some(found);
        }
    }
    let _ = std::fs::remove_dir_all(&staging);
    None
}

// ---------- пользовательские скины (D59) ----------

/// Пользовательский PNG — картинка 64×64/64×32, в instance.json не живёт:
/// лимит скромнее, чем у иконок/аккаунтных скинов.
const USER_SKIN_MAX_BYTES: usize = 50 * 1024;

/// id скина = имя файла без расширения. Только [A-Za-z0-9_-]: отсекает
/// разделители путей и `..` (сами точки запрещены), т.е. любой traversal.
fn valid_skin_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Имя скина от пользователя → безопасный id (имя файла): посторонние
/// символы схлопываются в `-`, повторы и краевые `-` срезаются.
fn sanitize_skin_name(name: &str) -> Result<String> {
    let mut out = String::new();
    let mut prev_dash = true; // ведущие '-' не нужны
    for c in name.chars() {
        let c = if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            c
        } else {
            '-'
        };
        if c == '-' && prev_dash {
            continue;
        }
        prev_dash = c == '-';
        out.push(c);
    }
    let mut out = out.trim_matches('-').to_string();
    if out.is_empty() {
        return Err(LauncherError::InvalidInput(
            "некорректное имя скина (нужны латиница, цифры, «-» или «_»)".into(),
        ));
    }
    // Выход ASCII, значит truncate не режет посреди символа.
    const MAX_NAME: usize = 64;
    if out.len() > MAX_NAME {
        out.truncate(MAX_NAME);
    }
    Ok(out)
}

/// PNG-скин для загрузки на сервер/показа: сигнатура + IHDR с размером
/// 64×64 (современный) или 64×32 (legacy) + лимит. IHDR разбирается вручную,
/// крейт изображений не нужен: 8 (сигнатура) + 8 (len+«IHDR») + 4×2 (стороны).
fn validate_skin_png(bytes: &[u8]) -> Result<()> {
    validate_png(bytes)?;
    if bytes.len() > USER_SKIN_MAX_BYTES {
        return Err(LauncherError::InvalidInput(format!(
            "скин больше лимита {} байт",
            USER_SKIN_MAX_BYTES
        )));
    }
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return Err(LauncherError::InvalidInput("PNG без IHDR".into()));
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if width != 64 || (height != 64 && height != 32) {
        return Err(LauncherError::InvalidInput(format!(
            "скин должен быть 64×64 или 64×32 (сейчас {width}×{height})"
        )));
    }
    Ok(())
}

fn user_skin_path(paths: &Paths, id: &str) -> std::path::PathBuf {
    paths.skins_dir().join(format!("{id}.png"))
}

/// Сохранить пользовательский скин: валидация → skins/<id>.png (атомарно,
/// перезапись того же id — обновление скина).
pub fn save_user_skin(paths: &Paths, name: &str, data: &[u8]) -> Result<SkinInfo> {
    let id = sanitize_skin_name(name)?;
    validate_skin_png(data)?;
    std::fs::create_dir_all(crate::util::fs::long_path(&paths.skins_dir()))?;
    crate::util::fs::atomic_write(&user_skin_path(paths, &id), data)?;
    Ok(SkinInfo {
        source: SkinSource::User,
        id: Some(id),
        model: None,
        data_url: format!("data:image/png;base64,{}", STANDARD.encode(data)),
    })
}

/// Удалить пользовательский скин по id. id обязан быть безопасным именем
/// файла из каталога skins/ — «../x» и прочий traversal отсечён до io.
pub fn delete_user_skin(paths: &Paths, skin_id: &str) -> Result<()> {
    if !valid_skin_id(skin_id) {
        return Err(LauncherError::InvalidInput(format!(
            "некорректный id скина: {skin_id}"
        )));
    }
    let long = crate::util::fs::long_path(&user_skin_path(paths, skin_id));
    if !long.exists() {
        return Err(LauncherError::NotFound(format!("скин {skin_id}")));
    }
    std::fs::remove_file(&long)?;
    Ok(())
}

/// Каталог пользовательских скинов: *.png из skins/, битые пропускаются
/// с предупреждением (как битые instance.json в instance_list).
pub fn list_user_skins(paths: &Paths) -> Vec<SkinInfo> {
    let dir = crate::util::fs::long_path(&paths.skins_dir());
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("png"))
        {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !valid_skin_id(id) {
            tracing::warn!("skins: файл с небезопасным именем пропущен: {id}");
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        match validate_skin_png(&bytes) {
            Ok(()) => out.push(SkinInfo {
                source: SkinSource::User,
                id: Some(id.to_string()),
                model: None,
                data_url: format!("data:image/png;base64,{}", STANDARD.encode(&bytes)),
            }),
            Err(e) => tracing::warn!("skins: битый скин {id} пропущен: {e}"),
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Скин по id для skin_apply: сперва пользовательский каталог, потом
/// библиотека дефолтов. Не найден ни там, ни там — None (NotFound на границе).
pub fn find_skin_by_id(paths: &Paths, skin_id: &str) -> Option<SkinInfo> {
    list_user_skins(paths)
        .into_iter()
        .chain(list_library_skins(paths))
        .find(|skin| skin.id.as_deref() == Some(skin_id))
}

// ---------- применение скина аккаунту (D59) ----------

/// Официальный эндпоинт смены скина Mojang (вики «Microsoft authentication»).
const MSA_SKINS_URL: &str = "https://api.minecraftservices.com/minecraft/profile/skins";

/// Ротированный при refresh refresh-токен MSA — обратно в keyring под тем же
/// ref (аудит 2026-10-06, паттерн launch_identity в auth/mod.rs): MSA может
/// ротировать токен на каждом refresh, и без сохранения новый следующий
/// refresh пошёл бы по устаревшему. Нет ротации — но-оп, keyring не трогаем.
fn persist_rotated_msa_refresh(
    rr: &str,
    old_refresh: &str,
    session: &crate::auth::msa::Session,
) -> Result<()> {
    if session.refresh_token != old_refresh {
        crate::auth::keyring_set(rr, &session.refresh_token)?;
    }
    Ok(())
}

/// Скин применён — дисковый кэш текстуры аккаунта протух (TTL 1 час):
/// сносим, иначе после смены скина час показывается старый (аудит
/// 2026-10-06). Best-effort: неудача удаления — только warn (отсутствие
/// файла — норма), применение скина ошибкой не считаем.
fn invalidate_account_skin_cache(paths: &Paths, account: &Account) {
    let cache = crate::util::fs::long_path(&cache_path(paths, account));
    if let Err(e) = std::fs::remove_file(&cache) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "skins: не удалось сбросить кэш скина аккаунта {}: {e}",
                account.id
            );
        }
    }
}


/// SkinInfo несёт PNG как data-URL (тот же, что уходит в UI) — для применения
/// байты достаются обратно и проходят ту же валидацию, что при загрузке.
fn decode_skin_data_url(skin: &SkinInfo) -> Result<Vec<u8>> {
    const PREFIX: &str = "data:image/png;base64,";
    let b64 = skin
        .data_url
        .strip_prefix(PREFIX)
        .ok_or_else(|| LauncherError::InvalidInput("скин без data-URL PNG".into()))?;
    let png = STANDARD
        .decode(b64)
        .map_err(|e| LauncherError::InvalidInput(format!("скин: base64: {e}")))?;
    validate_skin_png(&png)?;
    Ok(png)
}

/// Эндпоинт смены скина ely.by по контракту их authserver API: multipart
/// PUT c Bearer-токеном сессии и моделью в query (?model=classic|slim — те
/// же значения, что ely.by кладёт в metadata текстур). ВНИМАНИЕ (проверка
/// 2026-10-04): маршрут пока отвечает 404 и в docs.ely.by загрузка не
/// задокументирована — такой запрос честно вернёт ошибку с подсказкой про
/// сайт, а при появлении эндпоинта код заработает без правок.
fn ely_upload_url(model: SkinModel) -> String {
    format!(
        "{}/api/skins?model={}",
        crate::auth::ely::AUTHSERVER,
        model.variant()
    )
}

/// multipart-тело запроса: PNG как файловая часть «file» (skin.png,
/// image/png). Чистая функция — тестируется без сети.
fn ely_skin_form(png: &[u8]) -> reqwest::multipart::Form {
    // «image/png» корректен по построению, но mime_str возвращает Result —
    // без unwrap() откатываемся к части без Content-Type.
    let part = reqwest::multipart::Part::bytes(png.to_vec())
        .file_name("skin.png")
        .mime_str("image/png")
        .unwrap_or_else(|_| {
            reqwest::multipart::Part::bytes(png.to_vec()).file_name("skin.png")
        });
    reqwest::multipart::Form::new().part("file", part)
}

/// multipart-тело смены скина Mojang: `variant` — текстовое поле (значения
/// Mojang: «wide»|«slim», см. SkinModel::mojang_variant), PNG — файловая
/// часть «file» (skin.png, image/png). JSON Mojang не принимает (HTTP 400).
/// Чистая функция — тестируется без сети.
fn msa_skin_form(png: &[u8], variant: &str) -> reqwest::multipart::Form {
    // «image/png» корректен по построению, но mime_str возвращает Result —
    // без unwrap() откатываемся к части без Content-Type.
    let part = reqwest::multipart::Part::bytes(png.to_vec())
        .file_name("skin.png")
        .mime_str("image/png")
        .unwrap_or_else(|_| {
            reqwest::multipart::Part::bytes(png.to_vec()).file_name("skin.png")
        });
    reqwest::multipart::Form::new()
        // text() требует 'static — превращаем &str в owned.
        .text("variant", variant.to_string())
        .part("file", part)
}

/// PUT скина на ely.by; возвращает HTTP-статус (разбор кодов — у вызова,
/// там же ретрай после refresh). Токен уходит только в заголовок.
async fn put_ely_skin(
    client: &HttpClient,
    token: &str,
    model: SkinModel,
    png: &[u8],
) -> Result<u16> {
    // Предохранитель (общий таймаут снят, аудит 2026-10-06): молчащий
    // сервер не должен держать смену скина навечно.
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        async {
            // D62: send под предохранителем TTFB; внешний таймаут 120 с
            // остаётся общим пределом попытки.
            let resp = client
                .send_timed(
                    client
                        .raw()
                        .put(ely_upload_url(model))
                        .bearer_auth(token)
                        .multipart(ely_skin_form(png)),
                )
                .await?;
            Ok::<u16, LauncherError>(resp.status().as_u16())
        },
    )
    .await
    .map_err(|_| LauncherError::network("сервер скинов не отвечает"))??;
    Ok(status)
}

/// Применить скин аккаунту по типу профиля (D59). Офлайн-гейты — ДО любых
/// сетевых вызовов и обращений к keyring (паттерн D43/A61).
pub async fn apply_skin(
    paths: &Paths,
    client: &HttpClient,
    account_id: &str,
    skin: &SkinInfo,
    model: SkinModel,
) -> Result<()> {
    let account = crate::auth::list(paths)
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| LauncherError::NotFound(format!("аккаунт {account_id}")))?;
    match account.kind {
        // D59: скин offline-профилю в игру не ставим (никаких инжекций) —
        // легальный пиратский путь уже есть: ely.by.
        AccountKind::Offline => Err(LauncherError::InvalidInput(
            "скин offline-профилю ставится только через бесплатный аккаунт Ely.by".into(),
        )),
        // Свой authlib-сервер (F13): единого API скинов у таких серверов нет.
        AccountKind::Authlib => Err(LauncherError::InvalidInput(
            "у своего authlib-сервера нет единого API скинов — загрузите скин на ely.by или на сервере вручную".into(),
        )),
        AccountKind::Msa => {
            // F16/D43: офлайн — честный отказ до keyring и сети.
            if client.offline() {
                return Err(LauncherError::OfflineMode("смена скина Mojang".into()));
            }
            let png = decode_skin_data_url(skin)?;
            let Some(rr) = &account.refresh_ref else {
                return Err(LauncherError::InvalidInput(
                    "у MSA-аккаунта нет refresh-токена".into(),
                ));
            };
            let settings = crate::settings::Settings::load(&paths.settings_file())?;
            let cid = settings.azure_client_id.ok_or_else(|| {
                LauncherError::InvalidInput(
                    "нет azureClientId в настройках — укажите его в Настройках (MSA-вход)".into(),
                )
            })?;
            // Свежий access_token (как в launch_identity): скины требуют
            // валидный токен. Single-flight на refresh_ref (аудит 2026-10-06):
            // параллельный launch не гонит второй refresh с тем же токеном.
            // D64: чтение refresh-токена — ПОД захваченным gate, иначе
            // параллельный потребитель успевает ротацию, и сюда уезжает
            // уже протухший токен. Запись ротации — вне блока, как было.
            let (session, refresh) = {
                let gate = crate::auth::refresh_gate(rr);
                let _held = gate.lock().await;
                let refresh = crate::auth::keyring_get(rr)?;
                let session = crate::auth::msa::refresh_session(client, &refresh, &cid).await?;
                (session, refresh)
            };
            // Ротация refresh-токена — новый сохраняем (иначе следующий
            // refresh пойдёт по протухшему).
            persist_rotated_msa_refresh(rr, &refresh, &session)?;
            // D64: PUT скина — multipart/form-data с полями variant («wide»|
            // «slim», не ely-«classic») и файловой частью «file»; JSON-тело
            // Mojang отвергает (HTTP 400 на каждый запрос).
            // D62: «сырой» запрос под предохранителем (30 с TTFB):
            // дальше нужен только статус, тело не читается.
            let resp = client
                .send_timed(
                    client
                        .raw()
                        .put(MSA_SKINS_URL)
                        .bearer_auth(&session.access_token)
                        .multipart(msa_skin_form(&png, model.mojang_variant())),
                )
                .await?;
            let status = resp.status();
            if status.is_success() {
                // Кэш текстуры протух — сбрасываем, UI сразу покажет новый скин.
                invalidate_account_skin_cache(paths, &account);
                return Ok(()); // 200/204 = скин применён
            }
            if status.as_u16() == 403 {
                // Аудит 2026-10-06 (appID одобрен Mojang): 403 — просто отказ
                // запроса, без исторических подсказок про заявки/одобрения.
                return Err(LauncherError::network(
                    "Mojang skins API отклонил запрос (HTTP 403). Проверь, что аккаунт владеет Java Edition, и попробуй позже",
                ));
            }
            Err(LauncherError::network(format!("Mojang skins: HTTP {status}")))
        }
        AccountKind::Ely => {
            // D59, прямая загрузка: multipart PUT на authserver ely.by с
            // Bearer-токеном их сессии (не OAuth) — контракт см. у
            // ely_upload_url. 401 → refresh (как в launch_identity) и ровно
            // один ретрай; 404 → честная подсказка про сайт (эндпоинта нет).
            if client.offline() {
                return Err(LauncherError::OfflineMode("загрузка скина ely.by".into()));
            }
            let png = decode_skin_data_url(skin)?;
            let Some(rr) = &account.refresh_ref else {
                return Err(LauncherError::InvalidInput(
                    "у ely-аккаунта нет refresh-токена — войдите заново".into(),
                ));
            };
            // Токен сессии ely живёт в keyring под refresh_ref: при логине
            // туда пишется refresh_token, у ely он равен accessToken.
            let mut token = crate::auth::keyring_get(rr)?;
            let mut status = put_ely_skin(client, &token, model, &png).await?;
            if status == 401 {
                // Single-flight на refresh_ref (аудит 2026-10-06): refresh
                // инвалидирует прежний токен — параллельный потребитель ждёт.
                let session = {
                    let gate = crate::auth::refresh_gate(rr);
                    let _held = gate.lock().await;
                    crate::auth::ely::refresh_session(client, &token).await?
                };
                if session.refresh_token != token {
                    crate::auth::keyring_set(rr, &session.refresh_token)?;
                }
                crate::util::redact::register_secret(&session.access_token);
                token = session.access_token;
                status = put_ely_skin(client, &token, model, &png).await?;
            }
            if (200..300).contains(&status) {
                // Кэш текстуры протух — сбрасываем, UI сразу покажет новый скин.
                invalidate_account_skin_cache(paths, &account);
                return Ok(()); // 200/204 = скин применён
            }
            if status == 404 {
                return Err(LauncherError::InvalidInput(
                    "API загрузки скинов ely.by недоступно (HTTP 404) — загрузите скин через сайт ely.by".into(),
                ));
            }
            Err(LauncherError::network(format!("ely.by skins: HTTP {status}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(kind: AccountKind, name: &str, uuid: &str) -> Account {
        Account {
            id: "acc1".into(),
            kind,
            name: name.into(),
            uuid: uuid.into(),
            refresh_ref: None,
            authlib_server: None,
        }
    }

    #[test]
    fn offline_has_no_skin() {
        assert_eq!(
            target(&account(AccountKind::Offline, "Tester", "409f7b8c")),
            Target::None
        );
    }

    /// Свой authlib-сервер (F13): скина из skinsystem нет — честный дефолт UI.
    #[test]
    fn authlib_has_no_skin_source() {
        let mut acc = account(AccountKind::Authlib, "Tester", "409f7b8c");
        acc.authlib_server = Some("https://auth.example.org".into());
        assert_eq!(target(&acc), Target::None);
    }

    #[test]
    fn ely_target_uses_name() {
        assert_eq!(
            target(&account(AccountKind::Ely, "erickskrauch", "d2a9f3c1")),
            Target::Ely("erickskrauch".into())
        );
    }

    #[test]
    fn ely_url_rejects_bad_nick() {
        assert!(ely_skin_url("ok_nick-1").is_ok());
        assert!(ely_skin_url("ник").is_err());
        assert!(ely_skin_url("../etc").is_err());
        assert!(ely_skin_url("").is_err());
    }

    #[test]
    fn mojang_url_requires_uuid32() {
        assert!(mojang_profile_url(&"a".repeat(32)).is_ok());
        assert!(mojang_profile_url("069a79f444e94726a5befca90e38aaf").is_err()); // 31 символ
        assert!(mojang_profile_url(&"z".repeat(32)).is_err()); // не hex
    }

    #[test]
    fn mojang_profile_parses_and_normalizes_http() {
        let payload = serde_json::json!({ "textures": { "SKIN": {
            "url": "http://textures.minecraft.net/texture/292009a4"
        }}});
        let value = STANDARD.encode(payload.to_string());
        let profile = SessionProfile {
            properties: vec![SessionProperty {
                name: "textures".into(),
                value,
            }],
        };
        assert_eq!(
            extract_mojang_skin_url(&profile).unwrap(),
            Some("https://textures.minecraft.net/texture/292009a4".into())
        );
    }

    #[test]
    fn mojang_profile_without_skin_is_none() {
        let payload = serde_json::json!({ "textures": {} });
        let value = STANDARD.encode(payload.to_string());
        let profile = SessionProfile {
            properties: vec![SessionProperty { name: "textures".into(), value }],
        };
        assert_eq!(extract_mojang_skin_url(&profile).unwrap(), None);

        let empty = SessionProfile { properties: vec![] };
        assert_eq!(extract_mojang_skin_url(&empty).unwrap(), None);
    }

    #[test]
    fn foreign_texture_host_rejected() {
        let payload = serde_json::json!({ "textures": { "SKIN": {
            "url": "https://evil.example/skin.png"
        }}});
        let value = STANDARD.encode(payload.to_string());
        let profile = SessionProfile {
            properties: vec![SessionProperty { name: "textures".into(), value }],
        };
        assert!(extract_mojang_skin_url(&profile).is_err());
    }

    #[test]
    fn png_validation() {
        assert!(validate_png(b"\x89PNG\r\n\x1a\nrest").is_ok());
        assert!(validate_png(b"\xFF\xD8\xFF\xE0jpeg").is_err());
        assert!(validate_png(b"\x89PNG").is_err()); // короче 8 байт
        let big = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
        let mut oversize = big.clone();
        oversize.resize(MAX_SKIN_BYTES + 1, 0);
        assert!(validate_png(&oversize).is_err());
    }

    /// D54: default_skin вытаскивает стива из клиент-jar стора, кэширует и
    /// отдаёт с источником Default; без клиентов в сторе — честный None.
    #[test]
    fn default_skin_extracts_steve_from_client_jar() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        assert!(default_skin(&paths).is_none(), "нет jar — нет Стива");

        let png_bytes = b"\x89PNG\r\n\x1a\nsteve-texture-bytes";
        let jar_path = crate::util::fs::long_path(&paths.clients_store().join("abc123.jar"));
        let file = std::fs::File::create(&jar_path).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        zw.start_file(
            "assets/minecraft/textures/entity/steve.png",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(&mut zw, png_bytes).unwrap();
        zw.finish().unwrap();

        let skin = default_skin(&paths).expect("стив должен найтись в jar");
        assert!(matches!(skin.source, SkinSource::Default));
        assert!(skin.data_url.starts_with("data:image/png;base64,"));
        // Кэш записан: повторный вызов работает уже без jar (сносим его).
        assert!(paths.cache_dir().join("steve.png").exists());
        std::fs::remove_file(&jar_path).unwrap();
        assert!(default_skin(&paths).is_some(), "кэш переживает снос jar");
    }

    /// Минимальный PNG с нужным IHDR: сигнатура + IHDR-чанк. CRC и IDAT не
    /// имитируем — валидация смотрит только сигнатуру и стороны из IHDR.
    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend_from_slice(&13u32.to_be_bytes());
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&width.to_be_bytes());
        v.extend_from_slice(&height.to_be_bytes());
        v.extend_from_slice(&[8, 6, 0, 0, 0]); // глубина 8, RGBA
        v.extend_from_slice(&[0; 4]); // CRC (не проверяется)
        v.extend_from_slice(b"IDAT");
        v
    }

    fn test_skin() -> SkinInfo {
        SkinInfo {
            source: SkinSource::User,
            id: Some("test".into()),
            model: None,
            data_url: format!(
                "data:image/png;base64,{}",
                STANDARD.encode(png_bytes(64, 64))
            ),
        }
    }

    #[test]
    fn skin_png_validation_rules() {
        assert!(validate_skin_png(&png_bytes(64, 64)).is_ok());
        assert!(validate_skin_png(&png_bytes(64, 32)).is_ok(), "legacy 64×32");
        assert!(validate_skin_png(&png_bytes(128, 128)).is_err());
        assert!(validate_skin_png(&png_bytes(64, 96)).is_err());
        assert!(validate_skin_png(b"\x89PNG\r\n\x1a\nno-ihdr").is_err());
    }

    #[test]
    fn sanitize_skin_name_rules() {
        assert_eq!(sanitize_skin_name("My Skin.png").unwrap(), "My-Skin-png");
        assert_eq!(sanitize_skin_name("--a__b--").unwrap(), "a__b");
        assert_eq!(sanitize_skin_name("a!!!b").unwrap(), "a-b");
        assert!(sanitize_skin_name("").is_err());
        assert!(sanitize_skin_name("///").is_err());
        assert_eq!(
            sanitize_skin_name(&"a".repeat(100)).unwrap().len(),
            64,
            "id обрезается до разумной длины"
        );
    }

    #[test]
    fn save_user_skin_happy_legacy_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();

        let saved = save_user_skin(&paths, "My Skin", &png_bytes(64, 64)).unwrap();
        assert_eq!(saved.id.as_deref(), Some("My-Skin"), "имя санитизируется в id");
        assert!(matches!(saved.source, SkinSource::User));
        assert!(saved.data_url.starts_with("data:image/png;base64,"));
        assert!(
            paths.skins_dir().join("My-Skin.png").exists(),
            "файл лежит в skins/ под безопасным именем"
        );

        // Legacy-формат 64×32 тоже валиден.
        save_user_skin(&paths, "legacy", &png_bytes(64, 32)).unwrap();
        let ids: Vec<_> = list_user_skins(&paths)
            .into_iter()
            .map(|s| s.id.unwrap())
            .collect();
        assert_eq!(ids, vec!["My-Skin".to_string(), "legacy".to_string()]);

        // Перезапись того же имени — обновление того же id, а не дубль.
        save_user_skin(&paths, "My Skin", &png_bytes(64, 32)).unwrap();
        assert_eq!(list_user_skins(&paths).len(), 2);
    }

    #[test]
    fn save_user_skin_rejects_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();

        // Не-PNG (сигнатура JPEG).
        assert!(save_user_skin(&paths, "jpeg", b"\xFF\xD8\xFF\xE0jpeg").is_err());
        // Не те стороны.
        assert!(save_user_skin(&paths, "dims", &png_bytes(128, 128)).is_err());
        // Больше лимита ~50 КБ.
        let mut oversize = png_bytes(64, 64);
        oversize.resize(USER_SKIN_MAX_BYTES + 1, 0);
        assert!(save_user_skin(&paths, "huge", &oversize).is_err());
        // Имя без допустимых символов.
        assert!(save_user_skin(&paths, "скин", &png_bytes(64, 64)).is_err());
        assert!(list_user_skins(&paths).is_empty(), "ничего не сохранено");
    }

    /// id скина — строго имя файла каталога skins/: traversal и разделители
    /// отсечены до io, файл-жертва остаётся на месте.
    #[test]
    fn delete_user_skin_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        save_user_skin(&paths, "victim", &png_bytes(64, 64)).unwrap();

        for bad in ["../victim", "..", ".", "a/b", "a\\b", ""] {
            let err = delete_user_skin(&paths, bad).unwrap_err();
            assert!(
                matches!(err, LauncherError::InvalidInput(_)),
                "id {bad:?}: {err:?}"
            );
        }
        assert!(
            paths.skins_dir().join("victim.png").exists(),
            "валидация отсекла traversal — жертва не удалена"
        );

        delete_user_skin(&paths, "victim").unwrap();
        assert!(!paths.skins_dir().join("victim.png").exists());
        assert!(matches!(
            delete_user_skin(&paths, "victim").unwrap_err(),
            LauncherError::NotFound(_)
        ));
    }

    #[test]
    fn find_skin_prefers_user_catalog_then_library() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        save_user_skin(&paths, "mine", &png_bytes(64, 64)).unwrap();
        assert_eq!(
            find_skin_by_id(&paths, "mine").unwrap().id.as_deref(),
            Some("mine"),
            "пользовательский скин находится по id"
        );
        // Библиотечный без jar в сторе отсутствует — find отдаст None.
        assert!(find_skin_by_id(&paths, "steve-classic").is_none());
    }

    /// Библиотека (D59): 4 скина из jar (layout 1.19.3+), кэш переживает снос
    /// jar; старый jar без player/{wide,slim} — честно пустой список.
    #[test]
    fn library_skins_come_from_client_jar() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        assert!(list_library_skins(&paths).is_empty(), "нет jar — список пуст");

        let jar_path = crate::util::fs::long_path(&paths.clients_store().join("def123.jar"));
        let file = std::fs::File::create(&jar_path).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        for def in LIBRARY_SKINS {
            zw.start_file(def.entry, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zw, &png_bytes(64, 64)).unwrap();
        }
        zw.finish().unwrap();

        let skins = list_library_skins(&paths);
        assert_eq!(skins.len(), LIBRARY_SKINS.len(), "все четыре из одного jar");
        for (skin, def) in skins.iter().zip(LIBRARY_SKINS) {
            assert_eq!(skin.id.as_deref(), Some(def.id));
            assert_eq!(skin.model, Some(def.model));
            assert!(matches!(skin.source, SkinSource::Default));
            assert!(skin.data_url.starts_with("data:image/png;base64,"));
        }
        // Кэш записан: список живёт и после сноса jar.
        std::fs::remove_file(&jar_path).unwrap();
        assert_eq!(list_library_skins(&paths).len(), LIBRARY_SKINS.len());

        // Legacy-layout (только entity/steve.png): записи player/{wide,slim}
        // отсутствуют — из такого jar скинов библиотеки честно нет.
        let dir2 = tempfile::tempdir().unwrap();
        let paths2 = Paths::new(dir2.path().to_path_buf());
        paths2.ensure_dirs().unwrap();
        let jar2 = crate::util::fs::long_path(&paths2.clients_store().join("legacy.jar"));
        let file2 = std::fs::File::create(&jar2).unwrap();
        let mut zw2 = zip::ZipWriter::new(file2);
        zw2.start_file(
            "assets/minecraft/textures/entity/steve.png",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(&mut zw2, &png_bytes(64, 64)).unwrap();
        zw2.finish().unwrap();
        assert!(
            list_library_skins(&paths2).is_empty(),
            "нет записей layout 1.19.3+ — пусто, ничего не докачиваем"
        );
    }

    /// D59: offline-профилю скин не ставится — InvalidInput до любых IO
    /// (клиент не в офлайн-режиме: единственная причина — тип аккаунта).
    #[tokio::test]
    async fn apply_skin_offline_account_fails_before_io() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        crate::auth::add(&paths, account(AccountKind::Offline, "Tester", "409f7b8c")).unwrap();
        let client = HttpClient::new(None).unwrap();
        let err = apply_skin(&paths, &client, "acc1", &test_skin(), SkinModel::Classic)
            .await
            .expect_err("offline-профилю скин не ставится");
        assert!(matches!(err, LauncherError::InvalidInput(_)));
        assert!(
            err.to_string().contains("Ely.by"),
            "подсказка ведёт на бесплатный ely.by: {err}"
        );
    }

    /// D43-паттерн: офлайн-гейт стоит ДО keyring (несуществующий refresh_ref
    /// дал бы NotFound) и до сети — OfflineMode доказывает порядок проверок.
    #[tokio::test]
    async fn apply_skin_msa_gate_fails_before_io() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut acc = account(AccountKind::Msa, "Steve", "409f7b8cf6e5441c");
        acc.refresh_ref = Some("apply-skin-nonexistent-keyring".into());
        crate::auth::add(&paths, acc).unwrap();
        let client = HttpClient::new(None).unwrap();
        client.set_offline(true);
        let err = apply_skin(&paths, &client, "acc1", &test_skin(), SkinModel::Slim)
            .await
            .expect_err("офлайн — отказ до IO");
        assert!(
            matches!(err, LauncherError::OfflineMode(_)),
            "получено: {err:?}"
        );
    }

    /// ely.by: гейт офлайна до IO; без keyring-токена — честный InvalidInput
    /// с подсказкой перелогиниться (сетевые варианты не тестируем — D59).
    #[tokio::test]
    async fn apply_skin_ely_gate_then_missing_token() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        crate::auth::add(&paths, account(AccountKind::Ely, "erickskrauch", "d2a9f3c1")).unwrap();
        let client = HttpClient::new(None).unwrap();

        client.set_offline(true);
        let err = apply_skin(&paths, &client, "acc1", &test_skin(), SkinModel::Classic)
            .await
            .expect_err("офлайн — отказ до IO");
        assert!(matches!(err, LauncherError::OfflineMode(_)), "{err:?}");

        client.set_offline(false);
        let err = apply_skin(&paths, &client, "acc1", &test_skin(), SkinModel::Classic)
            .await
            .expect_err("без токена сессии — честный отказ");
        assert!(matches!(err, LauncherError::InvalidInput(_)), "{err:?}");
        assert!(
            err.to_string().contains("войдите заново"),
            "подсказка перелогиниться: {err}"
        );
    }

    /// Контракт ely.by: модель уходит в query, значения — те же, что ely.by
    /// кладёт в metadata текстур (classic|slim, см. SkinModel::variant).
    #[test]
    fn ely_upload_url_encodes_model() {
        assert!(ely_upload_url(SkinModel::Slim).starts_with("https://authserver.ely.by/api/skins"));
        assert!(ely_upload_url(SkinModel::Classic).ends_with("?model=classic"));
        assert!(ely_upload_url(SkinModel::Slim).ends_with("?model=slim"));
    }

    /// multipart-форма собирается без сети: boundary сгенерирован (имя файла
    /// и image/png уходят в заголовках части — проверяет сборку в целом).
    #[test]
    fn ely_skin_form_builds_multipart() {
        let form = ely_skin_form(b"\x89PNG\r\n\x1a\nskin-bytes");
        assert!(!form.boundary().is_empty(), "boundary должен быть сгенерирован");
    }

    /// D64: Mojang ждёт «wide»/«slim», а не ely-«classic» — значения вариант()
    /// для Mojang берутся из mojang_variant().
    #[test]
    fn mojang_variant_uses_wide_slim() {
        assert_eq!(SkinModel::Classic.mojang_variant(), "wide");
        assert_eq!(SkinModel::Slim.mojang_variant(), "slim");
        // ely-контракт не изменился.
        assert_eq!(SkinModel::Classic.variant(), "classic");
        assert_eq!(SkinModel::Slim.variant(), "slim");
    }

    /// D64: multipart-форма Mojang собирается без сети: boundary сгенерирован,
    /// форма из двух полей (variant + файловая «file»).
    #[test]
    fn msa_skin_form_builds_multipart() {
        let form = msa_skin_form(b"\x89PNG\r\n\x1a\nskin-bytes", "wide");
        assert!(!form.boundary().is_empty(), "boundary должен быть сгенерирован");
    }

    /// Неизвестный аккаунт — NotFound до прочих проверок (как account_skin).
    #[tokio::test]
    async fn apply_skin_unknown_account_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let client = HttpClient::new(None).unwrap();
        let err = apply_skin(&paths, &client, "ghost", &test_skin(), SkinModel::Classic)
            .await
            .unwrap_err();
        assert!(matches!(err, LauncherError::NotFound(_)));
    }

    /// Аудит 2026-10-06: скин применён — дисковый кэш текстуры аккаунта
    /// сбрасывается (TTL 1 час, иначе час показывается старый скин);
    /// отсутствие кэша — не ошибка (best-effort).
    #[test]
    fn applied_skin_invalidates_account_cache() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let acc = account(AccountKind::Msa, "Steve", "409f7b8cf6e5441c");
        let cache = crate::util::fs::long_path(&cache_path(&paths, &acc));
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::fs::write(&cache, b"\x89PNG\r\n\x1a\nstale-skin").unwrap();

        invalidate_account_skin_cache(&paths, &acc);
        assert!(!cache.exists(), "протухший кэш удалён");

        // Повторный вызов без файла — тихо, без ошибки.
        invalidate_account_skin_cache(&paths, &acc);
    }

    /// Ротация refresh-токена MSA при apply_skin: новый сохраняется в keyring
    /// под тем же ref (ручная приёмка на реальном Credential Manager — как
    /// keyring_roundtrip в auth/mod.rs). Сеть/MSA не участвуют: проверяется
    /// только политика записи по факту refresh.
    #[tokio::test]
    #[ignore]
    async fn rotated_msa_refresh_is_persisted_to_keyring() {
        let rr = "test-apply-skin-rotation";
        let old = "M.COLD-ROTATION-CHECK-0001";
        let session = crate::auth::msa::Session {
            access_token: "unused".into(),
            refresh_token: "M.CNEW-ROTATION-CHECK-0002".into(),
            player_name: "Steve".into(),
            uuid: "409f7b8cf6e5441c".into(),
        };
        // Стартуем со старым токеном (как после первого логина).
        crate::auth::keyring_set(rr, old).unwrap();
        persist_rotated_msa_refresh(rr, old, &session).unwrap();
        assert_eq!(
            crate::auth::keyring_get(rr).unwrap(),
            session.refresh_token,
            "ротированный токен сохранён под тем же ref"
        );
        crate::auth::keyring_delete(rr).unwrap();
    }

    /// Без ротации — но-оп: запись в keyring не появляется. Герметично, пока
    /// записи нет; падение теста означает, что helper стал писать без ротации
    /// (запись утечёт в Credential Manager — починить и удалить её).
    #[test]
    fn unrotated_msa_refresh_leaves_keyring_untouched() {
        let rr = "test-apply-skin-noop";
        let token = "M.CSAME-TOKEN-NOOP-0003";
        let session = crate::auth::msa::Session {
            access_token: "unused".into(),
            refresh_token: token.into(),
            player_name: "Steve".into(),
            uuid: "409f7b8cf6e5441c".into(),
        };
        persist_rotated_msa_refresh(rr, token, &session).unwrap();
        assert!(
            crate::auth::keyring_get(rr).is_err(),
            "без ротации keyring не пишется"
        );
    }
}
