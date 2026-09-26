//! Modrinth API v2 (спека §6.6). UA обязателен (уже в HttpClient).

use crate::errors::Result;
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
