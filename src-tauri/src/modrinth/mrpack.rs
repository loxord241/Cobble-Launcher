//! Модпаки .mrpack (спека §6.2, §6.6): modrinth.index.json + overrides/,
//! зависимости → создание инстанса нужного вида, env-фильтр client.

use crate::errors::{LauncherError, Result};
use crate::instances::{ContentEntry, ContentKind, ContentSource, Instance};
use crate::net::download::{DownloadEngine, DownloadTask};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Кап на текстовые манифесты внутри .mrpack: 16 МБ. Зеркало D23 в import
/// (там константа приватная): modrinth.index.json и missing_mods_checker.json —
/// килобайты; больше — либо битый архив, либо попытка съесть память ядра
/// (заявленный размер записи проверяем ДО чтения в память).
pub(crate) const MAX_MRPACK_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

/// Кап на число файлов в modrinth.index.json: легальные паки — сотни записей,
/// десятки тысяч — мусор или DoS (перебор таких файлов дороже честного отказа).
const MAX_INDEX_FILES: usize = 50_000;

/// D62: тишина при чтении стрима дольше двух минут = «сервер не отвечает» —
/// зеркало READ_IDLE движка загрузок (net/download.rs) и adoptium.rs. Общий
/// таймаут у HttpClient снят: без этого капа молчащий сервер (отдал заголовки
/// и замолчал в теле) подвешивал бы установку модпака/скачивание картинки навечно.
const READ_IDLE: std::time::Duration = std::time::Duration::from_secs(120);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackIndex {
    pub format_version: u32,
    #[serde(default)]
    pub game: Option<String>,
    pub version_id: String,
    pub name: String,
    #[serde(default)]
    pub files: Vec<MrpackFile>,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackFile {
    pub path: String,
    pub hashes: HashMap<String, String>,
    /// env может отсутствовать — тогда считается required (спека §6.2).
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    pub downloads: Vec<String>,
    #[serde(default)]
    pub file_size: u64,
    /// D64: projectId/versionId КОНКРЕТНОГО файла. Официальная спека их НЕ
    /// описывает (там только path/hashes/env/downloads/fileSize), но Modrinth
    /// App пишет их в files[] — а в content-manifest нужна версия МОДА, а не
    /// версия пака (index.version_id): check_updates сверяет запись с версией
    /// мода, и паковая версия делала весь свежий пак «устаревшим». Полей нет —
    /// None, версию добирает CDN-ссылка (ids_from_cdn_url).
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
}

impl MrpackIndex {
    /// Проверка структуры: внятные ошибки вместо паник (спека §6.7 — битые
    /// манифесты сообщать понятно).
    pub fn validate(&self) -> Result<()> {
        if self.format_version != 1 {
            return Err(LauncherError::InvalidInput(format!(
                "formatVersion {} не поддерживается (ждём 1)",
                self.format_version
            )));
        }
        if let Some(game) = &self.game {
            if game != "minecraft" {
                return Err(LauncherError::InvalidInput(format!(
                    "игра модпака: {game}, ждём minecraft"
                )));
            }
        }
        if self.files.len() > MAX_INDEX_FILES {
            return Err(LauncherError::InvalidInput(format!(
                "слишком много файлов в модпаке: {} (лимит {MAX_INDEX_FILES})",
                self.files.len()
            )));
        }
        Ok(())
    }

    /// Файлы для КЛИЕНТА: env.client != "unsupported" (спека §6.6).
    pub fn client_files(&self) -> impl Iterator<Item = &MrpackFile> {
        self.files.iter().filter(|f| {
            f.env
                .as_ref()
                .and_then(|e| e.get("client").map(|c| c != "unsupported"))
                .unwrap_or(true)
        })
    }
}

/// Открыть .mrpack (zip) и распарсить modrinth.index.json.
fn open_mrpack(path: &Path) -> Result<MrpackIndex> {
    let file = std::fs::File::open(crate::util::fs::long_path(path))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| LauncherError::Zip(format!("не удалось открыть .mrpack: {e}")))?;
    let index_entry = zip
        .by_name("modrinth.index.json")
        .map_err(|_| {
            LauncherError::InvalidInput(
                "в .mrpack нет modrinth.index.json — это не модпак Modrinth".into(),
            )
        })?;
    // D23-зеркало: заявленный распакованный размер проверяем ДО чтения —
    // чужой zip может объявить гигабайтный index.json и уронить ядро по OOM.
    if index_entry.size() > MAX_MRPACK_MANIFEST_BYTES {
        return Err(LauncherError::InvalidInput(format!(
            "манифест modrinth.index.json слишком большой: {} байт (лимит {} МБ)",
            index_entry.size(),
            MAX_MRPACK_MANIFEST_BYTES / (1024 * 1024)
        )));
    }
    let index: MrpackIndex = serde_json::from_reader(index_entry)?;
    index.validate()?;
    Ok(index)
}

/// Запись конфига MissingModsChecker (Better MC и похожие паки): мод, которого
/// НЕТ в files[] .mrpack — при первом запуске игры такой мод сам качает их
/// с CurseForge через Swing-окно. Поле `url` (ссылка на CurseForge) сознательно
/// НЕ разбирается: качаем только с Modrinth по хэшам (правило «хэш на каждый
/// байт»), прочие лишние поля serde игнорирует сам.
#[derive(Debug, Clone, Deserialize)]
struct MissingModsEntry {
    /// Как искать проект на Modrinth.
    #[serde(rename = "displayName", alias = "display_name")]
    display_name: String,
    /// Имя файла, которое мод ждёт в каталоге (например
    /// `balm-fabric-1.20.1-7.3.38.jar`) — по нему ищем файл версии.
    pattern: String,
    /// `mods` | `resourcepacks` | `shaderpacks`; прочее — пропускаем.
    destination: String,
    /// sha1 из конфига (бывают не у всех паков): по нему сверяем файл, уже
    /// лежащий на диске, — побитый/подменённый перекачиваем, а не пропускаем.
    #[serde(default)]
    sha1: Option<String>,
}

/// Кап на число записей missing_mods_checker: легальные конфиги — десятки;
/// больше — мусор, берём первые (спека §15: чужой конфиг не управляет объёмом
/// работы лаунчера).
const MAX_MM_ENTRIES: usize = 50;

/// Потолок длины displayName (слаги Modrinth короткие, как CATEGORY_MAX_LEN
/// в api.rs): длинная строка поиском не является — мусор конфига, skip.
const MAX_MM_DISPLAY_NAME: usize = 64;

/// displayName годен для поиска: непустой и не длиннее потолка.
fn display_name_ok(display_name: &str) -> bool {
    !display_name.is_empty() && display_name.len() <= MAX_MM_DISPLAY_NAME
}

/// Конфиг MissingModsChecker читаем прямо из zip .mrpack
/// (`overrides/config/missing_mods_checker.json`). Нет файла — это нормально
/// (большинство паков его не имеет): тихо пусто. Битый JSON — warn и пусто:
/// конфиг не стоит сорванной установки.
fn read_missing_mods_checker(path: &Path) -> Vec<MissingModsEntry> {
    const ENTRY: &str = "overrides/config/missing_mods_checker.json";
    let Ok(file) = std::fs::File::open(crate::util::fs::long_path(path)) else {
        return Vec::new();
    };
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(z) => z,
        Err(e) => {
            tracing::warn!("не удалось открыть .mrpack для missing_mods_checker: {e}");
            return Vec::new();
        }
    };
    let Ok(entry) = zip.by_name(ENTRY) else {
        return Vec::new(); // конфига нет — обычный пак
    };
    // Тот же кап, что у index.json: конфиг — килобайты, гигабайтная запись
    // из чужого архива в память не читается (D23-зеркало).
    if entry.size() > MAX_MRPACK_MANIFEST_BYTES {
        tracing::warn!(
            "конфиг {ENTRY} слишком большой ({} байт) — предустановка пропущена",
            entry.size()
        );
        return Vec::new();
    }
    let mut parsed: Vec<MissingModsEntry> = match serde_json::from_reader(entry) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("битый конфиг {ENTRY}: {e} — предустановка пропущена");
            Vec::new()
        }
    };
    if parsed.len() > MAX_MM_ENTRIES {
        tracing::warn!(
            "missing_mods_checker: {} записей — беру первые {MAX_MM_ENTRIES}",
            parsed.len()
        );
        parsed.truncate(MAX_MM_ENTRIES);
    }
    parsed
}

/// Загрузчик из dependencies .mrpack (спека §6.2): у Forge/NeoForge ключ —
/// `forge`/`neoforge` БЕЗ суффикса `-loader` (пример BMC4:
/// {"minecraft":"1.20.1","forge":"47.3.0"}), у Fabric/Quilt —
/// `fabric-loader`/`quilt-loader`. Значение — точная версия загрузчика.
/// Раньше ключи искались фильтром `ends_with("-loader")`, и forge/neoforge
/// молча пропускались — пак вставал как vanilla без загрузчика.
/// Неизвестный `*-loader` — честная ошибка ДО скачиваний (спека §15: не падать
/// молча), остальные ключи (кроме `minecraft` — версия игры) игнорируются.
pub fn resolve_loader(deps: &HashMap<String, String>) -> Result<Option<(&str, &str)>> {
    for (key, name) in [
        ("fabric-loader", "fabric"),
        ("quilt-loader", "quilt"),
        ("forge", "forge"),
        ("neoforge", "neoforge"),
    ] {
        if let Some(v) = deps.get(key) {
            return Ok(Some((name, v.as_str())));
        }
    }
    if let Some(other) = deps.keys().find(|k| k.ends_with("-loader")) {
        return Err(LauncherError::InvalidInput(format!(
            "неизвестный загрузчик модпака: {other}"
        )));
    }
    Ok(None)
}

