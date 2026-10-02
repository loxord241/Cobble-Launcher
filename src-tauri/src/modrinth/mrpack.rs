//! Модпаки .mrpack (спека §6.2, §6.6): modrinth.index.json + overrides/,
//! зависимости → создание инстанса нужного вида, env-фильтр client.

use crate::errors::{LauncherError, Result};
use crate::instances::{ContentEntry, ContentKind, ContentSource, Instance};
use crate::net::download::{DownloadEngine, DownloadTask};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackIndex {
    pub format_version: u32,
    #[serde(default)]
    pub game: Option<String>,
    pub version_id: String,
    pub name: String,
    #[serde(default)]
    pub files: Vec<MrpackFile>,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackFile {
    pub path: String,
    pub hashes: HashMap<String, String>,
    /// env может отсутствовать — тогда считается required (спека §6.2).
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    pub downloads: Vec<String>,
    #[serde(default)]
    pub file_size: u64,
}

impl MrpackIndex {
    /// Проверка структуры: внятные ошибки вместо паник (спека §6.7 — битые
    /// манифесты сообщать понятно).
    pub fn validate(&self) -> Result<()> {
        if self.format_version != 1 {
            return Err(LauncherError::InvalidInput(format!(
                "formatVersion {} не поддерживается (ждём 1)",
                self.format_version
            )));
        }
        if let Some(game) = &self.game {
            if game != "minecraft" {
                return Err(LauncherError::InvalidInput(format!(
                    "игра модпака: {game}, ждём minecraft"
                )));
            }
        }
        Ok(())
    }

    /// Файлы для КЛИЕНТА: env.client != "unsupported" (спека §6.6).
    pub fn client_files(&self) -> impl Iterator<Item = &MrpackFile> {
        self.files.iter().filter(|f| {
            f.env
                .as_ref()
                .and_then(|e| e.get("client").map(|c| c != "unsupported"))
                .unwrap_or(true)
        })
    }
}

/// Открыть .mrpack (zip) и распарсить modrinth.index.json.
fn open_mrpack(path: &Path) -> Result<MrpackIndex> {
    let file = std::fs::File::open(crate::util::fs::long_path(path))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| LauncherError::Zip(format!("не удалось открыть .mrpack: {e}")))?;
    let index_entry = zip
        .by_name("modrinth.index.json")
        .map_err(|_| {
            LauncherError::InvalidInput(
                "в .mrpack нет modrinth.index.json — это не модпак Modrinth".into(),
            )
        })?;
    let index: MrpackIndex = serde_json::from_reader(index_entry)?;
    index.validate()?;
    Ok(index)
}

/// Хосты, с которых .mrpack имеет право тянуть файлы (A24). Чужой архив мог
/// указать `http://127.0.0.1:8080/...` или адрес метаданных облака — лаунчер
/// сходил бы туда из процесса пользователя (SSRF). Разрешены только
/// официальные API/CDN Modrinth и GitHub-релизы, куда модпаки и кладут jar'ы.
pub const ALLOWED_MRPACK_HOSTS: &[&str] = &[
    "api.modrinth.com",
    "cdn.modrinth.com",
    "github.com",
    "objects.githubusercontent.com",
    "raw.githubusercontent.com",
];

/// Проверить URL загрузки из НЕдоверенного .mrpack: только https + allowlist.
pub fn ensure_allowed_mrpack_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url).map_err(|e| {
        LauncherError::InvalidInput(format!("в модпаке некорректный URL {url:?}: {e}"))
    })?;
    let host = parsed.host_str().unwrap_or_default();
    // Тесты гоняют загрузку на локальном axum-сервере (http, внешней сети нет) —
    // в test-сборке петельбек разрешён до проверок схемы и allowlist. В релизной
    // сборке этой ветки не существует: прод-проверка не ослаблена.
    #[cfg(test)]
    if matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1") {
        return Ok(());
    }
    if parsed.scheme() != "https" {
        return Err(LauncherError::InvalidInput(format!(
            "в модпаке разрешены только https-ссылки: {url}"
        )));
    }
    if ALLOWED_MRPACK_HOSTS
        .iter()
        .any(|h| host.eq_ignore_ascii_case(h))
    {
        return Ok(());
    }
    Err(LauncherError::InvalidInput(format!(
        "модпак ссылается на недоверенный хост {host}: разрешены {ALLOWED_MRPACK_HOSTS:?}"
    )))
}

