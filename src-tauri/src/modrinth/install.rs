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
        ContentKind::Datapack => "datapacks",
    }
}

fn kind_for_project_type(project_type: &str) -> Result<ContentKind> {
    match project_type {
        "mod" => Ok(ContentKind::Mod),
        "resourcepack" => Ok(ContentKind::ResourcePack),
        "shader" => Ok(ContentKind::Shader),
        "datapack" => Ok(ContentKind::Datapack),
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
    // id/slug уходит в URL запроса — мусор из IPC-границы отсекаем до сети.
    api::validate_id_or_slug(project_id)?;
    let project = api::project(&client, project_id).await?;
    let project_type = project
        .get("project_type")
        .and_then(|v| v.as_str())
        .unwrap_or("mod")
        .to_string();
    let kind = kind_for_project_type(&project_type)?;
    // D64: мод в ванильный инстанс раньше «ставился» молча — jar ложился в
    // mods/, но игра без загрузчика его не грузит. Честный отказ вместо тихо
    // неработающего мода (ресурспаки/шейдеры загрузчика не требуют).
    let loader = if kind == ContentKind::Mod {
        Some(inst.loader.clone().ok_or_else(|| {
            LauncherError::InvalidInput(
                "это мод — для него нужен Fabric/Forge/NeoForge/Quilt; создай инстанс с загрузчиком или выбери ресурс-пак/шейдер"
                    .into(),
            )
        })?)
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
    // Зависимости приходят из ответа API, но id уходит в URL — guard и здесь.
    api::validate_id_or_slug(project_id)?;

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

    let engine = ctx.engine.clone();
    let installed = &mut ctx.installed;
    download_and_record(
        paths,
        inst,
        &engine,
        *kind,
        project_id,
        &version.id,
        file,
        installed,
    )
    .await
}

/// Скачать файл контента и записать его в content-manifest.json.
/// Вынесено отдельно, чтобы решение «писать ли запись» было покрыто тестом (A18).
#[allow(clippy::too_many_arguments)]
async fn download_and_record(
    paths: &Paths,
    inst: &Instance,
    engine: &Arc<DownloadEngine>,
    kind: ContentKind,
    project_id: &str,
    version_id: &str,
    file: &api::VersionFile,
    installed: &mut Vec<ContentEntry>,
) -> Result<()> {
    let dir = dir_for_kind(kind);
    // D64: filename из API — не доверенный путь: тот же санитайзер, что в
    // updates/mrpack (join с «именем» вида `../../x` не должен писать мимо
    // каталога контента). Поле `file` в манифесте — из того же имени, иначе
    // обновления не найдут файл на диске.
    let name = api::sanitize_file_name(&file.filename);
    let dest = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id))
        .join(dir)
        .join(&name);
    // Железное правило: файл без хэша из доверенного манифеста не качаем
    // (Modrinth отдаёт sha1 и/или sha512 — берём оба, сверяется движок).
    let sha1 = file.hashes.get("sha1").cloned();
    let sha512 = file.hashes.get("sha512").cloned();
    if sha1.is_none() && sha512.is_none() {
        return Err(LauncherError::InvalidInput(format!(
            "в манифесте нет хэша для {}",
            file.filename
        )));
    }
    engine.add_tasks(vec![DownloadTask {
        id: format!("content:{}", file.filename),
        url: file.url.clone(),
        dest,
        sha1,
        sha512,
        size: Some(file.size),
        group: format!("instance:{}", inst.id),
        priority: 20,
    }]);
    engine.run().await?;
    let st = engine.queue_state();
    if st.failed > 0 {
        return Err(LauncherError::network(format!(
            "загрузка контента не удалась: {}",
            st.failed_items
                .first()
                .map(|(_, r)| r.clone())
                .unwrap_or_default()
        )));
    }
    // A18: отмена (группа/глобальная) переводит задачу в Cancelled, а НЕ в Failed —
    // без этой проверки запись о «скачанном» моде попадала в content-manifest.json,
    // хотя файла на диске нет (мод исчезает, а манифест врёт про обновления).
    if st.cancelled > 0 {
        return Err(LauncherError::Cancelled);
    }

    // Манифест: заменить записи этого проекта (переустановка), добавить новую.
    let entry = ContentEntry {
        kind,
        file: format!("{dir}/{name}"),
        source: ContentSource::Modrinth,
        project_id: Some(project_id.to_string()),
        version_id: Some(version_id.to_string()),
        sha1: file.hashes.get("sha1").cloned(),
        url: Some(file.url.clone()),
        enabled: true,
    };
    let mut manifest = crate::instances::load_content_manifest(paths, &inst.id);
    manifest.retain(|e| e.project_id.as_deref() != Some(project_id));
    manifest.push(entry.clone());
    crate::instances::save_content_manifest(paths, &inst.id, &manifest)?;
    installed.push(entry);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBus;
    use std::time::{Duration, Instant};

    fn test_paths(dir: &tempfile::TempDir) -> Paths {
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        paths
    }

    /// id с `../` или пробелом — InvalidInput ДО сети: клиент в офлайн-режиме,
    /// любая попытка похода в сеть вернула бы OfflineMode (F16), а не
    /// invalid_input. Подмена пути запроса (`../`) отсекается на границе.
    #[tokio::test]
    async fn bad_project_id_fails_before_network() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let inst = Instance::new("Тест", "1.21.1");
        let client = Arc::new(HttpClient::new(None).unwrap());
        client.set_offline(true);
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        for bad in ["../evil", "a b", ""] {
            let err = install_project(&paths, client.clone(), engine.clone(), &inst, bad, None)
                .await
                .unwrap_err();
            assert_eq!(err.code(), "invalid_input", "{bad:?}: {err}");
        }
    }

    async fn spawn_axum(app: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    /// A18-регресс: отменённая загрузка контента НЕ попадает в манифест
    /// (до фикса задача получала Cancelled, но запись всё равно писалась).
    #[tokio::test]
    async fn cancelled_download_is_not_recorded_in_manifest() {
        use futures::StreamExt as _;
        let app = axum::Router::new().route(
            "/slow.jar",
            axum::routing::get(|| async {
                let stream = futures::stream::repeat_with(|| Ok::<_, std::io::Error>(b"chunk".to_vec()))
                    .take(200)
                    .then(|c| async move {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        c
                    });
                axum::body::Body::from_stream(stream)
            }),
        );
        let base = spawn_axum(app).await;

        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let inst = Instance::new("Тест", "1.21.1");
        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client, 2, EventBus::default());
        // Файл обязан иметь хэш (железное правило) — тест отмены не про это,
        // поэтому даём фиктивный sha512: до его сверки загрузка не доживёт
        // (отмена приходит посреди потока).
        let file = api::VersionFile {
            hashes: [("sha512".to_string(), "0".repeat(128))]
                .into_iter()
                .collect(),
            url: format!("{base}/slow.jar"),
            filename: "x.jar".into(),
            primary: true,
            size: 1000,
        };
        let game = crate::instances::minecraft_dir(&crate::instances::instance_dir(&paths, &inst.id));

        // Отменяем группу, как только движок реально начал писать `.part`
        // (после этого флаг группы уже существует) — вместо фиксированного sleep.
        let group = format!("instance:{}", inst.id);
        let part = game.join("mods").join("x.jar.part");
        let part_probe = part.clone();
        let e2 = engine.clone();
        let cancel = tokio::spawn(async move {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !part_probe.exists() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            e2.cancel_group(&group);
        });

        let mut installed = Vec::new();
        let err = download_and_record(
            &paths,
            &inst,
            &engine,
            ContentKind::Mod,
            "proj-1",
            "ver-1",
            &file,
            &mut installed,
        )
        .await
        .unwrap_err();
        cancel.await.unwrap();

        assert!(matches!(err, LauncherError::Cancelled), "{err}");
        assert!(installed.is_empty(), "отменённый файл не считается установленным");
        assert!(
            crate::instances::load_content_manifest(&paths, &inst.id).is_empty(),
            "манифест не должен описывать несуществующий файл"
        );
        assert!(!part.exists(), ".part убирается движком");
    }
}