/// Хосты, с которых .mrpack имеет право тянуть файлы (A24). Чужой архив мог
/// указать `http://127.0.0.1:8080/...` или адрес метаданных облака — лаунчер
/// сходил бы туда из процесса пользователя (SSRF). Разрешены только
/// официальные API/CDN Modrinth и GitHub-релизы, куда модпаки и кладут jar'ы.
pub const ALLOWED_MRPACK_HOSTS: &[&str] = &[
    "api.modrinth.com",
    "cdn.modrinth.com",
    "github.com",
    "objects.githubusercontent.com",
    "raw.githubusercontent.com",
];

/// Проверить URL загрузки из НЕдоверенного .mrpack: только https + allowlist.
pub fn ensure_allowed_mrpack_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url).map_err(|e| {
        LauncherError::InvalidInput(format!("в модпаке некорректный URL {url:?}: {e}"))
    })?;
    let host = parsed.host_str().unwrap_or_default();
    // Тесты гоняют загрузку на локальном axum-сервере (http, внешней сети нет) —
    // в test-сборке петельбек разрешён до проверок схемы и allowlist. В релизной
    // сборке этой ветки не существует: прод-проверка не ослаблена.
    #[cfg(test)]
    if matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1") {
        return Ok(());
    }
    if parsed.scheme() != "https" {
        return Err(LauncherError::InvalidInput(format!(
            "в модпаке разрешены только https-ссылки: {url}"
        )));
    }
    if ALLOWED_MRPACK_HOSTS
        .iter()
        .any(|h| host.eq_ignore_ascii_case(h))
    {
        return Ok(());
    }
    Err(LauncherError::InvalidInput(format!(
        "модпак ссылается на недоверенный хост {host}: разрешены {ALLOWED_MRPACK_HOSTS:?}"
    )))
}

/// Хосты, с которых разрешено тянуть картинки проекта (иконка `icon_url` и
/// галерейный арт `raw_url` — оба живут на CDN Modrinth). Ответ API даже в
/// проде — JSON с чужого сервера: URL картинки не должен уводить GET на
/// произвольный хост (та же SSRF-гигиена, что A24 у .mrpack).
const ALLOWED_CDN_HOSTS: &[&str] = &["cdn.modrinth.com"];

/// Проверить URL картинки из ответа API: только https + CDN Modrinth.
pub(crate) fn ensure_allowed_cdn_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url).map_err(|e| {
        LauncherError::InvalidInput(format!("некорректный URL картинки {url:?}: {e}"))
    })?;
    let host = parsed.host_str().unwrap_or_default();
    // Тесты гоняют картинки через локальный axum-мок (http, внешней сети нет) —
    // в test-сборке петельбек разрешён. В релизной сборке этой ветки нет:
    // прод-проверка не ослаблена (как в ensure_allowed_mrpack_url).
    #[cfg(test)]
    if matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1") {
        return Ok(());
    }
    if parsed.scheme() != "https" {
        return Err(LauncherError::InvalidInput(format!(
            "картинки разрешено тянуть только по https: {url}"
        )));
    }
    if ALLOWED_CDN_HOSTS
        .iter()
        .any(|h| host.eq_ignore_ascii_case(h))
    {
        return Ok(());
    }
    Err(LauncherError::InvalidInput(format!(
        "картинка с недоверенного хоста {host}: разрешены {ALLOWED_CDN_HOSTS:?}"
    )))
}

