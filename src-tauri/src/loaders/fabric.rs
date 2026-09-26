//! Fabric (спека §6.3): meta.fabricmc.net v2 — версии загрузчика и готовый
//! profile JSON с `inheritsFrom`. Установка = сохранить JSON в инстанс.

use crate::errors::{LauncherError, Result};
use crate::mojang::version::VersionJson;
use crate::net::http::HttpClient;
use serde::Deserialize;

pub const META_BASE: &str = "https://meta.fabricmc.net/v2";

#[derive(Debug, Clone, Deserialize)]
pub struct LoaderVersion {
    pub version: String,
    #[serde(default)]
    pub stable: bool,
}

/// Список версий загрузчика (`/v2/versions/loader`).
pub async fn loader_versions(client: &HttpClient) -> Result<Vec<LoaderVersion>> {
    client.get_json_retry(&format!("{META_BASE}/versions/loader")).await
}

/// Готовый profile JSON (`/v2/versions/loader/<mc>/<loader>/profile/json`):
/// id = `fabric-loader-<l>-<mc>`, inheritsFrom = <mc>, свой mainClass и библиотеки.
pub async fn profile_json(client: &HttpClient, mc: &str, loader: &str) -> Result<VersionJson> {
    let url = format!("{META_BASE}/versions/loader/{mc}/{loader}/profile/json");
    client.get_json_retry::<VersionJson>(&url).await
}

/// Установить Fabric в инстанс: профиль сохраняется в `versions/` инстанса,
/// возвращается id версии. `versions_dir` — каталог версий инстанса.
pub async fn install(
    client: &HttpClient,
    versions_dir: &std::path::Path,
    mc: &str,
    loader: &str,
) -> Result<String> {
    let profile = profile_json(client, mc, loader).await?;
    if profile.inherits_from.is_none() {
        return Err(LauncherError::InvalidInput(format!(
            "профиль Fabric {mc}/{loader} без inheritsFrom — не ждали"
        )));
    }
    let id = profile.id.clone();
    crate::util::fs::atomic_write(
        &versions_dir.join(format!("{id}.json")),
        &serde_json::to_vec_pretty(&profile)?,
    )?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Реальная сеть — #[ignore], гонять на приёмке (спека §9).
    #[tokio::test]
    #[ignore]
    async fn fetches_profile_for_1_20_1() {
        let client = HttpClient::new(None).unwrap();
        let p = profile_json(&client, "1.20.1", "0.15.11").await.unwrap();
        assert_eq!(p.inherits_from.as_deref(), Some("1.20.1"));
        assert!(p.main_class.as_deref().unwrap_or("").contains("KnotClient"));
    }
}
