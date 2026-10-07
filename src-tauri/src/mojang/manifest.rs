//! version_manifest_v2: fetch, кэш с TTL 1 час, фильтры по типу (спека §6.1, §7).

use crate::errors::{LauncherError, Result};
use crate::mojang::version::VersionJson;
use crate::net::http::HttpClient;
use crate::util::fs::{atomic_write, long_path};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MANIFEST_TTL: Duration = Duration::from_secs(3600); // спека §7: манифест — 1 час

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    pub time: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    /// sha1 самого version JSON — проверяется после скачивания (спека §6.2).
    pub sha1: String,
    #[serde(default)]
    pub compliance_level: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<ManifestVersion>,
}

fn cache_meta_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("version_manifest_v2.meta.json")
}

fn cache_data_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("version_manifest_v2.json")
}

/// Манифест с кэшем (TTL 1 час). Кэш-файл без TTL-метки = протухший.
/// В офлайн-режиме (ошибка сети) протухший кэш используется как fallback (спека §6.1, §6.10).
pub async fn fetch_manifest(client: &HttpClient, cache_dir: &Path) -> Result<VersionManifest> {
    std::fs::create_dir_all(crate::util::fs::long_path(cache_dir))?;
    let meta_path = cache_meta_path(cache_dir);
    if meta_path.exists() {
        if let Ok(meta) = std::fs::read(&meta_path) {
            if let Ok(fetched_at) = serde_json::from_slice::<u64>(&meta) {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if now.saturating_sub(fetched_at) < MANIFEST_TTL.as_secs() {
                    // D64: свежая TTL-метка, но data-файл удалён/не читается/бит
                    // — прежде `?` на чтении пробрасывал ошибку наверх: час
                    // падений до истечения TTL без отката к сети. Сбрасываем
                    // метку (успешная загрузка из сети перезапишет её) и идём
                    // в сеть за свежим манифестом.
                    let fresh = std::fs::read(cache_data_path(cache_dir))
                        .ok()
                        .and_then(|data| {
                            serde_json::from_slice::<VersionManifest>(&data).ok()
                        });
                    match fresh {
                        Some(m) => return Ok(m),
                        None => {
                            tracing::warn!(
                                "кэш манифеста не читается при свежей TTL-метке — метка сброшена, идём в сеть"
                            );
                            let _ = std::fs::remove_file(&meta_path);
                        }
                    }
                }
            }
        }
    }

    match client.get_json_retry(MANIFEST_URL).await {
        Ok(m) => {
            atomic_write(&cache_data_path(cache_dir), &serde_json::to_vec_pretty(&m)?)?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            atomic_write(&meta_path, &serde_json::to_vec(&now)?)?;
            Ok(m)
        }
        Err(e) => {
            // Офлайн-fallback: если есть кэш (пусть даже протухший) — используем его
            let data_path = cache_data_path(cache_dir);
            if data_path.exists() {
                if let Ok(data) = std::fs::read(&data_path) {
                    if let Ok(m) = serde_json::from_slice::<VersionManifest>(&data) {
                        tracing::warn!("fetch_manifest не удался ({e}), используем кэш");
                        return Ok(m);
                    }
                }
            }
            Err(e)
        }
    }
}

/// Найти версию: `latest`/`latest.release` → последний релиз, `latest.snapshot`
/// → снапшот, иначе — точный id.
pub fn resolve_entry<'a>(
    manifest: &'a VersionManifest,
    id_or_latest: &str,
) -> Result<&'a ManifestVersion> {
    match id_or_latest {
        "latest" | "latest.release" => manifest
            .versions
            .iter()
            .find(|v| v.id == manifest.latest.release)
            .ok_or_else(|| {
                LauncherError::VersionNotFound(manifest.latest.release.clone())
            }),
        "latest.snapshot" => manifest
            .versions
            .iter()
            .find(|v| v.id == manifest.latest.snapshot)
            .ok_or_else(|| {
                LauncherError::VersionNotFound(manifest.latest.snapshot.clone())
            }),
        _ => manifest
            .versions
            .iter()
            .find(|v| v.id == id_or_latest)
            .ok_or_else(|| LauncherError::VersionNotFound(id_or_latest.into())),
    }
}

/// Фильтр списка версий для UI (спека §8: снапшоты/старые — по настройкам).
pub fn filter_versions(
    manifest: &VersionManifest,
    show_snapshots: bool,
    show_old: bool,
) -> Vec<&ManifestVersion> {
    manifest
        .versions
        .iter()
        .filter(|v| match v.version_type.as_str() {
            "release" => true,
            "snapshot" => show_snapshots,
            "old_alpha" | "old_beta" => show_old,
            _ => false,
        })
        .collect()
}

/// Version JSON: из кэша по sha1, иначе скачать с обязательной проверкой sha1
/// (спека §6.2). Кэш: `<cache_dir>/<id>.json`.
pub async fn fetch_version_json_cached(
    client: &HttpClient,
    entry: &ManifestVersion,
    cache_dir: &Path,
) -> Result<VersionJson> {
    // Аудит 2026-10-06: id приходит из удалённого манифеста и попадает в имя
    // файла — до склейки пути проверяем набор символов.
    require_safe_file_stem(&entry.id)?;
    let path = cache_dir.join(format!("{}.json", entry.id));
    let long = long_path(&path);
    if long.exists() {
        let data = std::fs::read(&long)?;
        if crate::util::fs::sha1_bytes(&data) == entry.sha1 {
            return Ok(serde_json::from_slice(&data)?);
        }
        let _ = std::fs::remove_file(&long);
    }
    let v = VersionJson::fetch(client, &entry.url, &entry.sha1).await?;
    atomic_write(&path, &serde_json::to_vec_pretty(&v)?)?;
    Ok(v)
}