/// Скачать картинку (иконка/арт проекта) с жёстким капом размера.
/// Ok(None) — файл тяжелее лимита: галерейные кадры бывают на несколько МБ,
/// это не ошибка, вызывающий просто пропускает картинку (спека §15).
/// Тело читается СТРИМОМ с обрывом на лимите: `resp.bytes()` ждал бы тело
/// целиком, и сервер, совравший в Content-Length, надул бы память ядра.
async fn fetch_image_capped(
    client: &HttpClient,
    url: &str,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>> {
    ensure_allowed_cdn_url(url)?;
    // D62: send «сырого» запроса под предохранителем send_timed (30 с) —
    // сервер, принявший соединение и молчащий, не подвешивает установку.
    let resp = client
        .send_timed(client.raw().get(url))
        .await?
        .error_for_status()?;
    // Быстрый путь: Content-Length известен заранее — поток не открываем.
    if let Some(len) = resp.content_length() {
        if len > max_bytes as u64 {
            tracing::warn!("картинка {url} больше лимита ({len} > {max_bytes} байт) — пропускаю");
            return Ok(None);
        }
    }
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = std::pin::pin!(resp.bytes_stream());
    // D62: каждый шаг чтения ограничен READ_IDLE — тишина дольше двух минут =
    // «сервер не отвечает», наружу сетевая ошибка (картинка — некритичный шаг,
    // вызывающий её проглотит), а не вечное висение стрима.
    while let Some(chunk) =
        tokio::time::timeout(READ_IDLE, futures::StreamExt::next(&mut stream))
            .await
            .map_err(|_| {
                LauncherError::network(format!(
                    "сервер не отвечает более {} с: {url}",
                    READ_IDLE.as_secs()
                ))
            })?
    {
        let chunk = chunk?;
        if buf.len() + chunk.len() > max_bytes {
            tracing::warn!(
                "картинка {url} выросла за лимит ({max_bytes} байт) по ходу стрима — пропускаю"
            );
            return Ok(None);
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(Some(buf))
}

/// Базовый URL Modrinth API: в прод-сборке — константа, в тестах — локальный
/// mock-сервер (внешней сети в тестах нет). Как в updates.rs.
fn api_base() -> String {
    #[cfg(test)]
    if let Some(base) = TEST_API_BASE.get() {
        return base.clone();
    }
    crate::modrinth::api::API_BASE.to_string()
}

#[cfg(test)]
static TEST_API_BASE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Поиск модов для MissingModsChecker: query + facets project_type:mod, те же
/// параметры, что api::search, но через api_base() — иначе тестовый mock этот
/// путь не перехватывает и тесты уходят во внешнюю сеть.
async fn search_mods(
    client: &HttpClient,
    query: &str,
    limit: u32,
) -> Result<crate::modrinth::api::SearchResult> {
    // F16: офлайн — мгновенный отказ до похода в сеть.
    if client.offline() {
        return Err(LauncherError::OfflineMode("поиск Modrinth".into()));
    }
    let facets = serde_json::to_string(&[["project_type:mod"]])?;
    let url = format!("{}/search", api_base());
    // D62: send «сырого» запроса под предохранителем send_timed, JSON-тело —
    // под json_timed (60 с): заголовки могли прийти, а тело — нет.
    let resp = client
        .send_timed(client.raw().get(&url).query(&[
            ("query", query.to_string()),
            ("facets", facets),
            ("index", "relevance".to_string()),
            ("limit", limit.to_string()),
            ("offset", "0".to_string()),
        ]))
        .await?
        .error_for_status()?;
    let result: crate::modrinth::api::SearchResult = client.json_timed(resp).await?;
    Ok(result)
}

/// Версии проекта: тот же эндпоинт и `get_json_retry`, что
/// api::project_versions, но через api_base() — как у search_mods (см.
/// updates.rs: иначе тестовый mock этот путь не перехватывает).
async fn project_versions(
    client: &HttpClient,
    project_id: &str,
) -> Result<Vec<crate::modrinth::api::Version>> {
    let url = format!("{}/project/{project_id}/version", api_base());
    client.get_json_retry(&url).await
}

/// Имя файла из чужого конфига: ровно одна безопасная компонента пути
/// (санитизация недоверенных данных, как safe_relative_path у overrides) —
/// иначе чужой конфиг мог бы записать файл мимо каталога назначения.
fn is_safe_single_filename(name: &str) -> bool {
    // Длина — в байтах: потолок файловой системы (255) именно про байты UTF-8.
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    if name == "." || name == ".." {
        return false;
    }
    if name.contains(['/', '\\', ':', '\0']) {
        return false;
    }
    // Управляющие символы и невидимый юникод (zero-width, bidi-метки, изоляторы):
    // имя, которое «не такое, каким выглядит» — вектор подмены и обхода фильтров.
    if name.chars().any(|c| {
        char::is_control(c)
            || matches!(
                c as u32,
                0x200B..=0x200F | 0x202A..=0x202E | 0x2066..=0x2069
            )
    }) {
        return false;
    }
    // Windows молча съедает хвостовые '.'/' ': заявленный pattern перестал бы
    // совпадать с реальным именем на диске — такой файл пропускаем.
    if name != name.trim_end_matches(['.', ' ']) {
        return false;
    }
    // Зарезервированные имена DOS (до первой точки, регистр не важен):
    // CON/PRN/AUX/NUL/COM1-9/LPT1-9 Windows трактует как устройства.
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return false;
    }
    if upper.len() == 4 {
        let (prefix, digit) = upper.split_at(3);
        if matches!(prefix, "COM" | "LPT")
            && matches!(digit.as_bytes()[0], b'1'..=b'9')
        {
            return false;
        }
    }
    true
}

/// Файл уже лежит на диске: пропускаем ли мы его? Если в конфиге нет sha1 —
/// да (exists() достаточно, прежнее поведение). Если есть — сверяем с диском:
/// побитый или подменённый файл перекачиваем, а не считаем установленным.
fn existing_file_matches(dest: &Path, config_sha1: Option<&str>) -> bool {
    match config_sha1 {
        None => true,
        Some(want) => match crate::util::fs::sha1_file(&crate::util::fs::long_path(dest)) {
            Ok(actual) => actual.eq_ignore_ascii_case(want),
            // Нечитаемый файл (каталог, права) — считаем несовпадением.
            Err(_) => false,
        },
    }
}

/// Предустановка модов MissingModsChecker: каждый пункт конфига ищем на
/// Modrinth по displayName и предустанавливаем файл с ТОЧНО именем pattern
/// по хэшам с cdn.modrinth.com — в общий прогон загрузок. Не найден, нет
/// хэшей, подозрительное имя или неподдерживаемый каталог — warn и пропуск:
/// установка не роняется никогда, игру мод докачает сам, как раньше.
async fn satisfy_missing_mods_checker(
    client: &HttpClient,
    entries: &[MissingModsEntry],
    mc_dir: &Path,
    group: &str,
) -> Vec<DownloadTask> {
    let mut tasks = Vec::new();
    if entries.is_empty() {
        return tasks;
    }
    // F16: офлайн — всё равно ничего не скачается; игра докачает сама.
    if client.offline() {
        tracing::warn!("missing-mods-checker: офлайн-режим — предустановка пропущена");
        return tasks;
    }
    for entry in entries {
        // Каталог — только фиксированный список: чужой конфиг не выберет путь.
        if !matches!(
            entry.destination.as_str(),
            "mods" | "resourcepacks" | "shaderpacks"
        ) {
            tracing::warn!(
                "missing-mods-checker: destination {:?} не поддерживается — пропускаю {}",
                entry.destination,
                entry.pattern
            );
            continue;
        }
        if !is_safe_single_filename(&entry.pattern) {
            tracing::warn!(
                "missing-mods-checker: небезопасное имя файла {:?} — пропускаю",
                entry.pattern
            );
            continue;
        }
        if !display_name_ok(&entry.display_name) {
            tracing::warn!(
                "missing-mods-checker: displayName пустой или длиннее {MAX_MM_DISPLAY_NAME} символов — пропускаю {}",
                entry.pattern
            );
            continue;
        }
        let dest = mc_dir.join(&entry.destination).join(&entry.pattern);
        if crate::util::fs::long_path(&dest).exists()
            && existing_file_matches(&dest, entry.sha1.as_deref())
        {
            continue; // уже на диске (и хэш из конфига сошёлся) — сеть не трогаем
        }
        let hits = match search_mods(client, &entry.display_name, 5).await {
            Ok(r) => r.hits,
            Err(e) => {
                tracing::warn!(
                    "missing-mods-checker: поиск {:?} не удался ({e}) — пропускаю {}",
                    entry.display_name,
                    entry.pattern
                );
                continue;
            }
        };
        // Первые 3 хита: файл должен совпасть точным именем; имена файлов на
        // Modrinth общепринятые, поэтому обычно хватает первого же проекта.
        let mut found: Option<DownloadTask> = None;
        for hit in hits.iter().take(3) {
            let versions = match project_versions(client, &hit.project_id).await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let Some(file) = versions.iter().find_map(|v| {
                v.files.iter().find(|f| f.filename == entry.pattern)
            }) else {
                continue;
            };
            // Железное правило: без хэша из доверенного API не качаем.
            let sha1 = file.hashes.get("sha1").cloned();
            let sha512 = file.hashes.get("sha512").cloned();
            if sha1.is_none() && sha512.is_none() {
                tracing::warn!(
                    "missing-mods-checker: у файла {} на Modrinth нет хэша — не качаю непроверенное",
                    file.filename
                );
                continue;
            }
            found = Some(DownloadTask {
                id: format!("mrpack-mm:{}", entry.pattern),
                url: file.url.clone(),
                dest,
                sha1,
                sha512,
                size: Some(file.size),
                group: group.to_string(),
                priority: 15,
            });
            break;
        }
        match found {
            Some(t) => tasks.push(t),
            None => tracing::warn!(
                "missing-mods-checker: {} нет на Modrinth — оставляем игре",
                entry.pattern
            ),
        }
    }
    tasks
}

/// D64: projectId/versionId КОНКРЕТНОГО файла из его CDN-ссылки
/// (`…/data/<project>/versions/<version>/<filename>`). Запасной источник для
/// content-manifest: официальная спека не требует этих полей в files[], не
/// все паки их пишут. Паковый version_id (index.version_id) сюда НЕ годится:
/// это версия пака, и сверка с ней считала бы весь свежий пак «устаревшим».
fn ids_from_cdn_url(url: &str) -> (Option<String>, Option<String>) {
    const MARKER: &str = "cdn.modrinth.com/data/";
    let Some(rest) = url.split(MARKER).nth(1) else {
        return (None, None);
    };
    let mut seg = rest.split('/');
    let project = seg.next().filter(|s| !s.is_empty()).map(String::from);
    let version = match (seg.next(), seg.next()) {
        (Some("versions"), Some(v)) if !v.is_empty() => Some(v.to_string()),
        _ => None,
    };
    (project, version)
}

/// Полная установка .mrpack: инстанс по dependencies → overrides поверх →
/// файлы с хэшами → content-manifest. Возвращает созданный инстанс.
///
/// Движок загрузок приходит снаружи (и со своей шиной — D11: собственный
/// `EventBus::default()` здесь означал, что `dl_progress` не доходил до UI).
/// `group` — тот же ключ, под которым движок лежит в реестре AppState; он же
/// группа задач, поэтому отмена группы отменяет именно эту установку.
pub async fn install_mrpack(
    paths: &Paths,
    settings: &crate::settings::Settings,
    engine: Arc<DownloadEngine>,
    client: Arc<HttpClient>,
    group: &str,
    mrpack_path: &Path,
    name_override: Option<&str>,
) -> Result<Instance> {
    let index = open_mrpack(mrpack_path)?;

    // 1. Инстанс нужного вида: dependencies {minecraft, fabric-loader?, …}.
    let mc = index.dependencies.get("minecraft").ok_or_else(|| {
        LauncherError::InvalidInput("в модпаке нет зависимости minecraft".into())
    })?;
    let mut inst = Instance::new(name_override.unwrap_or(&index.name), mc);
    inst.mc_version = mc.clone();

    // 2. Загрузчик разбираем ДО скачивания: неизвестный загрузчик не должен
    //    стоить пользователю сотен мегабайт трафика (спека §15). fabric/quilt —
    //    профиль JSON, forge/neoforge — headless-инсталлятор, оба пути ведёт
    //    run::install_loader.
    let loader: Option<(&str, &str)> = resolve_loader(&index.dependencies)?;

    // 3. Файлы модпака в очередь (до создания инстанса на диске — каталоги
    //    создаст движок).
    let mc_dir = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
    let mut tasks = Vec::new();
    for f in index.client_files() {
        if f.downloads.is_empty() {
            return Err(LauncherError::InvalidInput(format!(
                "у файла {} нет ссылок для скачивания",
                f.path
            )));
        }
        // A24: сначала проверяем все зеркала файла на allowlist (SSRF), потом
        // уже строим задачу — до сети и до создания каталогов.
        for url in &f.downloads {
            ensure_allowed_mrpack_url(url)?;
        }
        // Санитизация пути из ЧУЖОГО модпака (спека §3): только относительные
        // безопасные компоненты.
        let rel = crate::util::zip::safe_relative_path(&f.path)?;
        // Железное правило: каждый байт сверяется с хэшем доверенного
        // манифеста. Modrinth отдаёт sha1 и/или sha512 — без обоих не качаем.
        let sha1 = f.hashes.get("sha1").cloned();
        let sha512 = f.hashes.get("sha512").cloned();
        if sha1.is_none() && sha512.is_none() {
            return Err(LauncherError::InvalidInput(format!(
                "в манифесте нет хэша для {}",
                f.path
            )));
        }
        tasks.push(DownloadTask {
            id: format!("mrpack:{}", f.path),
            url: f.downloads[0].clone(),
            dest: mc_dir.join(rel),
            sha1,
            sha512,
            size: Some(f.file_size),
            group: group.to_string(),
            priority: 15,
        });
    }
    // 3b. MissingModsChecker (Better MC и похожие): моды, которых нет в
    //    files[], но которые игра качает сама с CurseForge при первом запуске.
    //    Честная предустановка: те же имена файлов ищем на Modrinth и качаем
    //    по хэшам в ТОТ ЖЕ прогон — лишнего окна и повторной установки нет.
    let mm = read_missing_mods_checker(mrpack_path);
    tasks.extend(satisfy_missing_mods_checker(&client, &mm, &mc_dir, group).await);

    // 4. Загрузка файлов. На диске инстанса ещё нет: сохраняем его только после
    //    успеха (D12), иначе сбой оставил бы «пустой рабочий» инстанс.
    engine.add_tasks(tasks);
    engine.run().await?;
        let st = engine.queue_state();
    if st.failed > 0 {
        rollback_instance(paths, &inst.id);
        return Err(LauncherError::network(format!(
            "{} файлов модпака не скачались: {}",
            st.failed,
            st.failed_items
                .first()
                .map(|(u, r)| format!("{u}: {r}"))
                .unwrap_or_default()
        )));
    }
    if st.cancelled > 0 {
        // Отмена посреди установки: файлов нет — инстанса тоже быть не должно.
        rollback_instance(paths, &inst.id);
        return Err(LauncherError::Cancelled);
    }

    // 5–6. Инстанс на диске, загрузчик, overrides. A29: сбой на любом из этих
    // шагов откатывает инстанс — до этого момента пользователь его не видел,
    // а битый (без загрузчика/overrides) остался бы в списке навсегда.
    // Наружу уходит исходная причина сбоя, откат только логируется.
    let inst = match install_loader_and_overrides(paths, settings, client, &inst, loader, mrpack_path)
        .await
    {
        Ok(updated) => updated,
        Err(e) => {
            tracing::warn!("установка модпака не удалась ({e}) — откатываю инстанс");
            rollback_instance(paths, &inst.id);
            return Err(e);
        }
    };

    // 7. content-manifest: только моды/рп/шейдеры (пути не в mods/ — local).
    let mut manifest = Vec::new();
    for f in index.client_files() {
        let rel = crate::util::zip::safe_relative_path(&f.path)?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let kind = if rel_str.starts_with("mods/") {
            ContentKind::Mod
        } else if rel_str.starts_with("resourcepacks/") {
            ContentKind::ResourcePack
        } else if rel_str.starts_with("shaderpacks/") {
            ContentKind::Shader
        } else {
            continue; // конфиги и прочее — не контент для обновлений
        };
        // D64: projectId/versionId КОНКРЕТНОГО файла, не пака: спека их не
        // описывает, Modrinth App пишет — иначе добираем из CDN-ссылки. Когда
        // нет ни в JSON, ни в ссылке, запись остаётся без version_id и
        // обновления по ней не проверяются (как legacy-записи) — писать
        // версию ПАКА нельзя: check_updates сверяет её с версией мода и
        // перекачивал бы весь свежий пак целиком.
        let (url_project, url_version) = f
            .downloads
            .first()
            .map(|u| ids_from_cdn_url(u))
            .unwrap_or((None, None));
        manifest.push(ContentEntry {
            kind,
            file: rel_str,
            source: ContentSource::Modrinth,
            project_id: f.project_id.clone().or(url_project),
            version_id: f.version_id.clone().or(url_version),
            sha1: f.hashes.get("sha1").cloned(),
            url: f.downloads.first().cloned(),
            enabled: true,
        });
    }
    crate::instances::save_content_manifest(paths, &inst.id, &manifest)?;
    Ok(inst)
}

/// Откат несостоявшейся установки: каталог инстанса создан движком под свежий
/// uuid, пользователь его не видел (в списке инстансов он не показывался) —
/// удаляем целиком, вместе с недокачанными `.part` (D12). Ошибку удаления
/// только логируем: важнее вернуть исходную причину сбоя.
fn rollback_instance(paths: &Paths, id: &str) {
    let dir = crate::instances::instance_dir(paths, id);
    if let Err(e) = std::fs::remove_dir_all(crate::util::fs::long_path(&dir)) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("откат инстанса {id}: {} не удалён: {e}", dir.display());
        }
    }
}

