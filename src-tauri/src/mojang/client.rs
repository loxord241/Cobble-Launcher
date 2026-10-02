//! Клиент Minecraft: планирование и путь в сторе (спека §6.1.2).

use crate::errors::{LauncherError, Result};
use crate::mojang::version::ResolvedVersion;
use crate::net::download::DownloadTask;
use std::path::Path;

/// Задача загрузки клиента jar в стор по sha1 (`store/clients/<sha1>.jar`).
/// Клиент без хэша не качается никогда (спека §3).
pub fn plan_client_task(rv: &ResolvedVersion, clients_store: &Path, group: &str) -> Result<DownloadTask> {
    let info = rv.client_download.as_ref().ok_or_else(|| {
        LauncherError::InvalidInput(format!("в версии {} нет downloads.client", rv.id))
    })?;
    let sha1 = info.sha1.clone().ok_or_else(|| {
        LauncherError::InvalidInput("у клиента нет sha1 — скачивание запрещено (спека §3)".into())
    })?;
    Ok(DownloadTask {
        id: format!("client:{}", rv.id),
        url: info.url.clone(),
        dest: clients_store.join(format!("{sha1}.jar")),
        sha1: Some(sha1.clone()),
        size: info.size,
        group: group.to_string(),
        priority: 20, // клиент важнее ассетов
    })
}

/// Путь к клиент-jar в сторе по хэшу.
pub fn client_jar_in_store(clients_store: &Path, sha1: &str) -> std::path::PathBuf {
    clients_store.join(format!("{sha1}.jar"))
}

/// Путь к клиент-jar из ResolvedVersion (или None, если клиента нет).
pub fn client_jar_path(rv: &ResolvedVersion, clients_store: &Path) -> Option<std::path::PathBuf> {
    rv.client_download
        .as_ref()
        .and_then(|c| c.sha1.as_deref())
        .map(|sha| client_jar_in_store(clients_store, sha))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mojang::version::DownloadInfo;

    #[test]
    fn client_task_uses_sha1_store_path() {
        let rv = ResolvedVersion {
            id: "1.20.1".into(),
            version_type: Some("release".into()),
            main_class: "net.minecraft.client.main.Main".into(),
            assets: "5".into(),
            asset_index: None,
            client_download: Some(DownloadInfo {
                url: "https://piston-data.mojang.com/v1/objects/aaaa/client.jar".into(),
                sha1: Some("aaaa".repeat(10)),
                size: Some(23_000_000),
            }),
            java_version: None,
            arguments_jvm: vec![],
            arguments_game: vec![],
            minecraft_arguments: None,
            libraries: vec![],
            logging: None,
            inherits_from: None,
        };
        let t = plan_client_task(&rv, Path::new("/store/clients"), "g").unwrap();
        assert!(t.dest.to_string_lossy().ends_with(&format!("{}.jar", "a".repeat(40))));
        assert_eq!(t.priority, 20);
    }

    #[test]
    fn client_without_sha1_rejected() {
        let rv = ResolvedVersion {
            id: "x".into(),
            version_type: None,
            main_class: "M".into(),
            assets: "5".into(),
            asset_index: None,
            client_download: Some(DownloadInfo {
                url: "https://x/y.jar".into(),
                sha1: None,
                size: None,
            }),
            java_version: None,
            arguments_jvm: vec![],
            arguments_game: vec![],
            minecraft_arguments: None,
            libraries: vec![],
            logging: None,
            inherits_from: None,
        };
        assert!(plan_client_task(&rv, Path::new("/s"), "g").is_err());
    }
}
