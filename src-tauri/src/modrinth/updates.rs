//! Обновления контента (спека §6.6): проверка совместимых свежих версий,
//! батч-обновление с ченджлогами и откатом (старый jar → .trash/).

use crate::errors::{LauncherError, Result};
use crate::instances::{ContentKind, ContentSource, Instance};
use crate::modrinth::api;
use crate::net::download::{DownloadEngine, DownloadTask};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use serde::Serialize;
use std::sync::Arc;

/// Найденное обновление одного контента.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub file: String,
    pub project_id: String,
    pub current_version_id: String,
    pub latest_version_id: String,
    pub latest_version_number: String,
    pub changelog: Option<String>,
}

/// Проверить обновления для всех записей манифеста с source=modrinth.
pub async fn check_updates(
    paths: &Paths,
    client: &HttpClient,
    inst: &Instance,
) -> Result<Vec<UpdateCheck>> {
    let manifest = crate::instances::load_content_manifest(paths, &inst.id);
    let loader = if inst.loader.as_deref() == Some("fabric") || inst.loader.as_deref() == Some("quilt") {
        inst.loader.clone()
    } else {
        // neoforge-моды ищутся с categories:neoforge
        inst.loader.clone()
    };
    let mut out = Vec::new();
    for entry in &manifest {
        if entry.source != ContentSource::Modrinth {
            continue;
        }
        let Some(project_id) = &entry.project_id else {
            continue;
        };
        let Some(current) = &entry.version_id else {
            continue;
        };
        let versions = api::project_versions(client, project_id).await?;
        let loader_filter = if entry.kind == ContentKind::Mod {
            loader.as_deref()
        } else {
            None
        };
        if let Some(latest) =
            crate::modrinth::install::pick_compatible_version(&versions, &inst.mc_version, loader_filter)
        {
            if &latest.id != current {
                out.push(UpdateCheck {
                    file: entry.file.clone(),
                    project_id: project_id.clone(),
                    current_version_id: current.clone(),
                    latest_version_id: latest.id.clone(),
                    latest_version_number: latest.version_number.clone(),
                    changelog: latest.changelog.clone(),
                });
            }
        }
    }
    Ok(out)
}

/// Батч-обновление: новая версия скачивается, старый файл → `.trash/`,
/// манифест обновляется. Возвращает число обновлённых.
pub async fn update_all(
    paths: &Paths,
    client: Arc<HttpClient>,
    engine: Arc<DownloadEngine>,
    inst: &Instance,
) -> Result<usize> {
    let checks = check_updates(paths, &client, inst).await?;
    let manifest = crate::instances::load_content_manifest(paths, &inst.id);
    let game_dir =
        crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
    let trash = game_dir.join(".trash");
    std::fs::create_dir_all(crate::util::fs::long_path(&trash))?;
    let mut updated = 0;

    for check in &checks {
        let versions = api::project_versions(&client, &check.project_id).await?;
        let Some(latest) = versions.iter().find(|v| v.id == check.latest_version_id) else {
            continue;
        };
        let Some(file) = latest.files.iter().find(|f| f.primary).or_else(|| latest.files.first())
        else {
            continue;
        };
        let Some(old_entry) = manifest.iter().find(|e| e.file == check.file) else {
            continue;
        };

        // Старый файл → корзина инстанса (откат возможен вручную, спека §6.6).
        let old_path = game_dir.join(&old_entry.file);
        if old_path.exists() {
            let name = old_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("old-{}", check.project_id));
            let _ = std::fs::rename(
                crate::util::fs::long_path(&old_path),
                crate::util::fs::long_path(&trash.join(format!(
                    "{}-{}",
                    chrono_suffix(),
                    name
                ))),
            );
        }

        // Новый файл.
        let dir = crate::modrinth::install::dir_for_kind(old_entry.kind);
        let dest = game_dir.join(dir).join(&file.filename);
        engine.add_tasks(vec![DownloadTask {
            id: format!("update:{}", file.filename),
            url: file.url.clone(),
            dest,
            sha1: file.hashes.get("sha1").cloned(),
            size: Some(file.size),
            group: format!("instance:{}", inst.id),
            priority: 25,
        }]);
        engine.run().await?;
        let st = engine.queue_state();
        if st.failed > 0 {
            return Err(LauncherError::network(format!(
                "обновление {} не скачалось",
                file.filename
            )));
        }

        // Манифест: заменить запись.
        let mut manifest = crate::instances::load_content_manifest(paths, &inst.id);
        if let Some(e) = manifest
            .iter_mut()
            .find(|e| e.file == check.file)
        {
            e.file = format!("{dir}/{}", file.filename);
            e.version_id = Some(latest.id.clone());
            e.sha1 = file.hashes.get("sha1").cloned();
        }
        crate::instances::save_content_manifest(paths, &inst.id, &manifest)?;
        updated += 1;
    }
    Ok(updated)
}

fn chrono_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}