/// Шаги 5–6 установки модпака: сохранить инстанс, поставить загрузчик
/// (install_loader сам перезаписывает instance.json), накатить overrides.
/// Отдельная функция — чтобы вызывающий мог откатить инстанс при ЛЮБОЙ ошибке
/// одним местом (A29) и вернуть исходную причину.
async fn install_loader_and_overrides(
    paths: &Paths,
    settings: &crate::settings::Settings,
    client: Arc<HttpClient>,
    inst: &Instance,
    loader: Option<(&str, &str)>,
    mrpack_path: &Path,
) -> Result<Instance> {
    crate::instances::save(paths, inst)?;
    let mut updated = inst.clone();
    if let Some((loader, loader_version)) = loader {
        // Версию загрузчика можно не указывать: install_loader берёт latest
        // стабильную, если None — но модпак требует конкретную.
        updated = crate::instances::run::install_loader(
            paths,
            settings,
            client,
            inst,
            loader,
            Some(loader_version),
        )
        .await?;
    }
    // Overrides из .mrpack поверх каталога игры (zip-slip-защита в extract).
    apply_overrides(mrpack_path, paths, &updated.id)?;
    Ok(updated)
}

/// Распаковать `overrides/` из .mrpack поверх каталога игры.
fn apply_overrides(mrpack_path: &Path, paths: &Paths, instance_id: &str) -> Result<()> {
    let file = std::fs::File::open(crate::util::fs::long_path(mrpack_path))?;
    let dest = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, instance_id));
    let extracted = crate::util::zip::extract_zip_stripping(
        file,
        &dest,
        "overrides/",
        |_| true,
        |_| {},
    )?;
    tracing::info!("overrides применены: {} файлов", extracted.len());
    Ok(())
}

/// Установка модпака Modrinth по project_id: берём последнюю версию,
/// качаем primary-файл во временный .mrpack (sha1 проверяем — правило
/// «хэш на каждый байт») и переиспользуем install_mrpack.
///
/// Движок/шина и `group` — снаружи, как и у install_mrpack (D11); откат
/// несостоявшегося инстанса живёт в install_mrpack (до него инстанса ещё нет).
pub async fn install_modpack_project(
    paths: &Paths,
    settings: &crate::settings::Settings,
    engine: Arc<DownloadEngine>,
    client: Arc<HttpClient>,
    group: &str,
    project_id: &str,
    name_override: Option<&str>,
) -> Result<Instance> {
    let versions = crate::modrinth::api::project_versions(&client, project_id).await?;
    let version = versions.first().ok_or_else(|| {
        LauncherError::InvalidInput(format!("у модпака {project_id} нет версий"))
    })?;
    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| {
            LauncherError::InvalidInput(format!(
                "у версии {} нет файлов",
                version.version_number
            ))
        })?;

    // F16: офлайн-режим — мгновенный честный отказ вместо сетевого таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode(file.url.clone()));
    }
    // D62: send «сырого» запроса под предохранителем send_timed (30 с) —
    // сервер, принявший соединение и молчащий, не подвешивает установку.
    let resp = client
        .send_timed(client.raw().get(&file.url))
        .await?
        .error_for_status()?;

    // D25: архив модпака бывает 500 МБ+ — качаем потоком в temp-файл и
    // целиком в память не берём; sha1 считаем по готовому файлу (sha1_file).
    let tmp_dir = std::env::temp_dir().join("mc-launcher-v2");
    std::fs::create_dir_all(crate::util::fs::long_path(&tmp_dir))?;
    // D62: tmp-имя уникально на установку (uuid4-хвост в имени файла): раньше
    // две параллельные установки ОДНОГО проекта писали в общий
    // temp/mc-launcher-v2/{filename} — второй поток усекал недокачанный файл
    // первого, финальный sha1 ловил гонку. Файл по-прежнему удаляется после
    // установки и при сбоях ниже — меняется только сам путь, не жизненный цикл.
    let tmp_tail = uuid::Uuid::new_v4().simple().to_string();
    // D64: filename из API — не доверенный путь: санитайзер (как в install/
    // updates) не даст «имени» вида `../../x` вынести temp наружу каталога.
    let tmp_name = crate::modrinth::api::sanitize_file_name(&file.filename);
    let tmp_path =
        crate::util::fs::long_path(&tmp_dir.join(format!("{}-{tmp_name}", &tmp_tail[..8])));
    let wrote = async {
        use futures::StreamExt;
        use std::io::Write as _;
        let mut stream = resp.bytes_stream();
        let mut out = std::fs::File::create(&tmp_path)?;
        let mut total: u64 = 0;
        // D62: каждый шаг чтения ограничен READ_IDLE (зеркало движка загрузок
        // и adoptium.rs) — тишина дольше двух минут = «сервер не отвечает»,
        // честная сетевая ошибка вместо вечного висения; tmp-файл ниже сносится.
        while let Some(chunk) = tokio::time::timeout(READ_IDLE, stream.next())
            .await
            .map_err(|_| {
                LauncherError::network(format!(
                    "сервер не отвечает более {} с: {}",
                    READ_IDLE.as_secs(),
                    file.url
                ))
            })?
        {
            let chunk = chunk?;
            out.write_all(&chunk)?;
            total += chunk.len() as u64;
        }
        out.flush()?;
        if total == 0 {
            return Err(LauncherError::network(format!("пустой ответ для {}", file.url)));
        }
        Ok(())
    }
    .await;
    if let Err(e) = wrote {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    if let Some(sha) = file.hashes.get("sha1") {
        let actual = crate::util::fs::sha1_file(&tmp_path)?;
        if !actual.eq_ignore_ascii_case(sha) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(LauncherError::InvalidInput(format!(
                "sha1 не сошёлся для {}: ждали {sha}, получили {actual}",
                file.filename
            )));
        }
    }

    let installed = install_mrpack(
        paths,
        settings,
        engine,
        client.clone(),
        group,
        &tmp_path,
        name_override,
    )
    .await;
    let _ = std::fs::remove_file(&tmp_path);
    let inst = installed?;

    // Иконка и арт проекта — некритичный шаг: любой сбой (нет icon_url, HTTP,
    // размер/формат) оставляет инстанс со стандартной заглушкой и НЕ роняет
    // установку (спека §15). В самом .mrpack их нет по спеке, поэтому
    // install_mrpack (drag&drop) этот шаг не делает.
    apply_project_artwork(paths, &client, project_id, &inst.id).await;
    Ok(inst)
}

