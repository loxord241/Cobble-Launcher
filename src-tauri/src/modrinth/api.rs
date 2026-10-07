//! Modrinth API v2 (спека §6.6). UA обязателен (уже в HttpClient).

use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use serde::{Deserialize, Serialize};

pub const API_BASE: &str = "https://api.modrinth.com/v2";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub project_id: String,
    pub project_type: String,
    pub slug: String,
    pub author: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub versions: Vec<String>,
    pub downloads: u64,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub date_modified: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub total_hits: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub version_number: String,
    #[serde(default)]
    pub version_type: Option<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub files: Vec<VersionFile>,
    #[serde(default)]
    pub changelog: Option<String>,
    #[serde(default)]
    pub date_published: Option<String>,
}

/// Файл версии для контракта фронту (camelCase, D15).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFileInfo {
    pub url: String,
    pub filename: String,
    pub primary: bool,
    pub size: u64,
}

/// Версия проекта для контракта фронту (camelCase, D15): таб «Версии» и
/// «Журнал изменений» в окне проекта. Строится из внутреннего `Version`
/// (он остаётся на wire-именах Modrinth — на нём висят install/updates).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: String,
    pub version_number: String,
    pub version_type: Option<String>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub changelog: Option<String>,
    pub date_published: Option<String>,
    pub files: Vec<VersionFileInfo>,
}

