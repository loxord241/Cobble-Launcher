//! Forge (спека §6.3): официальный maven БЕЗ `--mirror` (антипаттерн 16Launcher),
//! installer с обязательной сверкой `.sha1`, headless `--installClient`.

use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use std::path::{Path, PathBuf};

pub const MAVEN_BASE: &str = "https://maven.minecraftforge.net";
pub const PROMOTIONS_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";

/// Recommended/Latest версии Forge по версиям MC (официальный promotions JSON).
pub async fn promotions(client: &HttpClient) -> Result<HashMap<String, String>> {
    #[derive(Deserialize)]
    struct Promos {
        promos: HashMap<String, String>,
    }
    let p: Promos = client.get_json_retry(PROMOTIONS_URL).await?;
    Ok(p.promos)
}

/// Версия Forge для MC: рекомендуемая, иначе последняя (формат ключей
/// `1.20.1-recommended` / `1.20.1-latest`).
pub async fn latest_for_mc(client: &HttpClient, mc: &str) -> Result<String> {
    let promos = promotions(client).await?;
    promos
        .get(&format!("{mc}-recommended"))
        .or_else(|| promos.get(&format!("{mc}-latest")))
        .cloned()
        .ok_or_else(|| LauncherError::VersionNotFound(format!("Forge для {mc}")))
}

fn installer_path(cache_dir: &Path, mc: &str, version: &str) -> PathBuf {
    // Координата maven включает MC: net/minecraftforge/forge/1.20.1-47.4.10/…
    cache_dir
        .join("installers")
        .join(format!("forge-{mc}-{version}-installer.jar"))
}

/// Скачать installer.jar с обязательной сверкой `.sha1` (спека §3, §6.3).
pub async fn download_installer(
    client: &HttpClient,
    cache_dir: &Path,
    mc: &str,
    version: &str,
) -> Result<PathBuf> {
    let dest = installer_path(cache_dir, mc, version);
    let dest_long = crate::util::fs::long_path(&dest);
    if dest_long.exists() {
        return Ok(dest);
    }
    // F16: офлайн-режим — мгновенный честный отказ вместо сетевого таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode("загрузчик Forge".into()));
    }
    let full = format!("{mc}-{version}");
    let jar_url =
        format!("{MAVEN_BASE}/net/minecraftforge/forge/{full}/forge-{full}-installer.jar");
    let sha_url = format!("{jar_url}.sha1");

    let resp = client.raw().get(&jar_url).send().await?;
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "HTTP {} для {jar_url}",
            resp.status()
        )));
    }
    let bytes = resp.bytes().await?;

    let sha_resp = client.raw().get(&sha_url).send().await?;
    if !sha_resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "нет .sha1 для installer {version} — скачивание запрещено (спека §3)"
        )));
    }
    let expected = String::from_utf8_lossy(&sha_resp.bytes().await?)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    let actual = crate::util::fs::sha1_bytes(&bytes);
    if expected.is_empty() || actual != expected {
        return Err(LauncherError::HashMismatch {
            path: jar_url,
            expected,
            actual,
        });
    }
    std::fs::create_dir_all(crate::util::fs::long_path(dest.parent().unwrap()))?;
    crate::util::fs::atomic_write(&dest, &bytes)?;
    Ok(dest)
}

use std::collections::HashMap;
use serde::Deserialize;

/// Полная установка Forge в инстанс (headless) — тот же путь, что у NeoForge.
pub async fn install(
    client: &HttpClient,
    cache_dir: &Path,
    java_exe: &Path,
    game_dir: &Path,
    instance_versions_dir: &Path,
    mc: &str,
    version: &str,
) -> Result<String> {
    let installer = download_installer(client, cache_dir, mc, version).await?;
    let id = crate::loaders::run_headless_installer(java_exe, &installer, game_dir)?;

    let json = game_dir.join("versions").join(&id).join(format!("{id}.json"));
    if json.exists() {
        let data = std::fs::read(crate::util::fs::long_path(&json))?;
        crate::util::fs::atomic_write(
            &instance_versions_dir.join(format!("{id}.json")),
            &data,
        )?;
    } else {
        return Err(LauncherError::Internal(format!(
            "инсталлятор не оставил JSON для {id}"
        )));
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_promotions_fixture() {
        let d: std::collections::HashMap<String, serde_json::Value> = serde_json::from_str(
            include_str!("../../tests/fixtures/forge_promotions.json"),
        )
        .unwrap();
        let promos: HashMap<String, String> = d
            .get("promos")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap();
        assert!(promos.contains_key("1.20.1-recommended"));
    }

    #[test]
    fn latest_prefers_recommended() {
        let mut promos: HashMap<String, String> = HashMap::new();
        promos.insert("1.20.1-recommended".into(), "47.3.0".into());
        promos.insert("1.20.1-latest".into(), "47.4.0".into());
        let v = promos
            .get("1.20.1-recommended")
            .or_else(|| promos.get("1.20.1-latest"))
            .cloned()
            .unwrap();
        assert_eq!(v, "47.3.0");
    }
}
