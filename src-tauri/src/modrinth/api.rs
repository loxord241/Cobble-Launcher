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

/// Поиск: facets строим вручную ([[«versions:x»], [«categories:y»], …], §6.6).
pub async fn search(
    client: &HttpClient,
    query: &str,
    mc_version: Option<&str>,
    loader: Option<&str>,
    project_type: Option<&str>,
    limit: u32,
    offset: u32,
) -> Result<SearchResult> {
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
    let facets_json = serde_json::to_string(&facets)?;
    let url = format!("{API_BASE}/search");
    let params = [
        ("query", query.to_string()),
        ("facets", facets_json),
        ("index", "downloads".to_string()),
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
        let r = search(&client, "sodium", Some("1.21.1"), Some("fabric"), Some("mod"), 5, 0)
            .await
            .unwrap();
        assert!(r.total_hits > 0);
    }
}
