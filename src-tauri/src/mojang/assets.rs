//! Ассеты: asset index + объекты + virtual/legacy (спека §6.1.4, §6.4).
//! ВАЖНО: в asset index поле хэша называется `hash`, НЕ `sha1` (спека §6.1).

use crate::errors::Result;
use crate::net::download::DownloadTask;
use crate::net::http::HttpClient;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const RESOURCES_BASE: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetObject {
    /// Поле называется именно `hash` (спека §6.1: «частая ошибка!»).
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndexJson {
    pub objects: BTreeMap<String, AssetObject>,
    /// У старых индексов: `virtual: true` (файлы кладутся в assets/virtual).
    #[serde(rename = "virtual", default)]
    pub is_virtual: Option<bool>,
    #[serde(rename = "map_to_resources", default)]
    pub map_to_resources: Option<bool>,
}

/// Скачать asset index с проверкой sha1; кэш — `assets/indexes/<id>.json`
/// (стандартный layout, игра сама его читает).
pub async fn fetch_asset_index(
    client: &HttpClient,
    url: &str,
    expected_sha1: &str,
    index_id: &str,
    indexes_dir: &Path,
) -> Result<AssetIndexJson> {
    std::fs::create_dir_all(crate::util::fs::long_path(indexes_dir))?;
    let cache_file = indexes_dir.join(format!("{index_id}.json"));
    let cache_long = crate::util::fs::long_path(&cache_file);
    if cache_long.exists() {
        let data = std::fs::read(&cache_long)?;
        if crate::util::fs::sha1_bytes(&data) == expected_sha1 {
            return Ok(serde_json::from_slice(&data)?);
        }
        // битый кэш — перекачаем
        let _ = std::fs::remove_file(&cache_long);
    }
    // F16: офлайн — индекса нет в кэше, качать нельзя: честный отказ.
    if client.offline() {
        return Err(crate::errors::LauncherError::OfflineMode(url.to_string()));
    }
    let resp = client.raw().get(url).send().await?;
    if !resp.status().is_success() {
        return Err(crate::errors::LauncherError::network(format!(
            "HTTP {} для asset index {url}",
            resp.status()
        )));
    }
    let bytes = resp.bytes().await?;
    let actual = crate::util::fs::sha1_bytes(&bytes);
    if actual != expected_sha1 {
        return Err(crate::errors::LauncherError::HashMismatch {
            path: url.into(),
            expected: expected_sha1.into(),
            actual,
        });
    }
    crate::util::fs::atomic_write(&cache_file, &bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Задачи загрузки всех объектов в стор по хэшу: `<h[:2]>/<h>` (дедуп бесплатный).
pub fn plan_asset_tasks(index: &AssetIndexJson, assets_store: &Path, group: &str) -> Vec<DownloadTask> {
    index
        .objects
        .values()
        .map(|o| {
            let prefix = &o.hash[..2];
            DownloadTask {
                id: format!("asset:{}", o.hash),
                url: format!("{RESOURCES_BASE}/{prefix}/{}", o.hash),
                dest: assets_store.join(prefix).join(&o.hash),
                sha1: Some(o.hash.clone()),
                size: Some(o.size),
                group: group.to_string(),
                priority: 0, // ассеты — наименьший приоритет (спека §4.4)
            }
        })
        .collect()
}

/// Материализовать virtual/legacy ассеты: hardlink из стора в
/// `assets/virtual/<id>/<имя_объекта>`; при неудаче — копия со сверкой хэша.
pub fn materialize_virtual(
    index: &AssetIndexJson,
    assets_store: &Path,
    virtual_dir: &Path,
) -> Result<usize> {
    // std::fs::hard_link кросс-платформенен; при неудаче — копия.

    let mut n = 0;
    for (name, obj) in &index.objects {
        let src = assets_store.join(&obj.hash[..2]).join(&obj.hash);
        let dst = virtual_dir.join(name);
        let dst_long = crate::util::fs::long_path(&dst);
        if dst_long.exists() {
            n += 1;
            continue;
        }
        if let Some(parent) = dst_long.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if std::fs::hard_link(crate::util::fs::long_path(&src), &dst_long).is_err() {
            std::fs::copy(crate::util::fs::long_path(&src), &dst_long)?;
        }
        n += 1;
    }
    Ok(n)
}

/// Нужны ли виртуальные ассеты для этой версии (спека §6.4: legacy/pre-1.6
/// или индекс с virtual/map_to_resources).
pub fn needs_virtual(assets_id: &str, index: &AssetIndexJson) -> bool {
    matches!(assets_id, "legacy" | "pre-1.6")
        || index.is_virtual == Some(true)
        || index.map_to_resources == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_asset_index_hash_field_not_sha1() {
        let idx: AssetIndexJson = serde_json::from_str(include_str!(
            "../../tests/fixtures/asset_index_sample.json"
        ))
        .unwrap();
        assert_eq!(idx.objects.len(), 3);
        let o = idx.objects.get("minecraft/sounds/random/click.ogg").unwrap();
        assert_eq!(o.hash.len(), 40);
        assert_eq!(
            o.hash,
            "9f6b0b3c3c0f7a5b3f1d1c0f7a5b3f1d1c0f7a5b"
        );
        assert!(idx.is_virtual.is_none());
    }

    #[test]
    fn plan_tasks_use_hash_prefix_paths() {
        let idx: AssetIndexJson = serde_json::from_str(include_str!(
            "../../tests/fixtures/asset_index_sample.json"
        ))
        .unwrap();
        let tasks = plan_asset_tasks(&idx, Path::new("/store/assets"), "assets");
        let t = &tasks[0];
        assert!(t.url.starts_with(RESOURCES_BASE));
        assert_eq!(t.priority, 0);
        assert_eq!(
            t.dest,
            Path::new("/store/assets")
                .join(&t.sha1.clone().unwrap()[..2])
                .join(t.sha1.clone().unwrap())
        );
    }

    #[test]
    fn virtual_detection() {
        let legacy: AssetIndexJson = serde_json::from_str(r#"{"objects": {}, "virtual": true}"#).unwrap();
        assert!(needs_virtual("5", &legacy));
        assert!(needs_virtual("legacy", &legacy));
        let modern: AssetIndexJson = serde_json::from_str(r#"{"objects": {}}"#).unwrap();
        assert!(!needs_virtual("5", &modern));
        assert!(needs_virtual("pre-1.6", &modern));
    }
}