/// Иконка + большой арт проекта Modrinth → data-URL в instance.json. Ровно
/// ОДИН запрос /project/{id} на обе картинки: icon_url — миниатюра 96×96 (на
/// карточке ~350px мылится), большой арт берём из первой картинки галереи
/// (raw_url). Любая ошибка — warn и продолжение: картинки не стоят сорванной
/// установки (спека §15).
async fn apply_project_artwork(
    paths: &Paths,
    client: &HttpClient,
    project_id: &str,
    instance_id: &str,
) {
    // id уходит в URL запроса: мусор отсекаем до сети (арт — некритичный шаг,
    // любая ошибка ниже — warn и продолжение установки).
    if let Err(e) = crate::modrinth::api::validate_id_or_slug(project_id) {
        tracing::warn!("иконка/арт модпака {project_id} не установлены: {e}");
        return;
    }
    let project = match crate::modrinth::api::project(client, project_id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("иконка/арт модпака {project_id} не установлены: {e}");
            return;
        }
    };
    if let Err(e) = apply_icon_from_project(paths, client, &project, instance_id).await {
        tracing::warn!("иконка модпака {project_id} не установлена: {e}");
    }
    if let Err(e) = apply_art_from_project(paths, client, &project, instance_id).await {
        tracing::warn!("арт модпака {project_id} не установлен: {e}");
    }
}

async fn apply_icon_from_project(
    paths: &Paths,
    client: &HttpClient,
    project: &serde_json::Value,
    instance_id: &str,
) -> Result<()> {
    // ~300 КБ — зеркало MAX_ICON_BYTES в instances/content (там константа
    // приватная; set_icon_from_bytes перепроверит лимит по сигнатуре).
    const MAX_ICON_BYTES: usize = 300 * 1024;
    let icon_url = project
        .get("icon_url")
        .and_then(|v| v.as_str())
        .filter(|u| !u.is_empty())
        .ok_or_else(|| LauncherError::NotFound("у проекта нет icon_url".into()))?;
    let Some(bytes) = fetch_image_capped(client, icon_url, MAX_ICON_BYTES).await? else {
        return Ok(()); // тяжелее лимита — warn уже в хелпере, это не ошибка
    };
    crate::instances::content::set_icon_from_bytes(paths, instance_id, &bytes)?;
    Ok(())
}