/// Полная установка .mrpack: инстанс по dependencies → overrides поверх →
/// файлы с хэшами → content-manifest. Возвращает созданный инстанс.
///
/// Движок загрузок приходит снаружи (и со своей шиной — D11: собственный
/// `EventBus::default()` здесь означал, что `dl_progress` не доходил до UI).
/// `group` — тот же ключ, под которым движок лежит в реестре AppState; он же
/// группа задач, поэтому отмена группы отменяет именно эту установку.
pub async fn install_mrpack(
    paths: &Paths,
    settings: &crate::settings::Settings,
    engine: Arc<DownloadEngine>,
    client: Arc<HttpClient>,
    group: &str,
    mrpack_path: &Path,
    name_override: Option<&str>,
) -> Result<Instance> {
    let index = open_mrpack(mrpack_path)?;

    // 1. Инстанс нужного вида: dependencies {minecraft, fabric-loader?, …}.
    let mc = index.dependencies.get("minecraft").ok_or_else(|| {
        LauncherError::InvalidInput("в модпаке нет зависимости minecraft".into())
    })?;
    let mut inst = Instance::new(name_override.unwrap_or(&index.name), mc);
    // Загрузчик: fabric/quilt — через profile JSON, neoforge — инсталлятор,
    // forge — честный отказ до M7 (спека §15: не падать молча).
    let loader_entry = index
        .dependencies
        .iter()
        .find(|(k, _)| k.ends_with("-loader"))
        .map(|(k, v)| (k.clone(), v.clone()));
    inst.mc_version = mc.clone();

    // 2. Загрузчик разбираем ДО скачивания: «Forge пока нет» не должен стоить
    //    пользователю сотен мегабайт трафика (спека §15).
    let loader: Option<(&str, &str)> = match &loader_entry {
        None => None,
        Some((key, version)) => Some((
            match key.as_str() {
                "fabric-loader" => "fabric",
                "quilt-loader" => "quilt",
                "neoforge" => "neoforge",
                "forge" => {
                    return Err(LauncherError::InvalidInput(
                        "модпак на Forge: поддержка Forge появится на этапе M7".into(),
                    ));
                }
                other => {
                    return Err(LauncherError::InvalidInput(format!(
                        "неизвестный загрузчик модпака: {other}"
                    )));
                }
            },
            version.as_str(),
        )),
    };

    // 3. Файлы модпака в очередь (до создания инстанса на диске — каталоги
    //    создаст движок).
    let mut tasks = Vec::new();
    for f in index.client_files() {
        if f.downloads.is_empty() {
            return Err(LauncherError::InvalidInput(format!(
                "у файла {} нет ссылок для скачивания",
                f.path
            )));
        }
        // A24: сначала проверяем все зеркала файла на allowlist (SSRF), потом
        // уже строим задачу — до сети и до создания каталогов.
        for url in &f.downloads {
            ensure_allowed_mrpack_url(url)?;
        }
        // Санитизация пути из ЧУЖОГО модпака (спека §3): только относительные
        // безопасные компоненты.
        let rel = crate::util::zip::safe_relative_path(&f.path)?;
        tasks.push(DownloadTask {
            id: format!("mrpack:{}", f.path),
            url: f.downloads[0].clone(),
            dest: crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id))
                .join(rel),
            sha1: f.hashes.get("sha1").cloned(),
            size: Some(f.file_size),
            group: group.to_string(),
            priority: 15,
        });
    }

    // 4. Загрузка файлов. На диске инстанса ещё нет: сохраняем его только после
    //    успеха (D12), иначе сбой оставил бы «пустой рабочий» инстанс.
    engine.add_tasks(tasks);
    engine.run().await?;
    engine.persist_queue(&paths.queue_file())?;
    let st = engine.queue_state();
    if st.failed > 0 {
        rollback_instance(paths, &inst.id);
        return Err(LauncherError::network(format!(
            "{} файлов модпака не скачались: {}",
            st.failed,
            st.failed_items
                .first()
                .map(|(u, r)| format!("{u}: {r}"))
                .unwrap_or_default()
        )));
    }
    if st.cancelled > 0 {
        // Отмена посреди установки: файлов нет — инстанса тоже быть не должно.
        rollback_instance(paths, &inst.id);
        return Err(LauncherError::Cancelled);
    }

    // 5–6. Инстанс на диске, загрузчик, overrides. A29: сбой на любом из этих
    // шагов откатывает инстанс — до этого момента пользователь его не видел,
    // а битый (без загрузчика/overrides) остался бы в списке навсегда.
    // Наружу уходит исходная причина сбоя, откат только логируется.
    let inst = match install_loader_and_overrides(paths, settings, client, &inst, loader, mrpack_path)
        .await
    {
        Ok(updated) => updated,
        Err(e) => {
            tracing::warn!("установка модпака не удалась ({e}) — откатываю инстанс");
            rollback_instance(paths, &inst.id);
            return Err(e);
        }
    };

    // 7. content-manifest: только моды/рп/шейдеры (пути не в mods/ — local).
    let mut manifest = Vec::new();
    for f in index.client_files() {
        let rel = crate::util::zip::safe_relative_path(&f.path)?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let kind = if rel_str.starts_with("mods/") {
            ContentKind::Mod
        } else if rel_str.starts_with("resourcepacks/") {
            ContentKind::ResourcePack
        } else if rel_str.starts_with("shaderpacks/") {
            ContentKind::Shader
        } else {
            continue; // конфиги и прочее — не контент для обновлений
        };
        manifest.push(ContentEntry {
            kind,
            file: rel_str,
            source: ContentSource::Modrinth,
            // projectId зашит в ссылке CDN (cdn.modrinth.com/data/<id>/versions/…)
            // — извлекаем, чтобы UI показывал иконки и названия модов модпака.
            project_id: f.downloads.first().and_then(|u| {
                let marker = "cdn.modrinth.com/data/";
                let rest = u.split(marker).nth(1)?;
                let pid = rest.split('/').next()?;
                (!pid.is_empty()).then(|| pid.to_string())
            }),
            version_id: Some(index.version_id.clone()),
            sha1: f.hashes.get("sha1").cloned(),
            url: f.downloads.first().cloned(),
            enabled: true,
        });
    }
    crate::instances::save_content_manifest(paths, &inst.id, &manifest)?;
    Ok(inst)
}