impl From<Version> for VersionInfo {
    fn from(v: Version) -> Self {
        Self {
            id: v.id,
            version_number: v.version_number,
            version_type: v.version_type,
            game_versions: v.game_versions,
            loaders: v.loaders,
            changelog: v.changelog,
            date_published: v.date_published,
            files: v
                .files
                .into_iter()
                .map(|f| VersionFileInfo {
                    url: f.url,
                    filename: f.filename,
                    primary: f.primary,
                    size: f.size,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    pub dependency_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionFile {
    pub hashes: std::collections::HashMap<String, String>,
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    pub size: u64,
}

/// Метаданные проекта для списка установленного контента (иконка/название).
///
/// D15: REST API v2 отвечает snake_case, а фронт (`src/api/types.ts`) ждёт
/// camelCase — поэтому сериализация camelCase, а входные имена принимаем
/// алиасами. Батч-эндпоинт `/projects` отдаёт id проекта в поле `id`
/// (проверено на api.modrinth.com), поэтому в алиасах и `project_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMeta {
    #[serde(alias = "id", alias = "project_id")]
    pub project_id: String,
    pub title: String,
    pub description: String,
    #[serde(default, alias = "icon_url")]
    pub icon_url: Option<String>,
}

/// Лимит Modrinth на один запрос `/projects?ids=[…]`.
pub const PROJECTS_BATCH: usize = 100;

/// Потолок id за один вызов команды (граница IPC).
pub const PROJECTS_MAX_IDS: usize = 500;

/// Валидация id из IPC-границы: рендеру не доверяем (A1/A23-принцип).
/// Пустой id, пробелы и `..` в query недопустимы.
pub fn validate_project_ids(ids: &[String]) -> Result<()> {
    if ids.is_empty() || ids.len() > PROJECTS_MAX_IDS {
        return Err(LauncherError::InvalidInput(format!(
            "ids: ожидается 1..={PROJECTS_MAX_IDS}, получено {}",
            ids.len()
        )));
    }
    for id in ids {
        if id.is_empty() || id.chars().any(char::is_whitespace) || id.contains("..") {
            return Err(LauncherError::InvalidInput(format!(
                "некорректный project id: {id:?}"
            )));
        }
    }
    Ok(())
}

/// Куски id не длиннее `PROJECTS_BATCH` (чистая функция — тестируется без сети).
fn chunk_ids(ids: &[String]) -> Vec<&[String]> {
    ids.chunks(PROJECTS_BATCH).collect()
}

/// Метаданные проектов батчем: `/projects?ids=["id1","id2"]` (ids — JSON-массивом
/// в query, как facets у поиска). Больше `PROJECTS_BATCH` id за вызов сервер не
/// принимает — режем кусками. Порядок ответа не контрактный: вызывающий
/// сопоставляет по `projectId`.
pub async fn projects_meta(client: &HttpClient, ids: &[String]) -> Result<Vec<ProjectMeta>> {
    // F16: офлайн-режим — мгновенный честный отказ вместо сетевого таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode("метаданные Modrinth".into()));
    }
    let url = format!("{API_BASE}/projects");
    let mut out: Vec<ProjectMeta> = Vec::with_capacity(ids.len());
    for chunk in chunk_ids(ids) {
        let ids_json = serde_json::to_string(chunk)?;
        let list: Vec<ProjectMeta> = client
            .raw()
            .get(&url)
            .query(&[("ids", ids_json)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        out.extend(list);
    }
    Ok(out)
}

/// Допустимые значения сортировки Modrinth (`index`); всё остальное из
/// IPC-границы — InvalidInput, а не тихий запрос с мусорным параметром.
pub const SEARCH_INDEXES: [&str; 5] = ["relevance", "downloads", "follows", "updated", "newest"];

/// Валидация и дефолт сортировки (чистая функция — тестируется без сети):
/// None → пустой запрос «популярное» (downloads), с запросом — relevance.
fn resolve_index(index: Option<&str>, query: &str) -> Result<String> {
    if let Some(ix) = index {
        if !SEARCH_INDEXES.contains(&ix) {
            return Err(LauncherError::InvalidInput(format!(
                "index: допустимо {SEARCH_INDEXES:?}, получено {ix:?}"
            )));
        }
    }
    Ok(index
        .unwrap_or(if query.is_empty() { "downloads" } else { "relevance" })
        .to_string())
}

/// Потолок длины одной категории поиска (IPC-граница; слаги Modrinth короче).
pub const CATEGORY_MAX_LEN: usize = 64;

/// Валидация категорий поиска из IPC-границы: непустые, без пробелов,
/// ограниченной длины (чистая функция — тестируется без сети).
pub fn validate_search_categories(categories: &[String]) -> Result<()> {
    for c in categories {
        if c.is_empty() || c.chars().any(char::is_whitespace) || c.len() > CATEGORY_MAX_LEN {
            return Err(LauncherError::InvalidInput(format!(
                "некорректная категория поиска: {c:?}"
            )));
        }
    }
    Ok(())
}

/// Facets поиска (чистая функция — тестируется без сети): каждый фильтр —
/// отдельный блок; между блоками AND, внутри блока OR (семантика Modrinth).
/// Каждая категория кладётся в СВОЙ блок — проект обязан иметь все выбранные
/// категории (фильтр «по нескольким категориям» в каталоге).
pub(crate) fn build_search_facets(
    mc_version: Option<&str>,
    loader: Option<&str>,
    project_type: Option<&str>,
    categories: Option<&[String]>,
) -> Vec<Vec<String>> {
    let mut facets: Vec<Vec<String>> = Vec::new();
    if let Some(mc) = mc_version {
        facets.push(vec![format!("versions:{mc}")]);
    }
    if let Some(l) = loader {
        facets.push(vec![format!("categories:{l}")]);
    }
    if let Some(pt) = project_type {
        facets.push(vec![format!("project_type:{pt}")]);
    }
    if let Some(cats) = categories {
        for c in cats {
            facets.push(vec![format!("categories:{c}")]);
        }
    }
    facets
}

/// Поиск: facets строим вручную ([[«versions:x»], [«categories:y»], …], §6.6).
/// `index` — сортировка (None: пустой запрос → downloads «популярное»,
/// с запросом → relevance как у Modrinth по умолчанию). `categories` —
/// фильтр по категориям каталога (каждая в своём блоке — AND-семантика).
#[allow(clippy::too_many_arguments)]
pub async fn search(
    client: &HttpClient,
    query: &str,
    mc_version: Option<&str>,
    loader: Option<&str>,
    project_type: Option<&str>,
    categories: Option<&[String]>,
    index: Option<&str>,
    limit: u32,
    offset: u32,
) -> Result<SearchResult> {
    // F16: офлайн-режим — мгновенный честный отказ вместо сетевого таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode("поиск Modrinth".into()));
    }
    let index = resolve_index(index, query)?;
    let facets = build_search_facets(mc_version, loader, project_type, categories);
    let facets_json = serde_json::to_string(&facets)?;
    let url = format!("{API_BASE}/search");
    let params = [
        ("query", query.to_string()),
        ("facets", facets_json),
        ("index", index.to_string()),
        ("limit", limit.to_string()),
        ("offset", offset.to_string()),
    ];
    let result: SearchResult = client
        .raw()
        .get(&url)
        .query(&params)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(result)
}

/// Версии проекта (совместимость выбирает вызывающий).
pub async fn project_versions(client: &HttpClient, project_id: &str) -> Result<Vec<Version>> {
    let url = format!("{API_BASE}/project/{project_id}/version");
    let list: Vec<Version> = client.get_json_retry(&url).await?;
    Ok(list)
}

/// Проект по id/slug.
pub async fn project(
    client: &HttpClient,
    id_or_slug: &str,
) -> Result<serde_json::Value> {
    let url = format!("{API_BASE}/project/{id_or_slug}");
    client.get_json_retry(&url).await
}

/// Картинка галереи проекта (контракт фронту, camelCase).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryImage {
    /// Полный размер (wire-имя `raw_url`).
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub featured: bool,
}

/// Страница проекта целиком (контракт фронту, camelCase): окно проекта
/// «как в CurseForge» — обзор (markdown-тело), галерея, версии, статистика.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetail {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    /// Тело страницы в markdown; фронт рендерит безопасным конвертером.
    pub body: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub follows: u64,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub date_published: Option<String>,
    #[serde(default)]
    pub date_modified: Option<String>,
    /// Человекочитаемое имя лицензии (например, «MIT»).
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub gallery: Vec<GalleryImage>,
}

/// Wire-форма `/project/{id}` (snake_case Modrinth) — только разбор,
/// в контракт не уходит (D15: серва в camelCase делает ProjectDetail).
#[derive(Debug, Deserialize)]
struct ProjectRaw {
    #[serde(alias = "project_id")]
    id: String,
    #[serde(default)]
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    icon_url: Option<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(alias = "follows", default)]
    followers: u64,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(alias = "date_published")]
    published: Option<String>,
    #[serde(alias = "date_modified")]
    updated: Option<String>,
    #[serde(default)]
    license: Option<LicenseRaw>,
    /// Элементы галереи разбираем вручную: в ответе Modrinth есть И `raw_url`,
    /// И `url` — serde-алиас на обоих падает «duplicate field».
    #[serde(default)]
    gallery: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct LicenseRaw {
    #[serde(default)]
    name: Option<String>,
}

/// Картинка галереи из wire-элемента: приоритет у raw_url (полный размер),
/// url — запасной (превью). Отсутствуют оба — элемент пропускается.
fn gallery_image(v: &serde_json::Value) -> Option<GalleryImage> {
    let url = v
        .get("raw_url")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("url").and_then(|x| x.as_str()))?
        .to_string();
    Some(GalleryImage {
        title: v.get("title").and_then(|x| x.as_str()).map(String::from),
        caption: v
            .get("description")
            .and_then(|x| x.as_str())
            .map(String::from),
        featured: v
            .get("featured")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        url,
    })
}

