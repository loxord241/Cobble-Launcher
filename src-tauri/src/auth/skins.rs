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

/// Скин для показа в UI (camelCase — зеркало src/api/types.ts).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinInfo {
    pub source: SkinSource,
    pub data_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SkinSource {
    Ely,
    Mojang,
    /// D54: классический Стив, извлечённый из клиент-jar стора (тот же файл,
    /// что игра использует по умолчанию). Зеркало — src/api/types.ts.
    Default,
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
            let resp = http.raw().get(&url).send().await.map_err(LauncherError::from)?;
            let status = resp.status();
            if status.as_u16() == 404 || status.as_u16() == 204 {
                // Скин сняли — кэш чистим, UI покажет дефолт.
                let _ = std::fs::remove_file(&cache);
                return Ok(None);
            }
            if !status.is_success() {
                return Err(LauncherError::network(format!("skin: HTTP {status}")));
            }
            let bytes = resp.bytes().await.map_err(LauncherError::from)?;
            validate_png(&bytes)?;
            if let Some(dir) = cache.parent() {
                std::fs::create_dir_all(dir)?;
            }
            crate::util::fs::atomic_write(&cache, &bytes)?;
            bytes.to_vec()
        }
    };

    let data_url = format!("data:image/png;base64,{}", STANDARD.encode(&bytes));
    Ok(Some(SkinInfo { source, data_url }))
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
}