/// Откат несостоявшейся установки: каталог инстанса создан движком под свежий
/// uuid, пользователь его не видел (в списке инстансов он не показывался) —
/// удаляем целиком, вместе с недокачанными `.part` (D12). Ошибку удаления
/// только логируем: важнее вернуть исходную причину сбоя.
fn rollback_instance(paths: &Paths, id: &str) {
    let dir = crate::instances::instance_dir(paths, id);
    if let Err(e) = std::fs::remove_dir_all(crate::util::fs::long_path(&dir)) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("откат инстанса {id}: {} не удалён: {e}", dir.display());
        }
    }
}

/// Шаги 5–6 установки модпака: сохранить инстанс, поставить загрузчик
/// (install_loader сам перезаписывает instance.json), накатить overrides.
/// Отдельная функция — чтобы вызывающий мог откатить инстанс при ЛЮБОЙ ошибке
/// одним местом (A29) и вернуть исходную причину.
async fn install_loader_and_overrides(
    paths: &Paths,
    settings: &crate::settings::Settings,
    client: Arc<HttpClient>,
    inst: &Instance,
    loader: Option<(&str, &str)>,
    mrpack_path: &Path,
) -> Result<Instance> {
    crate::instances::save(paths, inst)?;
    let mut updated = inst.clone();
    if let Some((loader, loader_version)) = loader {
        // Версию загрузчика можно не указывать: install_loader берёт latest
        // стабильную, если None — но модпак требует конкретную.
        updated = crate::instances::run::install_loader(
            paths,
            settings,
            client,
            inst,
            loader,
            Some(loader_version),
        )
        .await?;
    }
    // Overrides из .mrpack поверх каталога игры (zip-slip-защита в extract).
    apply_overrides(mrpack_path, paths, &updated.id)?;
    Ok(updated)
}