/// Валидация id/slug из IPC-границы: Modrinth допускает [A-Za-z0-9-_],
/// всё остальное (пробелы, `..`, пустота) — InvalidInput до запроса.
/// pub(crate): тот же guard на путях install/updates/mrpack, где id/slug
/// уходит в URL `{API_BASE}/project/{id}` — чужая строка не должна
/// подменять путь запроса (инъекция `../`).
pub(crate) fn validate_id_or_slug(id_or_slug: &str) -> Result<()> {
    let ok = !id_or_slug.is_empty()
        && id_or_slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(LauncherError::InvalidInput(format!(
            "некорректный id проекта: {id_or_slug:?}"
        )))
    }
}

/// Потолок имени файла из API в пути на диске: легальные Modrinth-имена —
/// десятки символов, длиннющее — мусор (резерв под лимит пути Windows).
const FILENAME_MAX_CHARS: usize = 180;

/// D64: `filename` из ответа API попадает в пути на диске (join в install/
/// updates/mrpack) — доверять строке нельзя (глюк API/подмена). Один
/// санитайзер на всё ядро: сепараторы и control-символы вырезаются
/// (разделителей нет — компонентов пути и `..` не остаётся), точки/пробелы
/// по краям срезаются (Windows их всё равно съедает), длина ограничена,
/// пусто → «download». Не ошибка, а приведение к безопасному имени: файл
/// качается дальше по хэшу из доверенного API.
pub fn sanitize_file_name(raw: &str) -> String {
    let cut: String = raw
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
        .take(FILENAME_MAX_CHARS)
        .collect();
    let name = cut.trim().trim_matches(['.', ' ']);
    if name.is_empty() {
        "download".into()
    } else {
        name.to_string()
    }
}