/// Имя версии как имя кэш-файла: только [A-Za-z0-9._-] (id из удалённого
/// манифеста; разделителей пути и прочей экзотики быть не должно).
fn require_safe_file_stem(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(LauncherError::InvalidInput(format!(
            "некорректный id версии из манифеста: {id}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_manifest_fixture() {
        let m: VersionManifest =
            serde_json::from_str(include_str!("../../tests/fixtures/version_manifest_sample.json"))
                .unwrap();
        assert_eq!(m.latest.release, "1.20.1");
        let e = resolve_entry(&m, "latest").unwrap();
        assert_eq!(e.id, "1.20.1");
        assert!(resolve_entry(&m, "no-such-version-xyz").is_err());
    }

    #[test]
    fn filters_by_type() {
        let m: VersionManifest =
            serde_json::from_str(include_str!("../../tests/fixtures/version_manifest_sample.json"))
                .unwrap();
        assert_eq!(filter_versions(&m, false, false).len(), 1); // только релизы
        assert_eq!(filter_versions(&m, true, false).len(), 2); // + снапшот
        assert_eq!(filter_versions(&m, true, true).len(), 4); // + alpha/beta
    }

    #[tokio::test]
    async fn fetch_manifest_uses_stale_cache_when_network_fails() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path();
        let fixture = include_str!("../../tests/fixtures/version_manifest_sample.json");
        // Записываем старый кэш без meta (т.е. TTL заведомо истёк)
        std::fs::write(cache_data_path(cache_dir), fixture).unwrap();

        // Несуществующий прокси гарантирует ошибку сети
        let client = HttpClient::new(Some("http://127.0.0.1:54321")).unwrap();
        let m = fetch_manifest(&client, cache_dir).await.expect("должен вернуть stale-кэш при ошибке сети");
        assert_eq!(m.latest.release, "1.20.1");
    }

    /// D64: свежая TTL-метка + валидный data-файл — кэш отдаётся и офлайн.
    #[tokio::test]
    async fn fresh_ttl_serves_cache_even_offline() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path();
        let now: u64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        std::fs::write(cache_meta_path(cache_dir), serde_json::to_vec(&now).unwrap()).unwrap();
        std::fs::write(
            cache_data_path(cache_dir),
            include_str!("../../tests/fixtures/version_manifest_sample.json"),
        )
        .unwrap();

        let client = HttpClient::new(None).unwrap();
        client.set_offline(true);
        let m = fetch_manifest(&client, cache_dir)
            .await
            .expect("свежий кэш должен отдаваться без сети");
        assert_eq!(m.latest.release, "1.20.1");
        // Метка не тронута — кэш в силе.
        assert!(cache_meta_path(cache_dir).exists());
    }

    /// D64-регресс: свежая TTL-метка + удалённый/битый data-файл — раньше
    /// `?` на чтении давал час падений без отката к сети. Теперь метка
    /// сбрасывается и вызов идёт в сеть (здесь — офлайн, честная ошибка).
    #[tokio::test]
    async fn fresh_ttl_with_broken_data_resets_meta() {
        let broken_data: &[u8] = b"{ not a json }";
        for broken in [None, Some(broken_data)] {
            let dir = tempfile::tempdir().unwrap();
            let cache_dir = dir.path();
            let now: u64 = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            std::fs::write(cache_meta_path(cache_dir), serde_json::to_vec(&now).unwrap()).unwrap();
            if let Some(data) = broken {
                std::fs::write(cache_data_path(cache_dir), data).unwrap();
            } // None — data-файл удалён

            let client = HttpClient::new(None).unwrap();
            client.set_offline(true); // сеть недоступна — мгновенный отказ
            let r = fetch_manifest(&client, cache_dir).await;
            assert!(r.is_err(), "сети нет и кэш нечитаем — должна быть ошибка");
            assert!(
                !cache_meta_path(cache_dir).exists(),
                "TTL-метка обязана быть сброшена (broken={})",
                broken.is_some()
            );
        }
    }

    /// Аудит 2026-10-06: id из удалённого манифеста с разделителями пути —
    /// честный InvalidInput ДО кэша и сети (сеть в тесте не нужна).
    #[tokio::test]
    async fn fetch_version_json_rejects_unsafe_id_before_io() {
        let client = HttpClient::new(Some("http://127.0.0.1:54321")).unwrap();
        let entry = ManifestVersion {
            id: "../evil".into(),
            version_type: "release".into(),
            url: "https://example.invalid/none.json".into(),
            time: "2023-01-01T00:00:00+00:00".into(),
            release_time: "2023-01-01T00:00:00+00:00".into(),
            sha1: "0".repeat(40),
            compliance_level: 1,
        };
        let err = fetch_version_json_cached(&client, &entry, Path::new("unused"))
            .await
            .expect_err("небезопасный id должен дать ошибку");
        assert!(
            matches!(err, LauncherError::InvalidInput(_)),
            "ожидался InvalidInput, получен {err:?}"
        );
    }
}
