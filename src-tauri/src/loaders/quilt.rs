//! Quilt (спека §6.3): meta.quiltmc.org v3 — аналог Fabric, mainClass QuiltClient,
//! библиотеки включают fabric-loader (Quilt наследует его).

use crate::errors::{LauncherError, Result};
use crate::mojang::version::VersionJson;
use crate::net::http::HttpClient;
use serde::Deserialize;

pub const META_BASE: &str = "https://meta.quiltmc.org/v3";

#[derive(Debug, Clone, Deserialize)]
pub struct LoaderVersion {
    pub version: String,
    #[serde(default)]
    pub stable: bool,
}

/// Список версий загрузчика (`/v3/versions/loader`).
pub async fn loader_versions(client: &HttpClient) -> Result<Vec<LoaderVersion>> {
    client.get_json_retry(&format!("{META_BASE}/versions/loader")).await
}

/// Готовый profile JSON (`/v3/versions/loader/<mc>/<loader>/profile/json`).
pub async fn profile_json(client: &HttpClient, mc: &str, loader: &str) -> Result<VersionJson> {
    let url = format!("{META_BASE}/versions/loader/{mc}/{loader}/profile/json");
    client.get_json_retry::<VersionJson>(&url).await
}

/// Установить Quilt в инстанс.
pub async fn install(
    client: &HttpClient,
    versions_dir: &std::path::Path,
    mc: &str,
    loader: &str,
) -> Result<String> {
    let profile = profile_json(client, mc, loader).await?;
    if profile.inherits_from.is_none() {
        return Err(LauncherError::InvalidInput(format!(
            "профиль Quilt {mc}/{loader} без inheritsFrom — не ждали"
        )));
    }
    let id = profile.id.clone();
    // Аудит 2026-10-06: id из удалённого meta — имя файла до записи.
    if !crate::loaders::is_safe_version_id(&id) {
        return Err(LauncherError::InvalidInput(format!(
            "некорректный id версии из профиля Quilt: {id}"
        )));
    }
    crate::util::fs::atomic_write(
        &versions_dir.join(format!("{id}.json")),
        &serde_json::to_vec_pretty(&profile)?,
    )?;
    Ok(id)
}