/// Страница проекта: `/project/{id|slug}` → wire-JSON → ProjectDetail.
pub async fn project_detail(client: &HttpClient, id_or_slug: &str) -> Result<ProjectDetail> {
    // F16: офлайн-режим — мгновенный честный отказ вместо сетевого таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode("страница проекта Modrinth".into()));
    }
    validate_id_or_slug(id_or_slug)?;
    project_detail_from_value(project(client, id_or_slug).await?)
}

/// Чистый маппинг wire-JSON → контракт (тестируется без сети).
fn project_detail_from_value(value: serde_json::Value) -> Result<ProjectDetail> {
    let raw: ProjectRaw = serde_json::from_value(value)?;
    Ok(ProjectDetail {
        project_id: raw.id,
        slug: raw.slug,
        title: raw.title,
        description: raw.description,
        body: raw.body,
        icon_url: raw.icon_url,
        downloads: raw.downloads,
        follows: raw.followers,
        categories: raw.categories,
        game_versions: raw.game_versions,
        loaders: raw.loaders,
        date_published: raw.published,
        date_modified: raw.updated,
        license: raw.license.and_then(|l| l.name),
        gallery: raw.gallery.iter().filter_map(gallery_image).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_fixture() {
        let r: SearchResult = serde_json::from_str(include_str!(
            "../../tests/fixtures/modrinth_search.json"
        ))
        .unwrap();
        assert_eq!(r.total_hits, 2);
        let hit = &r.hits[0];
        assert_eq!(hit.slug, "sodium");
        assert!(hit.versions.contains(&"1.21.1".to_string()));
    }

    #[test]
    fn parses_version_fixture() {
        let list: Vec<Version> = serde_json::from_str(include_str!(
            "../../tests/fixtures/modrinth_versions.json"
        ))
        .unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].files[0].primary);
        assert_eq!(list[0].dependencies[0].dependency_type, "required");
        assert!(list[0].game_versions.contains(&"1.21.1".to_string()));
        assert_eq!(list[0].loaders, vec!["fabric"]);
    }

    /// Ответ батч-эндпоинта: `id`/`icon_url` — wire-имена Modrinth (D15),
    /// отсутствующий `icon_url` — это `None`, а не ошибка разбора.
    #[test]
    fn parses_projects_meta_fixture() {
        let list: Vec<ProjectMeta> = serde_json::from_str(include_str!(
            "../../tests/fixtures/modrinth_projects.json"
        ))
        .unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].project_id, "AANobbMI");
        assert_eq!(list[0].title, "Sodium");
        assert_eq!(
            list[0].icon_url.as_deref(),
            Some("https://cdn.modrinth.com/data/AANobbMI/295862f4724dc3f78df3447ad6072b2dcd3ef0c9_96.webp")
        );
        assert_eq!(list[1].project_id, "P7dR8mSH");
        assert!(list[1].icon_url.is_none(), "icon_url без поля → None");
    }

    /// Фронт (`src/api/types.ts`) читает `projectId`/`iconUrl` — контракт
    /// camelCase (D25), иначе иконки и названия в табе «Моды» пропадут.
    #[test]
    fn project_meta_serializes_camel_case() {
        let m = ProjectMeta {
            project_id: "AANobbMI".into(),
            title: "Sodium".into(),
            description: "движок рендера".into(),
            icon_url: None,
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"projectId\":\"AANobbMI\""), "{json}");
        assert!(json.contains("\"iconUrl\":null"), "{json}");
        assert!(
            !json.contains("project_id") && !json.contains("icon_url"),
            "snake_case ушёл из контракта: {json}"
        );
    }

    /// Батч-лимит: больше `PROJECTS_BATCH` id одним запросом Modrinth не примет.
    #[test]
    fn chunks_ids_by_hundred() {
        let ids: Vec<String> = (0..250).map(|i| format!("id{i}")).collect();
        let sizes: Vec<usize> = chunk_ids(&ids).iter().map(|c| c.len()).collect();
        assert_eq!(sizes, vec![100, 100, 50], "не более PROJECTS_BATCH id на запрос");
        assert!(chunk_ids(&[]).is_empty(), "пустой список — ноль запросов");
    }

    /// Валидация IPC-границы: пустой список, перебор, пробелы и `..` — отказ.
    #[test]
    fn rejects_bad_project_ids() {
        assert!(validate_project_ids(&[]).is_err(), "пустой список");
        let many: Vec<String> = (0..=PROJECTS_MAX_IDS).map(|i| format!("id{i}")).collect();
        assert_eq!(many.len(), PROJECTS_MAX_IDS + 1);
        assert!(validate_project_ids(&many).is_err(), "больше потолка");
        let ok: Vec<String> = (0..PROJECTS_MAX_IDS).map(|i| format!("id{i}")).collect();
        assert!(validate_project_ids(&ok).is_ok(), "ровно PROJECTS_MAX_IDS — можно");
        assert!(validate_project_ids(&[String::new()]).is_err(), "пустой id");
        assert!(validate_project_ids(&["a b".to_string()]).is_err(), "пробел");
        assert!(validate_project_ids(&["\t".to_string()]).is_err(), "whitespace");
        assert!(validate_project_ids(&["../../etc".to_string()]).is_err(), "..");
        assert!(
            validate_project_ids(&["AANobbMI".to_string(), "sodium-extra".to_string()]).is_ok()
        );
    }

    /// Реальная сеть — #[ignore], гонять на приёмке (как real_search).
    #[tokio::test]
    #[ignore]
    async fn real_projects_meta() {
        let client = HttpClient::new(None).unwrap();
        let ids = vec!["AANobbMI".to_string(), "P7dR8mSH".to_string()];
        let list = projects_meta(&client, &ids).await.unwrap();
        assert_eq!(list.len(), 2, "{list:?}");
        assert!(list.iter().any(|p| p.title == "Sodium"), "{list:?}");
        assert!(list.iter().all(|p| !p.project_id.is_empty() && !p.title.is_empty()));
    }

    /// Реальная сеть — #[ignore], гонять на приёмке.
    #[tokio::test]
    #[ignore]
    async fn real_search() {
        let client = HttpClient::new(None).unwrap();
        let r = search(
            &client,
            "sodium",
            Some("1.21.1"),
            Some("fabric"),
            Some("mod"),
            None,
            None,
            5,
            0,
        )
        .await
        .unwrap();
        assert!(r.total_hits > 0);
        // Фильтр по категориям принимается сервером (каждая — свой блок facets),
        // явная сортировка тоже.
        let r = search(
            &client,
            "",
            Some("1.21.1"),
            Some("fabric"),
            Some("mod"),
            Some(&["optimization".to_string()]),
            Some("follows"),
            5,
            0,
        )
        .await
        .unwrap();
        assert!(r.total_hits > 0);
    }

    /// Сортировка вне белого списка — InvalidInput; дефолты: пустой запрос →
    /// downloads (популярное), с запросом → relevance.
    #[test]
    fn resolves_search_index() {
        assert_eq!(resolve_index(None, "").unwrap(), "downloads");
        assert_eq!(resolve_index(None, "sodium").unwrap(), "relevance");
        assert_eq!(resolve_index(Some("follows"), "").unwrap(), "follows");
        let err = resolve_index(Some("hype"), "").unwrap_err();
        assert_eq!(err.code(), "invalid_input");
    }

    /// Facets: каждый фильтр — свой блок; категории — каждая в своём блоке
    /// (AND-семантика: проект обязан иметь все выбранные категории).
    #[test]
    fn builds_search_facets_with_categories() {
        let cats = vec!["adventure".to_string(), "optimization".to_string()];
        let facets = build_search_facets(Some("1.21.1"), Some("fabric"), Some("mod"), Some(&cats));
        assert_eq!(
            facets,
            vec![
                vec!["versions:1.21.1".to_string()],
                vec!["categories:fabric".to_string()],
                vec!["project_type:mod".to_string()],
                vec!["categories:adventure".to_string()],
                vec!["categories:optimization".to_string()],
            ]
        );
        // Без фильтров — пустой facets (серверу уходит []).
        assert!(build_search_facets(None, None, None, None).is_empty());
        // Сериализация в wire-форму Modrinth: массив массивов.
        let json = serde_json::to_string(&facets).unwrap();
        assert_eq!(
            json,
            r#"[["versions:1.21.1"],["categories:fabric"],["project_type:mod"],["categories:adventure"],["categories:optimization"]]"#
        );
    }

    /// Валидация категорий из IPC-границы: пустые/с пробелами/слишком длинные —
    /// отказ; нормальные слаги проходят.
    #[test]
    fn validates_search_categories() {
        assert!(validate_search_categories(&["adventure".into(), "technology".into()]).is_ok());
        assert!(
            validate_search_categories(&[String::new()]).is_err(),
            "пустая категория"
        );
        assert!(
            validate_search_categories(&["a b".to_string()]).is_err(),
            "пробел внутри"
        );
        assert!(
            validate_search_categories(&["\t".to_string()]).is_err(),
            "whitespace"
        );
        let too_long = "x".repeat(CATEGORY_MAX_LEN + 1);
        assert!(
            validate_search_categories(&[too_long]).is_err(),
            "длиннее CATEGORY_MAX_LEN"
        );
        let exact = "x".repeat(CATEGORY_MAX_LEN);
        assert!(validate_search_categories(&[exact]).is_ok(), "ровно лимит — можно");
    }

    /// Разбор wire-формы /project/{id} (snake_case) в camelCase-контракт.
    #[test]
    fn parses_project_detail_fixture() {
        let raw = serde_json::json!({
            "id": "AANobbMI",
            "slug": "sodium",
            "title": "Sodium",
            "description": "короткое описание",
            "body": "# Sodium\nТекст **жирный**.",
            "icon_url": "https://cdn.modrinth.com/icon.webp",
            "downloads": 4_500_000u64,
            "followers": 12_345u64,
            "categories": ["optimization", "fabric"],
            "game_versions": ["1.21.1", "1.21"],
            "loaders": ["fabric"],
            "published": "2020-01-01T00:00:00Z",
            "updated": "2026-09-01T00:00:00Z",
            "license": { "id": "MIT", "name": "MIT" },
            "gallery": [
                { "raw_url": "https://cdn.modrinth.com/1.png", "url": "https://cdn.modrinth.com/1-small.png", "title": "Скрин", "description": "подпись", "featured": true },
                { "url": "https://cdn.modrinth.com/2.png" }
            ]
        });
        let d = project_detail_from_value(raw).unwrap();
        assert_eq!(d.project_id, "AANobbMI");
        assert_eq!(d.slug, "sodium");
        assert_eq!(d.body, "# Sodium\nТекст **жирный**.");
        assert_eq!(d.downloads, 4_500_000);
        assert_eq!(d.follows, 12_345, "followers → follows");
        assert_eq!(d.date_published.as_deref(), Some("2020-01-01T00:00:00Z"));
        assert_eq!(d.license.as_deref(), Some("MIT"));
        assert_eq!(d.gallery.len(), 2);
        assert_eq!(d.gallery[0].url, "https://cdn.modrinth.com/1.png", "raw_url → url");
        assert_eq!(d.gallery[0].caption.as_deref(), Some("подпись"));
        assert!(d.gallery[0].featured);
        assert!(!d.gallery[1].featured);
        assert!(d.gallery[1].title.is_none());
    }

    /// Контракт camelCase: ProjectDetail/VersionInfo сериализуются для фронта.
    #[test]
    fn detail_and_version_serialize_camel_case() {
        let d = ProjectDetail {
            project_id: "AANobbMI".into(),
            slug: "sodium".into(),
            title: "Sodium".into(),
            description: String::new(),
            body: String::new(),
            icon_url: None,
            downloads: 1,
            follows: 2,
            categories: vec![],
            game_versions: vec![],
            loaders: vec!["fabric".into()],
            date_published: None,
            date_modified: Some("2026-09-01".into()),
            license: Some("MIT".into()),
            gallery: vec![GalleryImage {
                url: "https://cdn.modrinth.com/1.png".into(),
                title: None,
                caption: None,
                featured: true,
            }],
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"projectId\":\"AANobbMI\""), "{json}");
        assert!(json.contains("\"iconUrl\""), "{json}");
        assert!(json.contains("\"dateModified\""), "{json}");
        assert!(!json.contains("date_modified"), "{json}");
        assert!(!json.contains("\"rawUrl\""), "{json}");

        let v = VersionInfo::from(Version {
            id: "abc".into(),
            project_id: "AANobbMI".into(),
            version_number: "0.6.0".into(),
            version_type: Some("release".into()),
            game_versions: vec!["1.21.1".into()],
            loaders: vec!["fabric".into()],
            dependencies: vec![],
            files: vec![VersionFile {
                hashes: Default::default(),
                url: "https://example/sodium.jar".into(),
                filename: "sodium.jar".into(),
                primary: true,
                size: 1234,
            }],
            changelog: Some("фиксы".into()),
            date_published: Some("2026-08-01T00:00:00Z".into()),
        });
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("\"versionNumber\":\"0.6.0\""), "{json}");
        assert!(json.contains("\"gameVersions\""), "{json}");
        assert!(json.contains("\"datePublished\""), "{json}");
        assert!(json.contains("\"filename\":\"sodium.jar\""), "{json}");
    }

    /// id/slug для IPC: мусор отклоняется, валидные проходят.
    #[test]
    fn validates_id_or_slug() {
        assert!(validate_id_or_slug("AANobbMI").is_ok());
        assert!(validate_id_or_slug("sodium-extra").is_ok());
        assert!(validate_id_or_slug("").is_err());
        assert!(validate_id_or_slug("../etc").is_err());
        assert!(validate_id_or_slug("a b").is_err());
        assert!(validate_id_or_slug("соль").is_err(), "не-ASCII — отказ");
    }

    /// D64: имя файла из API — один безопасный компонент: сепараторы и `..`
    /// разваливаются, точки/пробелы по краям срезаются (нрав Windows),
    /// control-символы вырезаются, длиннющее обрезается, пустое → дефолт.
    #[test]
    fn sanitizes_api_filename() {
        assert_eq!(sanitize_file_name("../evil"), "evil");
        assert_eq!(sanitize_file_name("a/b\\c.png"), "abc.png");
        assert_eq!(
            sanitize_file_name("имя с пробелами .png"),
            "имя с пробелами .png",
            "пробелы внутри не трогаем"
        );
        assert_eq!(sanitize_file_name(""), "download");
        assert_eq!(sanitize_file_name("   "), "download");
        assert_eq!(sanitize_file_name(".."), "download");
        assert_eq!(sanitize_file_name("mod.jar..."), "mod.jar");
        assert_eq!(
            sanitize_file_name("mod\u{0}x.jar"),
            "modx.jar",
            "control-символы вырезаются"
        );
        let long = format!("{}.jar", "x".repeat(FILENAME_MAX_CHARS + 50));
        let cut = sanitize_file_name(&long);
        assert_eq!(cut.chars().count(), FILENAME_MAX_CHARS, "обрезка до потолка");
        assert!(cut.starts_with("xxx"), "обрезка сохраняет начало имени");
    }
}

#[cfg(test)]
mod real_net_tests {
    use super::*;

    /// Реальная сеть — #[ignore], гонять на приёмке (D39).
    #[tokio::test]
    #[ignore]
    async fn real_project_detail_live() {
        let client = HttpClient::new(None).unwrap();
        let d = project_detail(&client, "AANobbMI").await.unwrap();
        assert_eq!(d.title, "Sodium");
        assert!(!d.body.is_empty());
        let v = project_versions(&client, "AANobbMI").await.unwrap();
        assert!(!v.is_empty());
    }
}