/// Распаковать `overrides/` из .mrpack поверх каталога игры.
fn apply_overrides(mrpack_path: &Path, paths: &Paths, instance_id: &str) -> Result<()> {
    let file = std::fs::File::open(crate::util::fs::long_path(mrpack_path))?;
    let dest = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, instance_id));
    let extracted = crate::util::zip::extract_zip_stripping(
        file,
        &dest,
        "overrides/",
        |_| true,
        |_| {},
    )?;
    tracing::info!("overrides применены: {} файлов", extracted.len());
    Ok(())
}

/// Установка модпака Modrinth по project_id: берём последнюю версию,
/// качаем primary-файл во временный .mrpack (sha1 проверяем — правило
/// «хэш на каждый байт») и переиспользуем install_mrpack.
///
/// Движок/шина и `group` — снаружи, как и у install_mrpack (D11); откат
/// несостоявшегося инстанса живёт в install_mrpack (до него инстанса ещё нет).
pub async fn install_modpack_project(
    paths: &Paths,
    settings: &crate::settings::Settings,
    engine: Arc<DownloadEngine>,
    client: Arc<HttpClient>,
    group: &str,
    project_id: &str,
    name_override: Option<&str>,
) -> Result<Instance> {
    let versions = crate::modrinth::api::project_versions(&client, project_id).await?;
    let version = versions.first().ok_or_else(|| {
        LauncherError::InvalidInput(format!("у модпака {project_id} нет версий"))
    })?;
    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| {
            LauncherError::InvalidInput(format!(
                "у версии {} нет файлов",
                version.version_number
            ))
        })?;

    // F16: офлайн-режим — мгновенный честный отказ вместо сетевого таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode(file.url.clone()));
    }
    let resp = client
        .raw()
        .get(&file.url)
        .send()
        .await?
        .error_for_status()?;
    let bytes = resp.bytes().await?;
    if let Some(sha) = file.hashes.get("sha1") {
        let actual = crate::util::fs::sha1_bytes(&bytes);
        if !actual.eq_ignore_ascii_case(sha) {
            return Err(LauncherError::InvalidInput(format!(
                "sha1 не сошёлся для {}: ждали {sha}, получили {actual}",
                file.filename
            )));
        }
    }

    let tmp_dir = std::env::temp_dir().join("mc-launcher-v2");
    std::fs::create_dir_all(crate::util::fs::long_path(&tmp_dir))?;
    let tmp_path = crate::util::fs::long_path(&tmp_dir.join(&file.filename));
    std::fs::write(&tmp_path, &bytes)?;

    let result =
        install_mrpack(paths, settings, engine, client, group, &tmp_path, name_override).await;
    let _ = std::fs::remove_file(&tmp_path);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventBus, LauncherEvent};
    use crate::settings::Settings;
    use std::io::Write as _;

    /// Тело «мода» тестового модпака (sha1 считается по этим байтам).
    const MOD_BYTES: &[u8] = b"mrpack test payload";

    /// Поднять mock-сервер, вернуть базовый URL (как в tests/download_engine.rs):
    /// сети нет — только localhost.
    async fn spawn_axum(app: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn test_paths(dir: &tempfile::TempDir) -> Paths {
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        paths
    }

    /// Минимально валидный .mrpack: zip с modrinth.index.json.
    fn write_mrpack(path: &Path, index_json: &str) {
        write_mrpack_entries(path, index_json, &[]);
    }

    /// .mrpack с произвольными записями (для проверок overrides).
    fn write_mrpack_entries(path: &Path, index_json: &str, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::SimpleFileOptions = Default::default();
        zip.start_file("modrinth.index.json", opts).unwrap();
        zip.write_all(index_json.as_bytes()).unwrap();
        for (name, data) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    /// Индекс без файлов (нужен там, где проверяется только шаги 5–6).
    fn index_without_files(name: &str) -> String {
        format!(
            r#"{{"formatVersion":1,"game":"minecraft","versionId":"test-1",
                 "name":"{name}","files":[],
                 "dependencies":{{"minecraft":"1.21.1"}}}}"#
        )
    }

    fn index_with_mod(url: &str, sha1: &str) -> String {
        format!(
            r#"{{"formatVersion":1,"game":"minecraft","versionId":"test-1",
                 "name":"Тест-пак",
                 "files":[{{"path":"mods/x.jar","hashes":{{"sha1":"{sha1}"}},
                            "downloads":["{url}"],"fileSize":{}}}],
                 "dependencies":{{"minecraft":"1.21.1"}}}}"#,
            MOD_BYTES.len()
        )
    }

    /// Каталоги в `instances/` — «пустой рабочий инстанс» виден именно здесь.
    fn instance_dirs(paths: &Paths) -> Vec<String> {
        let mut dirs: Vec<String> =
            std::fs::read_dir(crate::util::fs::long_path(&paths.instances_dir()))
                .map(|it| {
                    it.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
        dirs.sort();
        dirs
    }

    /// Битый .mrpack: инстанса не появляется вовсе.
    #[tokio::test]
    async fn broken_mrpack_leaves_no_instance_dir() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let broken = dir.path().join("broken.mrpack");
        std::fs::write(&broken, "это не zip".as_bytes()).unwrap();

        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let err = install_mrpack(
            &paths,
            &Settings::default(),
            engine,
            client,
            "mrpack:test",
            &broken,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, LauncherError::Zip(_)), "{err}");
        assert!(
            instance_dirs(&paths).is_empty(),
            "список инстансов должен остаться пустым"
        );
    }

    /// Сбой загрузки файлов модпака: каталог несостоявшегося инстанса удаляется
    /// целиком (D12) — «пустого рабочего» инстанса в списке быть не должно.
    #[tokio::test]
    async fn failed_download_rolls_back_instance_dir() {
        // 404 на любой путь: задача падает сразу, без повторов.
        let app = axum::Router::new().fallback(|| async { axum::http::StatusCode::NOT_FOUND });
        let base = spawn_axum(app).await;

        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let pack = dir.path().join("pack.mrpack");
        write_mrpack(
            &pack,
            &index_with_mod(
                &format!("{base}/mods/x.jar"),
                "0000000000000000000000000000000000000000",
            ),
        );

        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let err = install_mrpack(
            &paths,
            &Settings::default(),
            engine,
            client,
            "mrpack:test",
            &pack,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, LauncherError::Network(_)), "{err}");
        assert!(
            instance_dirs(&paths).is_empty(),
            "после сбоя загрузки каталога инстанса быть не должно"
        );
    }

    /// Прогресс уходит в шину вызывающего (D11), а инстанс сохраняется только
    /// после успешной загрузки (D12). Ожидание — по событиям, а не по sleep (A42):
    /// сервер отдаёт тело только после того, как тест увидел первый dl_progress.
    #[tokio::test]
    async fn progress_goes_to_caller_bus_and_instance_saved_after_download() {
        let gate = Arc::new(tokio::sync::Notify::new());
        let gate_srv = gate.clone();
        let app = axum::Router::new().route(
            "/mods/x.jar",
            axum::routing::get(move || {
                let gate = gate_srv.clone();
                async move {
                    // Тикер движка шлёт dl_progress раз в 100 мс, но на локальном
                    // диске загрузка успевает завершиться раньше — поэтому держим
                    // ответ до сигнала теста (таймаут — предохранитель).
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        gate.notified(),
                    )
                    .await;
                    axum::body::Body::from(MOD_BYTES.to_vec())
                }
            }),
        );
        let base = spawn_axum(app).await;

        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let pack = dir.path().join("pack.mrpack");
        write_mrpack(
            &pack,
            &index_with_mod(
                &format!("{base}/mods/x.jar"),
                &crate::util::fs::sha1_bytes(MOD_BYTES),
            ),
        );

        let bus = EventBus::new(64);
        let mut rx = bus.subscribe();
        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, bus);
        let paths_task = paths.clone();
        let pack_task = pack.clone();
        let install = tokio::spawn(async move {
            install_mrpack(
                &paths_task,
                &Settings::default(),
                engine,
                client,
                "mrpack:test",
                &pack_task,
                None,
            )
            .await
        });

        // Ждём первый dl_progress: до сигнала загрузка висит, поэтому условие
        // достижимо (не зависит от скорости диска и загрузки машины).
        let mut progress = 0;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while progress == 0 && std::time::Instant::now() < deadline {
            match rx.try_recv() {
                Ok(LauncherEvent::DlProgress(_)) => progress += 1,
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(_) => break,
            }
        }
        gate.notify_one(); // отпускаем загрузку

        let inst = install.await.unwrap().unwrap();
        assert!(progress > 0, "dl_progress должен идти в шину вызывающего");
        assert_eq!(
            crate::instances::list(&paths).len(),
            1,
            "инстанс сохраняется после успешной загрузки"
        );
        let game =
            crate::instances::minecraft_dir(&crate::instances::instance_dir(&paths, &inst.id));
        assert!(game.join("mods").join("x.jar").exists(), "файл мода на месте");
    }

    /// A24: URL из чужого .mrpack проходит проверку хоста (SSRF).
    #[test]
    fn mrpack_download_hosts_are_allowlisted() {
        for url in [
            "http://169.254.169.254/latest/meta-data/", // метаданные облака
            "https://169.254.169.254/latest/meta-data/",
            "https://evil.example/mod.jar",
            "http://cdn.modrinth.com/data/x/y.jar", // только https
            "file:///C:/Windows/System32/calc.exe",
            "https://user:pw@evil.example/x.jar", // userinfo не обманывает allowlist
            "не-url",
        ] {
            let err = ensure_allowed_mrpack_url(url).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "{url}: {err}");
        }
        for url in [
            "https://api.modrinth.com/v2/version_file/x",
            "https://CDN.modrinth.com/data/x/y.jar", // регистр не важен
            "https://github.com/a/b/releases/download/v1/m.jar",
            "https://objects.githubusercontent.com/github-production/x",
            "https://raw.githubusercontent.com/a/b/main/x.jar",
        ] {
            ensure_allowed_mrpack_url(url).unwrap_or_else(|e| panic!("{url}: {e}"));
        }
    }

    /// A24: модпак со ссылкой на внутренний хост отвергается ДО сети и до
    /// создания каталогов инстанса.
    #[tokio::test]
    async fn mrpack_with_ssrf_url_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let pack = dir.path().join("ssrf.mrpack");
        write_mrpack(
            &pack,
            &index_with_mod(
                "http://169.254.169.254/latest/meta-data/iam/security-credentials/",
                "0000000000000000000000000000000000000000",
            ),
        );

        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let err = install_mrpack(
            &paths,
            &Settings::default(),
            engine,
            client,
            "mrpack:test",
            &pack,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), "invalid_input", "{err}");
        assert!(
            instance_dirs(&paths).is_empty(),
            "недоверенный модпак не должен создавать инстанс"
        );
    }

    /// A29: сбой overrides (запись с `..` внутри overrides/) уже ПОСЛЕ сохранения
    /// инстанса — каталог обязан быть откачен, а исходная ошибка — видна наружу.
    #[tokio::test]
    async fn overrides_failure_rolls_back_instance_dir() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let pack = dir.path().join("broken-overrides.mrpack");
        write_mrpack_entries(
            &pack,
            &index_without_files("Битые overrides"),
            &[("overrides/../evil.txt", b"X")],
        );

        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let err = install_mrpack(
            &paths,
            &Settings::default(),
            engine,
            client,
            "mrpack:test",
            &pack,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), "zip_slip", "{err}");
        assert!(
            instance_dirs(&paths).is_empty(),
            "инстанс не должен остаться после сбоя overrides"
        );
    }
}