/// Арт: первая картинка галереи (raw_url — полный размер, запасная url —
/// превью, как в api::gallery_image). Тяжелее лимита — warn и пропустить
/// (галерейные кадры бывают на несколько МБ), это не ошибка.
async fn apply_art_from_project(
    paths: &Paths,
    client: &HttpClient,
    project: &serde_json::Value,
    instance_id: &str,
) -> Result<()> {
    use crate::instances::content::MAX_ART_BYTES;
    let art_url = project
        .get("gallery")
        .and_then(|g| g.as_array())
        .and_then(|g| g.first())
        .and_then(|img| {
            img.get("raw_url")
                .and_then(|v| v.as_str())
                .or_else(|| img.get("url").and_then(|v| v.as_str()))
        })
        .filter(|u| !u.is_empty())
        .ok_or_else(|| LauncherError::NotFound("у проекта нет галереи".into()))?;
    let Some(bytes) = fetch_image_capped(client, art_url, MAX_ART_BYTES).await? else {
        return Ok(()); // тяжелее лимита — warn уже в хелпере, это не ошибка
    };
    crate::instances::content::set_art_from_bytes(paths, instance_id, &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventBus, LauncherEvent};
    use crate::settings::Settings;
    use std::io::Write as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Тело «мода» тестового модпака (sha1 считается по этим байтам).
    const MOD_BYTES: &[u8] = b"mrpack test payload";

    /// Тело предустанавливаемого «мода» MissingModsChecker и его точное имя
    /// файла (как у Better MC: `balm-fabric-1.20.1-7.3.38.jar`).
    const MM_BYTES: &[u8] = b"missing-mods-checker payload";
    const MM_PATTERN: &str = "balm-fabric-1.20.1-7.3.38.jar";

    /// Поднять mock-сервер, вернуть базовый URL (как в tests/download_engine.rs):
    /// сети нет — только localhost.
    async fn spawn_axum(app: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn test_paths(dir: &tempfile::TempDir) -> Paths {
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        paths
    }

    /// Минимально валидный .mrpack: zip с modrinth.index.json.
    fn write_mrpack(path: &Path, index_json: &str) {
        write_mrpack_entries(path, index_json, &[]);
    }

    /// .mrpack с произвольными записями (для проверок overrides).
    fn write_mrpack_entries(path: &Path, index_json: &str, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::SimpleFileOptions = Default::default();
        zip.start_file("modrinth.index.json", opts).unwrap();
        zip.write_all(index_json.as_bytes()).unwrap();
        for (name, data) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    /// Индекс без файлов (нужен там, где проверяется только шаги 5–6).
    fn index_without_files(name: &str) -> String {
        format!(
            r#"{{"formatVersion":1,"game":"minecraft","versionId":"test-1",
                 "name":"{name}","files":[],
                 "dependencies":{{"minecraft":"1.21.1"}}}}"#
        )
    }

    fn index_with_mod(url: &str, sha1: &str) -> String {
        format!(
            r#"{{"formatVersion":1,"game":"minecraft","versionId":"test-1",
                 "name":"Тест-пак",
                 "files":[{{"path":"mods/x.jar","hashes":{{"sha1":"{sha1}"}},
                            "downloads":["{url}"],"fileSize":{}}}],
                 "dependencies":{{"minecraft":"1.21.1"}}}}"#,
            MOD_BYTES.len()
        )
    }

    /// Каталоги в `instances/` — «пустой рабочий инстанс» виден именно здесь.
    fn instance_dirs(paths: &Paths) -> Vec<String> {
        let mut dirs: Vec<String> =
            std::fs::read_dir(crate::util::fs::long_path(&paths.instances_dir()))
                .map(|it| {
                    it.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
        dirs.sort();
        dirs
    }

    /// Регрессия: ключи `forge`/`neoforge` в dependencies (спека §6.2, без
    /// суффикса `-loader` — пример BMC4: {"minecraft":"1.20.1","forge":"47.3.0"})
    /// резолвятся в загрузчик с версией, а не молча пропускались (пак вставал
    /// как vanilla). Реальную установку Forge не гоняем — ей нужна сеть.
    #[test]
    fn resolves_forge_and_neoforge_loaders() {
        let deps: HashMap<String, String> =
            serde_json::from_str(r#"{"minecraft":"1.20.1","forge":"47.3.0"}"#).unwrap();
        let (name, version) = resolve_loader(&deps).unwrap().unwrap();
        assert_eq!(name, "forge");
        assert_eq!(version, "47.3.0");

        let deps: HashMap<String, String> =
            serde_json::from_str(r#"{"minecraft":"1.20.4","neoforge":"20.4.237"}"#).unwrap();
        let (name, version) = resolve_loader(&deps).unwrap().unwrap();
        assert_eq!(name, "neoforge");
        assert_eq!(version, "20.4.237");
    }

    /// Прежнее поведение не сломано: fabric/quilt через `*-loader`, пакет без
    /// загрузчика — None.
    #[test]
    fn resolves_fabric_quilt_and_loaderless_pack() {
        for (key, name) in [("fabric-loader", "fabric"), ("quilt-loader", "quilt")] {
            let deps: HashMap<String, String> = serde_json::from_str(&format!(
                r#"{{"minecraft":"1.21.1","{key}":"0.16.0"}}"#
            ))
            .unwrap();
            let (n, v) = resolve_loader(&deps).unwrap().unwrap();
            assert_eq!(n, name);
            assert_eq!(v, "0.16.0");
        }
        let deps: HashMap<String, String> =
            serde_json::from_str(r#"{"minecraft":"1.21.1"}"#).unwrap();
        assert!(
            resolve_loader(&deps).unwrap().is_none(),
            "без загрузчика — None (vanilla)"
        );
    }

    /// Неизвестный `*-loader` — честная ошибка ДО скачиваний (спека §15),
    /// посторонние ключи зависимостей резолвер не смущают.
    #[test]
    fn unknown_loader_key_is_honest_error() {
        let deps: HashMap<String, String> =
            serde_json::from_str(r#"{"minecraft":"1.21.1","weird-loader":"1.0"}"#).unwrap();
        let err = resolve_loader(&deps).unwrap_err();
        assert_eq!(err.code(), "invalid_input", "{err}");
        assert!(err.to_string().contains("weird-loader"), "{err}");

        let deps: HashMap<String, String> =
            serde_json::from_str(r#"{"minecraft":"1.21.1","custom":"1.0"}"#).unwrap();
        assert!(
            resolve_loader(&deps).unwrap().is_none(),
            "посторонний ключ без -loader игнорируется"
        );
    }

    /// Битый .mrpack: инстанса не появляется вовсе.
    #[tokio::test]
    async fn broken_mrpack_leaves_no_instance_dir() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let broken = dir.path().join("broken.mrpack");
        std::fs::write(&broken, "это не zip".as_bytes()).unwrap();

        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let err = install_mrpack(
            &paths,
            &Settings::default(),
            engine,
            client,
            "mrpack:test",
            &broken,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, LauncherError::Zip(_)), "{err}");
        assert!(
            instance_dirs(&paths).is_empty(),
            "список инстансов должен остаться пустым"
        );
    }

    /// Сбой загрузки файлов модпака: каталог несостоявшегося инстанса удаляется
    /// целиком (D12) — «пустого рабочего» инстанса в списке быть не должно.
    #[tokio::test]
    async fn failed_download_rolls_back_instance_dir() {
        // 404 на любой путь: задача падает сразу, без повторов.
        let app = axum::Router::new().fallback(|| async { axum::http::StatusCode::NOT_FOUND });
        let base = spawn_axum(app).await;

        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let pack = dir.path().join("pack.mrpack");
        write_mrpack(
            &pack,
            &index_with_mod(
                &format!("{base}/mods/x.jar"),
                "0000000000000000000000000000000000000000",
            ),
        );

        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let err = install_mrpack(
            &paths,
            &Settings::default(),
            engine,
            client,
            "mrpack:test",
            &pack,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, LauncherError::Network(_)), "{err}");
        assert!(
            instance_dirs(&paths).is_empty(),
            "после сбоя загрузки каталога инстанса быть не должно"
        );
    }

    /// Прогресс уходит в шину вызывающего (D11), а инстанс сохраняется только
    /// после успешной загрузки (D12). Ожидание — по событиям, а не по sleep (A42):
    /// сервер отдаёт тело только после того, как тест увидел первый dl_progress.
    #[tokio::test]
    async fn progress_goes_to_caller_bus_and_instance_saved_after_download() {
        let gate = Arc::new(tokio::sync::Notify::new());
        let gate_srv = gate.clone();
        let app = axum::Router::new().route(
            "/mods/x.jar",
            axum::routing::get(move || {
                let gate = gate_srv.clone();
                async move {
                    // Тикер движка шлёт dl_progress раз в 100 мс, но на локальном
                    // диске загрузка успевает завершиться раньше — поэтому держим
                    // ответ до сигнала теста (таймаут — предохранитель).
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        gate.notified(),
                    )
                    .await;
                    axum::body::Body::from(MOD_BYTES.to_vec())
                }
            }),
        );
        let base = spawn_axum(app).await;

        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let pack = dir.path().join("pack.mrpack");
        write_mrpack(
            &pack,
            &index_with_mod(
                &format!("{base}/mods/x.jar"),
                &crate::util::fs::sha1_bytes(MOD_BYTES),
            ),
        );

        let bus = EventBus::new(64);
        let mut rx = bus.subscribe();
        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, bus);
        let paths_task = paths.clone();
        let pack_task = pack.clone();
        let install = tokio::spawn(async move {
            install_mrpack(
                &paths_task,
                &Settings::default(),
                engine,
                client,
                "mrpack:test",
                &pack_task,
                None,
            )
            .await
        });

        // Ждём первый dl_progress: до сигнала загрузка висит, поэтому условие
        // достижимо (не зависит от скорости диска и загрузки машины).
        let mut progress = 0;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while progress == 0 && std::time::Instant::now() < deadline {
            match rx.try_recv() {
                Ok(LauncherEvent::DlProgress(_)) => progress += 1,
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(_) => break,
            }
        }
        gate.notify_one(); // отпускаем загрузку

        let inst = install.await.unwrap().unwrap();
        assert!(progress > 0, "dl_progress должен идти в шину вызывающего");
        assert_eq!(
            crate::instances::list(&paths).len(),
            1,
            "инстанс сохраняется после успешной загрузки"
        );
        let game =
            crate::instances::minecraft_dir(&crate::instances::instance_dir(&paths, &inst.id));
        assert!(game.join("mods").join("x.jar").exists(), "файл мода на месте");
    }

    /// A24: URL из чужого .mrpack проходит проверку хоста (SSRF).
    #[test]
    fn mrpack_download_hosts_are_allowlisted() {
        for url in [
            "http://169.254.169.254/latest/meta-data/", // метаданные облака
            "https://169.254.169.254/latest/meta-data/",
            "https://evil.example/mod.jar",
            "http://cdn.modrinth.com/data/x/y.jar", // только https
            "file:///C:/Windows/System32/calc.exe",
            "https://user:pw@evil.example/x.jar", // userinfo не обманывает allowlist
            "не-url",
        ] {
            let err = ensure_allowed_mrpack_url(url).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "{url}: {err}");
        }
        for url in [
            "https://api.modrinth.com/v2/version_file/x",
            "https://CDN.modrinth.com/data/x/y.jar", // регистр не важен
            "https://github.com/a/b/releases/download/v1/m.jar",
            "https://objects.githubusercontent.com/github-production/x",
            "https://raw.githubusercontent.com/a/b/main/x.jar",
        ] {
            ensure_allowed_mrpack_url(url).unwrap_or_else(|e| panic!("{url}: {e}"));
        }
    }

    /// A24: модпак со ссылкой на внутренний хост отвергается ДО сети и до
    /// создания каталогов инстанса.
    #[tokio::test]
    async fn mrpack_with_ssrf_url_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let pack = dir.path().join("ssrf.mrpack");
        write_mrpack(
            &pack,
            &index_with_mod(
                "http://169.254.169.254/latest/meta-data/iam/security-credentials/",
                "0000000000000000000000000000000000000000",
            ),
        );

        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let err = install_mrpack(
            &paths,
            &Settings::default(),
            engine,
            client,
            "mrpack:test",
            &pack,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), "invalid_input", "{err}");
        assert!(
            instance_dirs(&paths).is_empty(),
            "недоверенный модпак не должен создавать инстанс"
        );
    }

    /// A29: сбой overrides (запись с `..` внутри overrides/) уже ПОСЛЕ сохранения
    /// инстанса — каталог обязан быть откачен, а исходная ошибка — видна наружу.
    #[tokio::test]
    async fn overrides_failure_rolls_back_instance_dir() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let pack = dir.path().join("broken-overrides.mrpack");
        write_mrpack_entries(
            &pack,
            &index_without_files("Битые overrides"),
            &[("overrides/../evil.txt", b"X")],
        );

        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let err = install_mrpack(
            &paths,
            &Settings::default(),
            engine,
            client,
            "mrpack:test",
            &pack,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), "zip_slip", "{err}");
        assert!(
            instance_dirs(&paths).is_empty(),
            "инстанс не должен остаться после сбоя overrides"
        );
    }

    /// Разбор конфига MissingModsChecker: camelCase displayName (alias —
    /// snake_case), посторонние поля (включая curseforge-url) игнорируются;
    /// sha1 из конфига (не обязателен) разбирается для сверки с диском.
    #[test]
    fn missing_mods_config_parses_ignoring_url() {
        let entries: Vec<MissingModsEntry> = serde_json::from_str(
            r#"[{"displayName":"Balm","pattern":"balm.jar",
                 "url":"https://www.curseforge.com/minecraft/mc-mods/balm",
                 "sha1":"abc123","destination":"mods","someExtra":42}]"#,
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].display_name, "Balm");
        assert_eq!(entries[0].pattern, "balm.jar");
        assert_eq!(entries[0].destination, "mods");
        assert_eq!(entries[0].sha1.as_deref(), Some("abc123"));

        let entries: Vec<MissingModsEntry> = serde_json::from_str(
            r#"[{"displayName":"Balm","pattern":"balm.jar","destination":"mods"}]"#,
        )
        .unwrap();
        assert_eq!(entries[0].sha1, None, "sha1 в конфиге не обязателен");
    }

    /// zip-запись, распакованно больше капа манифеста (16 МБ): ~17 МБ повторов,
    /// deflate сжимает в килобайты — тест быстрый.
    fn write_oversized_entry(path: &Path, entry_name: &str) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::SimpleFileOptions = Default::default();
        zip.start_file(entry_name, opts).unwrap();
        let chunk = [b'a'; 64 * 1024];
        let mut written = 0u64;
        while written <= MAX_MRPACK_MANIFEST_BYTES {
            zip.write_all(&chunk).unwrap();
            written += chunk.len() as u64;
        }
        zip.finish().unwrap();
    }

    /// P1: гигантский modrinth.index.json отвергается ДО чтения в память
    /// (зеркало D23): ошибка — InvalidInput именно про размер, а не serde-разбор.
    #[test]
    fn oversized_manifest_rejected_without_reading() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("big.mrpack");
        write_oversized_entry(&pack, "modrinth.index.json");
        let err = open_mrpack(&pack).unwrap_err();
        assert_eq!(err.code(), "invalid_input", "{err}");
        assert!(err.to_string().contains("слишком большой"), "{err}");
    }

    /// Тот же кап для missing_mods_checker.json: warn и пусто — гигабайтный
    /// конфиг из чужого архива в память не читается.
    #[test]
    fn oversized_missing_mods_config_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("big-config.mrpack");
        write_oversized_entry(&pack, "overrides/config/missing_mods_checker.json");
        assert!(read_missing_mods_checker(&pack).is_empty());
    }

    /// P1: кап на число файлов в манифесте — десятки тысяч записей не проходят
    /// validate (легальные паки — сотни, остальное — мусор или DoS).
    #[test]
    fn index_with_too_many_files_is_rejected() {
        let mut index: MrpackIndex =
            serde_json::from_str(&index_without_files("Много файлов")).unwrap();
        index.files = (0..=MAX_INDEX_FILES)
            .map(|i| MrpackFile {
                path: format!("mods/f{i}.jar"),
                hashes: HashMap::new(),
                env: None,
                downloads: vec!["https://cdn.modrinth.com/x.jar".to_string()],
                file_size: 1,
                project_id: None,
                version_id: None,
            })
            .collect();
        let err = index.validate().unwrap_err();
        assert_eq!(err.code(), "invalid_input", "{err}");
        assert!(err.to_string().contains("слишком много файлов"), "{err}");
    }

    /// P2: картинки проекта (icon_url/галерейный raw_url) тянутся только с
    /// CDN Modrinth — чужой URL из ответа API (или тестового мока) не уводит
    /// GET на произвольный хост (SSRF-гигиена, как A24 у .mrpack).
    #[test]
    fn art_urls_are_cdn_only() {
        for url in [
            "http://cdn.modrinth.com/x.png", // только https
            "https://evil.example/x.png",
            "https://api.modrinth.com/x.png", // API-хост — не CDN
            "https://cdn.modrinth.com.evil.ru/x.png",
            "не-url",
        ] {
            let err = ensure_allowed_cdn_url(url).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "{url}: {err}");
        }
        for url in [
            "https://cdn.modrinth.com/data/AANobbMI/1.png",
            "https://CDN.modrinth.com/data/x/y.webp", // регистр не важен
        ] {
            ensure_allowed_cdn_url(url).unwrap_or_else(|e| panic!("{url}: {e}"));
        }
    }

    /// Санитизация имени из чужого missing_mods_checker.json: разделители пути,
    /// управляющие, невидимый юникод, зарезервированные DOS-имена, хвостовые
    /// '.'/' ' и длина — всё отсекается; нормальные имена проходят.
    #[test]
    fn unsafe_filenames_are_rejected() {
        for ok in [
            "balm-fabric-1.20.1-7.3.38.jar",
            "Some Mod 2.jar",
            "console-helper.jar",
            "companion.mod",
        ] {
            assert!(is_safe_single_filename(ok), "{ok}");
        }
        // прежние запреты: пусто, точки-ссылки, разделители, control-символы
        for bad in [
            "", ".", "..", "a/b.jar", "a\\b.jar", "a:b.jar", "a\0b.jar", "a\u{1}b.jar",
        ] {
            assert!(!is_safe_single_filename(bad), "{bad:?}");
        }
        // (а) невидимый юникод: zero-width, bidi-метки, изоляторы направлений
        for bad in [
            "a\u{200B}b.jar",
            "a\u{200F}b.jar",
            "a\u{202A}b.jar",
            "a\u{202E}b.jar",
            "a\u{2066}b.jar",
            "a\u{2069}b.jar",
        ] {
            assert!(!is_safe_single_filename(bad), "{bad:?}");
        }
        // (б) зарезервированные имена DOS (регистр не важен, стем до точки)
        for bad in [
            "CON", "con.jar", "PrN.txt", "AUX", "NUL.jar", "COM1", "com9.jar", "LPT1",
            "lpt4.jar",
        ] {
            assert!(!is_safe_single_filename(bad), "{bad}");
        }
        // COM10/LPT0 — не из списка COM1-9/LPT1-9: проходят
        assert!(is_safe_single_filename("COM10.jar"));
        assert!(is_safe_single_filename("LPT0.jar"));
        // (в) хвостовые '.'/' ' — Windows их молча съедает
        for bad in ["mod.jar.", "mod.jar ", "mod.jar. ."] {
            assert!(!is_safe_single_filename(bad), "{bad:?}");
        }
        // (г) длина ≤ 255 байт (именно байты UTF-8)
        assert!(is_safe_single_filename(&"x".repeat(255)));
        assert!(!is_safe_single_filename(&"x".repeat(256)));
        assert!(is_safe_single_filename(&"ы".repeat(127)), "254 байта — можно");
        assert!(!is_safe_single_filename(&"ы".repeat(128)), "256 байт — нельзя");
    }

    /// P3: кап на число записей missing_mods_checker — больше 50 берутся
    /// первые (чужой конфиг не управляет объёмом работы лаунчера).
    #[test]
    fn missing_mods_config_entries_are_capped() {
        let entries: Vec<String> = (0..60)
            .map(|i| {
                format!(r#"{{"displayName":"Mod{i}","pattern":"m{i}.jar","destination":"mods"}}"#)
            })
            .collect();
        let config = format!("[{}]", entries.join(","));
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("capped.mrpack");
        write_mrpack_entries(
            &pack,
            &index_without_files("Капы"),
            &[(
                "overrides/config/missing_mods_checker.json",
                config.as_bytes(),
            )],
        );
        let mm = read_missing_mods_checker(&pack);
        assert_eq!(mm.len(), MAX_MM_ENTRIES, "лишние записи отброшены");
        assert_eq!(mm[0].display_name, "Mod0");
        assert_eq!(mm.last().unwrap().display_name, "Mod49");
    }

    /// P3: потолок displayName — пустой или слишком длинный не годится для
    /// поиска (отсекается до сети).
    #[test]
    fn display_name_is_capped() {
        assert!(display_name_ok("Balm"));
        assert!(!display_name_ok(""), "пустой — мусор конфига");
        let long = "x".repeat(MAX_MM_DISPLAY_NAME + 1);
        assert!(!display_name_ok(&long), "длиннее 64 — skip");
        let exact = "x".repeat(MAX_MM_DISPLAY_NAME);
        assert!(display_name_ok(&exact), "ровно лимит — можно");
    }

    /// P3: файл, уже лежащий на диске, сверяется с sha1 из конфига (если он
    /// там есть): совпал — skip, не совпал/нечитаем — перекачка; без хэша в
    /// конфиге — прежнее поведение (exists() достаточно).
    #[test]
    fn existing_file_checked_against_config_sha1() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.jar");
        std::fs::write(&file, b"payload").unwrap();
        let good = crate::util::fs::sha1_bytes(b"payload");
        assert!(
            existing_file_matches(&file, None),
            "нет хэша в конфиге — exists() достаточно, как раньше"
        );
        assert!(existing_file_matches(&file, Some(&good)), "совпал — skip");
        assert!(
            existing_file_matches(&file, Some(&good.to_uppercase())),
            "регистр хэша не важен"
        );
        assert!(
            !existing_file_matches(&file, Some("deadbeef")),
            "не совпал — задача на перекачку"
        );
        assert!(
            !existing_file_matches(dir.path(), Some(&good)),
            "каталог вместо файла (нечитаем) — перекачка"
        );
    }

    /// D64: CDN-ссылка — запасной источник projectId/versionId КОНКРЕТНОГО
    /// файла; не-cdn и усечённые ссылки дают None (пакетную версию
    /// index.version_id подставлять в манифест нельзя — см. тест ниже).
    #[test]
    fn cdn_url_yields_project_and_version() {
        let (p, v) = ids_from_cdn_url(
            "https://cdn.modrinth.com/data/AABBCC/versions/xy12zz/sodium.jar",
        );
        assert_eq!(p.as_deref(), Some("AABBCC"));
        assert_eq!(v.as_deref(), Some("xy12zz"));
        assert_eq!(ids_from_cdn_url("http://127.0.0.1:9/x.jar"), (None, None));
        assert_eq!(
            ids_from_cdn_url("https://cdn.modrinth.com/data/"),
            (None, None)
        );
        assert_eq!(
            ids_from_cdn_url("https://cdn.modrinth.com/data/AABBCC"),
            (Some("AABBCC".into()), None),
            "без versions/ проекта нет версии"
        );
    }

    /// D64-регресс: в content-manifest пишется projectId/versionId
    /// КОНКРЕТНОГО файла (Modrinth App кладёт их в files[]), а не версия ПАКА
    /// (index.version_id) — check_updates сверяет запись с версией мода, и
    /// паковая версия делала весь свежий пак «устаревшим» с полной
    /// перекачкой модов.
    #[tokio::test]
    async fn manifest_records_per_file_version_not_pack() {
        let app = axum::Router::new().route(
            "/mods/x.jar",
            axum::routing::get(|| async { axum::body::Body::from(MOD_BYTES.to_vec()) }),
        );
        let base = spawn_axum(app).await;
        let sha1 = crate::util::fs::sha1_bytes(MOD_BYTES);
        let index = format!(
            r#"{{"formatVersion":1,"game":"minecraft","versionId":"pack-1.0",
                 "name":"Тест-пак",
                 "files":[{{"path":"mods/x.jar","hashes":{{"sha1":"{sha1}"}},
                            "downloads":["{base}/mods/x.jar"],"fileSize":{size},
                            "projectId":"proj-x","versionId":"file-ver-9"}}],
                 "dependencies":{{"minecraft":"1.21.1"}}}}"#,
            size = MOD_BYTES.len()
        );
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let pack = dir.path().join("pack.mrpack");
        write_mrpack(&pack, &index);

        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let inst = install_mrpack(
            &paths,
            &Settings::default(),
            engine,
            client,
            "mrpack:ids",
            &pack,
            None,
        )
        .await
        .unwrap();

        let manifest = crate::instances::load_content_manifest(&paths, &inst.id);
        assert_eq!(manifest.len(), 1, "{manifest:?}");
        assert_eq!(
            manifest[0].project_id.as_deref(),
            Some("proj-x"),
            "projectId файла из files[]"
        );
        assert_eq!(
            manifest[0].version_id.as_deref(),
            Some("file-ver-9"),
            "versionId ФАЙЛА, не версия пака pack-1.0"
        );
    }

    /// MissingModsChecker, весь сценарий на ОДНОМ mock-сервере (TEST_API_BASE —
    /// глобальный на процесс, параллельные серверы мешали бы друг другу —
    /// как в updates.rs). Найденный на Modrinth мод предустанавливается в
    /// общий прогон по хэшам из API; curseforge-URL из конфига не используется;
    /// не найденный, чужой destination и небезопасное имя пропущены без падения;
    /// повторный вызов по уже установленному файлу сети не трогает.
    #[tokio::test]
    async fn missing_mods_checker_preinstalls_found_and_skips_rest() {
        let search_calls = Arc::new(AtomicUsize::new(0));
        let version_calls = Arc::new(AtomicUsize::new(0));
        let mm_sha1 = crate::util::fs::sha1_bytes(MM_BYTES);
        // Свой bind: URL файла в ответе versions должен указывать на этот же
        // mock, а адрес известен только после привязки слушателя.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (search_srv, version_srv) = (search_calls.clone(), version_calls.clone());
        let (file_base, file_sha1) = (base.clone(), mm_sha1.clone());
        let app = axum::Router::new()
            .route(
                "/v2/search",
                axum::routing::get(
                    move |q: axum::extract::Query<HashMap<String, String>>| {
                        let calls = search_srv.clone();
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            if q.get("query").map(String::as_str) == Some("Balm") {
                                axum::Json(serde_json::json!({
                                    "hits": [{
                                        "project_id": "proj-balm", "project_type": "mod",
                                        "slug": "balm", "author": "a", "title": "Balm",
                                        "description": "", "downloads": 1,
                                    }],
                                    "total_hits": 1,
                                }))
                            } else {
                                // «Ghost Mod» — на Modrinth его нет.
                                axum::Json(serde_json::json!({"hits": [], "total_hits": 0}))
                            }
                        }
                    },
                ),
            )
            .route(
                "/v2/project/{id}/version",
                axum::routing::get(move |path: axum::extract::Path<String>| {
                    let (calls, base, sha1) =
                        (version_srv.clone(), file_base.clone(), file_sha1.clone());
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        if path.as_str() != "proj-balm" {
                            return axum::Json(serde_json::json!([]));
                        }
                        axum::Json(serde_json::json!([{
                            "id": "ver-1", "project_id": "proj-balm",
                            "version_number": "7.3.38",
                            "files": [{
                                "hashes": {"sha1": sha1, "sha512": "mock-sha512"},
                                "url": format!("{base}/files/{MM_PATTERN}"),
                                "filename": MM_PATTERN,
                                "primary": true,
                                "size": MM_BYTES.len(),
                            }],
                        }]))
                    }
                }),
            )
            .route(
                "/files/{name}",
                axum::routing::get(|| async { axum::body::Body::from(MM_BYTES.to_vec()) }),
            );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        // Единственная точка подмены базового URL (форма prod-константы).
        TEST_API_BASE.set(format!("{base}/v2")).ok();

        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        // Конфиг как у Better MC: найденный, не найденный, чужой destination,
        // попытка обхода каталога; curseforge-URL во всех пунктах.
        let config = format!(
            r#"[
                {{"displayName":"Balm","pattern":"{MM_PATTERN}",
                  "url":"https://www.curseforge.com/minecraft/mc-mods/balm","destination":"mods"}},
                {{"displayName":"Ghost Mod","pattern":"ghost-1.0.jar",
                  "url":"https://www.curseforge.com/x","destination":"mods"}},
                {{"displayName":"Weird","pattern":"weird.jar",
                  "url":"https://www.curseforge.com/w","destination":"config"}},
                {{"displayName":"Evil","pattern":"../evil.jar",
                  "url":"https://www.curseforge.com/e","destination":"mods"}}
            ]"#
        );
        let pack = dir.path().join("better-test.mrpack");
        write_mrpack_entries(
            &pack,
            &index_without_files("Better Тест"),
            &[(
                "overrides/config/missing_mods_checker.json",
                config.as_bytes(),
            )],
        );

        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let inst = install_mrpack(
            &paths,
            &Settings::default(),
            engine,
            client.clone(),
            "mrpack:mm-test",
            &pack,
            None,
        )
        .await
        .unwrap();

        // Найденный мод предустановлен тем же прогоном и по хэшу сошёлся.
        let game =
            crate::instances::minecraft_dir(&crate::instances::instance_dir(&paths, &inst.id));
        assert_eq!(
            std::fs::read(game.join("mods").join(MM_PATTERN)).unwrap(),
            MM_BYTES,
            "мод из missing_mods_checker предустановлен"
        );
        // Сеть: Balm — 1 search + 1 versions, Ghost — 1 search (пусто);
        // Weird/Evil отсечены до сети.
        assert_eq!(search_calls.load(Ordering::SeqCst), 2);
        assert_eq!(version_calls.load(Ordering::SeqCst), 1);

        // Повтор по уже установленному файлу (пункт Balm): skip до сети
        // (идемпотентность). Ghost ищется заново — его файл не появился.
        let mm = read_missing_mods_checker(&pack);
        assert_eq!(mm.len(), 4, "конфиг из .mrpack разобран целиком");
        let again =
            satisfy_missing_mods_checker(&client, std::slice::from_ref(&mm[0]), &game, "g2").await;
        assert!(again.is_empty(), "{again:?}");
        assert_eq!(
            search_calls.load(Ordering::SeqCst),
            2,
            "файл уже на диске — сети нет"
        );
        assert_eq!(version_calls.load(Ordering::SeqCst), 1);

        // Пункт-одиночка на пустом каталоге: задача собрана из данных Modrinth,
        // curse-URL не участвует.
        let fresh = tempfile::tempdir().unwrap();
        let tasks = satisfy_missing_mods_checker(&client, &mm, fresh.path(), "g2").await;
        assert_eq!(tasks.len(), 1, "{tasks:?}");
        let t = &tasks[0];
        assert_eq!(t.id, format!("mrpack-mm:{MM_PATTERN}"));
        assert_eq!(t.sha1.as_deref(), Some(mm_sha1.as_str()));
        assert_eq!(t.sha512.as_deref(), Some("mock-sha512"));
        assert!(t.url.starts_with(&base), "url из ответа API: {}", t.url);
        assert!(
            !t.url.contains("curseforge"),
            "curseforge-URL из конфига не используется: {}",
            t.url
        );
        assert_eq!(t.dest, fresh.path().join("mods").join(MM_PATTERN));
        assert_eq!(t.size, Some(MM_BYTES.len() as u64));
        assert_eq!(t.group, "g2");
        assert_eq!(t.priority, 15);
    }

    /// Пак без missing_mods_checker.json (большинство паков): тихо пусто,
    /// сети нет — пустой список конфига до поисков не доходит.
    #[tokio::test]
    async fn missing_mods_checker_absent_config_is_quiet_noop() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("plain.mrpack");
        write_mrpack(&pack, &index_without_files("Без конфига"));
        assert!(
            read_missing_mods_checker(&pack).is_empty(),
            "нет конфига в архиве — пусто"
        );
        let client = HttpClient::new(None).unwrap();
        let nowhere = dir.path().join("mc");
        assert!(
            satisfy_missing_mods_checker(&client, &[], &nowhere, "g")
                .await
                .is_empty(),
            "пустой список — пустой результат"
        );
    }

    /// F16: офлайн — предустановка пропускается целиком, до сети не доходит
    /// (игра докачает моды сама, как раньше).
    #[tokio::test]
    async fn missing_mods_checker_offline_is_noop() {
        let client = HttpClient::new(None).unwrap();
        client.set_offline(true);
        let entries: Vec<MissingModsEntry> = serde_json::from_str(
            r#"[{"displayName":"Balm","pattern":"balm.jar","destination":"mods"}]"#,
        )
        .unwrap();
        let mc = tempfile::tempdir().unwrap();
        assert!(
            satisfy_missing_mods_checker(&client, &entries, mc.path(), "g")
                .await
                .is_empty()
        );
    }
}
