//! Установка контента Modrinth в инстанс (спека §6.6): выбор совместимой
//! версии, рекурсивные обязательные зависимости, content-manifest.json.

use crate::errors::{LauncherError, Result};
use crate::instances::{Instance, ContentEntry, ContentKind, ContentSource};
use crate::net::download::{DownloadEngine, DownloadTask};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use std::collections::HashSet;
use std::sync::Arc;

use super::api::{self, Version};

/// Совместимая версия: game_versions содержит mc (или универсальная "*"),
/// loaders содержит loader (кейс-независимо); `loader=None` — фильтр по
/// загрузчику не применяется (ресурспаки/шейдеры).
pub fn pick_compatible_version<'a>(
    versions: &'a [Version],
    mc: &str,
    loader: Option<&str>,
) -> Option<&'a Version> {
    versions.iter().find(|v| {
        let mc_ok = v.game_versions.iter().any(|g| g == mc || g == "*");
        let loader_ok = match loader {
            None => true,
            Some(l) => v.loaders.iter().any(|x| x.eq_ignore_ascii_case(l)),
        };
        mc_ok && loader_ok && !v.files.is_empty()
    })
}

/// Каталог установки по типу контента (спека §6.6: правильные каталоги).
pub fn dir_for_kind(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Mod => "mods",
        ContentKind::ResourcePack => "resourcepacks",
        ContentKind::Shader => "shaderpacks",
    }
}

fn kind_for_project_type(project_type: &str) -> Result<ContentKind> {
    match project_type {
        "mod" => Ok(ContentKind::Mod),
        "resourcepack" => Ok(ContentKind::ResourcePack),
        "shader" => Ok(ContentKind::Shader),
        other => Err(LauncherError::InvalidInput(format!(
            "неподдерживаемый тип контента: {other}"
        ))),
    }
}

/// Установить проект (мод/ресурспак/шейдер) в инстанс. Обязательные зависимости
/// — рекурсивно (авто), optional — пропускаются (диалог в UI — M6+).
/// Возвращает список установленных записей этого вызова.
pub async fn install_project(
    paths: &Paths,
    client: Arc<HttpClient>,
    engine: Arc<DownloadEngine>,
    inst: &Instance,
    project_id: &str,
    version_id: Option<&str>,
) -> Result<Vec<ContentEntry>> {
    // Тип проекта → каталог; для модов фильтр по загрузчику инстанса.
    let project = api::project(&client, project_id).await?;
    let project_type = project
        .get("project_type")
        .and_then(|v| v.as_str())
        .unwrap_or("mod")
        .to_string();
    let kind = kind_for_project_type(&project_type)?;
    let loader = if kind == ContentKind::Mod {
        // Vanilla-инстанс (без загрузчика): предпочитаем fabric-сборку —
        // большинство модов публикует fabric первыми; иначе любая сборка.
        Some(inst.loader.clone().unwrap_or_else(|| "fabric".into()))
    } else {
        None
    };

    let mut ctx = InstallCtx {
        engine,
        visited: HashSet::new(),
        installed: Vec::new(),
    };
    install_recursive(
        paths,
        client,
        inst,
        project_id,
        version_id,
        &kind,
        loader.as_deref(),
        &mut ctx,
        0,
    )
    .await?;
    Ok(ctx.installed)
}

struct InstallCtx {
    engine: Arc<DownloadEngine>,
    visited: HashSet<String>,
    installed: Vec<ContentEntry>,
}

#[allow(clippy::too_many_arguments)]
async fn install_recursive(
    paths: &Paths,
    client: Arc<HttpClient>,
    inst: &Instance,
    project_id: &str,
    version_id: Option<&str>,
    kind: &ContentKind,
    loader: Option<&str>,
    ctx: &mut InstallCtx,
    depth: usize,
) -> Result<()> {
    if depth > 10 {
        return Err(LauncherError::InvalidInput(
            "слишком глубокая цепочка зависимостей (>10)".into(),
        ));
    }
    if !ctx.visited.insert(project_id.to_string()) {
        return Ok(()); // уже устанавливали
    }

    let versions = api::project_versions(&client, project_id).await?;
    let version = if let Some(vid) = version_id {
        versions
            .iter()
            .find(|v| v.id == vid)
            .ok_or_else(|| LauncherError::VersionNotFound(vid.into()))?
    } else {
        pick_compatible_version(&versions, &inst.mc_version, loader).ok_or_else(|| {
            LauncherError::VersionNotFound(format!(
                "совместимая версия {project_id} для {} ({:?})",
                inst.mc_version, loader
            ))
        })?
    };

    // Обязательные зависимости — сначала (рекурсия).
    for dep in &version.dependencies {
        if dep.dependency_type != "required" {
            continue; // optional/embedded/incompatible — не ставим автоматически
        }
        let dep_project = dep
            .project_id
            .clone()
            .ok_or_else(|| {
                LauncherError::InvalidInput(format!(
                    "зависимость без project_id у версии {}",
                    version.id
                ))
            })?;
        Box::pin(install_recursive(
            paths, client.clone(), inst, &dep_project, None, kind, loader, ctx, depth + 1,
        ))
        .await?;
    }

    // Основной файл (primary или первый) → каталог контента.
    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| LauncherError::InvalidInput(format!("у версии {} нет файлов", version.id)))?;

    let dir = dir_for_kind(*kind);
    let dest = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id))
        .join(dir)
        .join(&file.filename);
    ctx.engine.add_tasks(vec![DownloadTask {
        id: format!("content:{}", file.filename),
        url: file.url.clone(),
        dest,
        sha1: file.hashes.get("sha1").cloned(),
        size: Some(file.size),
        group: format!("instance:{}", inst.id),
        priority: 20,
    }]);
    ctx.engine.run().await?;
    let st = ctx.engine.queue_state();
    if st.failed > 0 {
        return Err(LauncherError::network(format!(
            "загрузка контента не удалась: {}",
            st.failed_items
                .first()
                .map(|(_, r)| r.clone())
                .unwrap_or_default()
        )));
    }

    // Манифест: заменить записи этого проекта (переустановка), добавить новую.
    let entry = ContentEntry {
        kind: *kind,
        file: format!("{dir}/{}", file.filename),
        source: ContentSource::Modrinth,
        project_id: Some(project_id.to_string()),
        version_id: Some(version.id.clone()),
        sha1: file.hashes.get("sha1").cloned(),
        enabled: true,
    };
    let mut manifest = crate::instances::load_content_manifest(paths, &inst.id);
    manifest.retain(|e| e.project_id.as_deref() != Some(project_id));
    manifest.push(entry.clone());
    crate::instances::save_content_manifest(paths, &inst.id, &manifest)?;
    ctx.installed.push(entry);
    Ok(())
}
