//! Тонкие #[tauri::command] над доменами (спека §4.2, §4.3). Никакого бизнеса
//! здесь: только вызовы доменов + AppState.

use crate::errors::{LauncherError, Result};
use crate::events::{EventBus, LauncherEvent};
use crate::instances::{self, Instance};
use crate::java::detect::JavaInstall;
use crate::mojang::manifest::{filter_versions, fetch_manifest, resolve_entry};
use crate::net::download::DownloadEngine;
use crate::net::http::HttpClient;
use crate::paths::Paths;
use crate::settings::Settings;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;

/// Общее состояние приложения для команд.
pub struct AppState {
    pub paths: Paths,
    pub bus: EventBus,
    pub settings: RwLock<Settings>,
    pub http: Arc<HttpClient>,
}

impl AppState {
    pub fn new(paths: Paths, bus: EventBus, settings: Settings) -> Result<Self> {
        paths.ensure_dirs()?;
        let http = Arc::new(HttpClient::new(settings.proxy_url.as_deref())?);
        Ok(Self {
            paths,
            bus,
            settings: RwLock::new(settings),
            http,
        })
    }

    fn engine(&self) -> Arc<DownloadEngine> {
        let parallelism = {
            // блокирующее чтение RwLock допустимо: поле маленькое
            self.settings.try_read().map(|s| s.download_parallelism as usize).unwrap_or(16)
        };
        DownloadEngine::new(self.http.clone(), parallelism, self.bus.clone())
    }
}

#[derive(Serialize)]
pub struct OkMsg {
    pub ok: bool,
}

// ---------- инстансы ----------

#[tauri::command]
pub async fn instance_list(state: State<'_, AppState>) -> Result<Vec<Instance>> {
    Ok(instances::list(&state.paths))
}

#[tauri::command]
pub async fn instance_create(
    state: State<'_, AppState>,
    mc_version: String,
    name: String,
) -> Result<Instance> {
    // Версия должна существовать (манифест кэшируется).
    let manifest = fetch_manifest(&state.http, &state.paths.manifests_cache()).await?;
    resolve_entry(&manifest, &mc_version)?;
    let inst = Instance::new(&name, &mc_version);
    instances::save(&state.paths, &inst)?;
    Ok(inst)
}

#[tauri::command]
pub async fn instance_rename(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<Instance> {
    instances::rename(&state.paths, &id, &name)
}

#[tauri::command]
pub async fn instance_duplicate(state: State<'_, AppState>, id: String) -> Result<Instance> {
    instances::duplicate(&state.paths, &id)
}

#[tauri::command]
pub async fn instance_delete(state: State<'_, AppState>, id: String, wipe: bool) -> Result<OkMsg> {
    instances::delete(&state.paths, &id, wipe)?;
    Ok(OkMsg { ok: true })
}

#[tauri::command]
pub async fn instance_open_dir(state: State<'_, AppState>, id: String) -> Result<OkMsg> {
    let dir = instances::instance_dir(&state.paths, &id);
    std::fs::create_dir_all(crate::util::fs::long_path(&dir))?;
    opener::open(dir).map_err(|e| LauncherError::Internal(format!("открытие папки: {e}")))?;
    Ok(OkMsg { ok: true })
}

#[tauri::command]
pub async fn instance_settings_get(
    state: State<'_, AppState>,
    id: String,
) -> Result<Instance> {
    instances::load(&state.paths, &id)
}

#[tauri::command]
pub async fn instance_settings_set(
    state: State<'_, AppState>,
    instance: Instance,
) -> Result<Instance> {
    instances::save(&state.paths, &instance)?;
    Ok(instance)
}

/// Запуск инстанса из UI: подготовка + спавн; супервизия живёт в фоне,
/// фазы и логи идут событиями. Возвращает сразу после спавна.
#[tauri::command]
pub async fn instance_launch(
    state: State<'_, AppState>,
    id: String,
    player: String,
) -> Result<OkMsg> {
    let inst = instances::load(&state.paths, &id)?;
    let engine = state.engine();
    let settings_guard = state.settings.read().await.clone();
    let prepared = instances::run::prepare(
        &state.paths,
        &settings_guard,
        state.http.clone(),
        engine,
        &state.bus,
        &inst,
        instances::run::offline_identity(&player),
    )
    .await?;

    // Спавн + фоновая супервизия: логи → события, lock до выхода игры.
    let bus = state.bus.clone();
    let paths = state.paths.clone();
    let instance_id = inst.id.clone();
    std::thread::spawn(move || {
        let result = instances::run::launch_detached(&paths, &instance_id, prepared, &bus);
        if let Err(e) = result {
            tracing::error!("супервизия инстанса {instance_id}: {e}");
        }
    });
    Ok(OkMsg { ok: true })
}

// ---------- контент (Modrinth, спека §6.6) ----------

#[tauri::command]
pub async fn modrinth_search(
    state: State<'_, AppState>,
    query: String,
    mc_version: Option<String>,
    loader: Option<String>,
    project_type: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<crate::modrinth::api::SearchResult> {
    crate::modrinth::api::search(
        &state.http,
        &query,
        mc_version.as_deref(),
        loader.as_deref(),
        project_type.as_deref(),
        limit.unwrap_or(20),
        offset.unwrap_or(0),
    )
    .await
}

#[tauri::command]
pub async fn content_install(
    state: State<'_, AppState>,
    instance_id: String,
    project_id: String,
    version_id: Option<String>,
) -> Result<Vec<crate::instances::ContentEntry>> {
    let inst = instances::load(&state.paths, &instance_id)?;
    let engine = state.engine();
    crate::modrinth::install::install_project(
        &state.paths,
        state.http.clone(),
        engine,
        &inst,
        &project_id,
        version_id.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn content_installed(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<crate::instances::ContentEntry>> {
    Ok(crate::instances::load_content_manifest(&state.paths, &instance_id))
}

#[tauri::command]
pub async fn content_update_check(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<crate::modrinth::updates::UpdateCheck>> {
    let inst = instances::load(&state.paths, &instance_id)?;
    crate::modrinth::updates::check_updates(&state.paths, &state.http, &inst).await
}

#[tauri::command]
pub async fn content_update_all(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<usize> {
    let inst = instances::load(&state.paths, &instance_id)?;
    let engine = state.engine();
    crate::modrinth::updates::update_all(&state.paths, state.http.clone(), engine, &inst)
        .await
}

/// Установка .mrpack в новый инстанс (путь выбирается диалогом/drag&drop).
#[tauri::command]
pub async fn mrpack_install(
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Result<Instance> {
    crate::modrinth::mrpack::install_mrpack(
        &state.paths,
        &state.settings.read().await.clone(),
        state.http.clone(),
        std::path::Path::new(&path),
        name.as_deref(),
    )
    .await
}

// ---------- оптимизация и процессы (спека §6.9, §6.10) ----------

#[tauri::command]
pub async fn instance_optimize(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<String>> {
    let inst = instances::load(&state.paths, &instance_id)?;
    let engine = state.engine();
    crate::optimize::optimize(&state.paths, state.http.clone(), engine, &inst).await
}

#[tauri::command]
pub async fn ram_guide() -> Result<crate::optimize::RamGuide> {
    Ok(crate::optimize::ram_guide())
}

#[tauri::command]
pub async fn instance_kill(state: State<'_, AppState>, instance_id: String) -> Result<OkMsg> {
    instances::kill(&instances::instance_dir(&state.paths, &instance_id))?;
    Ok(OkMsg { ok: true })
}

#[tauri::command]
pub async fn process_status(state: State<'_, AppState>) -> Result<Vec<(String, u32)>> {
    Ok(instances::running_instances(&state.paths))
}

// ---------- версии и загрузчики ----------

#[derive(Serialize)]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub release_time: String,
}

#[tauri::command]
pub async fn manifest_versions(
    state: State<'_, AppState>,
    show_snapshots: bool,
    show_old: bool,
) -> Result<Vec<VersionEntry>> {
    let manifest = fetch_manifest(&state.http, &state.paths.manifests_cache()).await?;
    Ok(filter_versions(&manifest, show_snapshots, show_old)
        .into_iter()
        .map(|v| VersionEntry {
            id: v.id.clone(),
            version_type: v.version_type.clone(),
            release_time: v.release_time.clone(),
        })
        .collect())
}

#[derive(Serialize)]
pub struct LoaderVersionEntry {
    pub version: String,
    pub stable: bool,
}

#[tauri::command]
pub async fn loader_versions(
    state: State<'_, AppState>,
    loader: String,
) -> Result<Vec<LoaderVersionEntry>> {
    match loader.as_str() {
        "fabric" => Ok(crate::loaders::fabric::loader_versions(&state.http)
            .await?
            .into_iter()
            .map(|l| LoaderVersionEntry { version: l.version, stable: l.stable })
            .collect()),
        "quilt" => Ok(crate::loaders::quilt::loader_versions(&state.http)
            .await?
            .into_iter()
            .map(|l| LoaderVersionEntry { version: l.version, stable: l.stable })
            .collect()),
        "neoforge" => {
            // Список длинный и плоский — отдаём последние 30 стабильных.
            let all = crate::loaders::neoforge::versions(&state.http).await?;
            Ok(all
                .into_iter()
                .filter(|v| !v.ends_with("-beta"))
                .rev()
                .take(30)
                .map(|v| LoaderVersionEntry { version: v, stable: true })
                .collect())
        }
        other => Err(LauncherError::InvalidInput(format!(
            "неизвестный загрузчик {other}"
        ))),
    }
}

#[tauri::command]
pub async fn loader_install(
    state: State<'_, AppState>,
    id: String,
    loader: String,
    loader_version: Option<String>,
) -> Result<Instance> {
    let inst = instances::load(&state.paths, &id)?;
    let settings_guard = state.settings.read().await.clone();
    instances::run::install_loader(
        &state.paths,
        &settings_guard,
        state.http.clone(),
        &inst,
        &loader,
        loader_version.as_deref(),
    )
    .await
}

// ---------- java ----------

#[tauri::command]
pub async fn java_list(state: State<'_, AppState>) -> Result<Vec<JavaInstall>> {
    let extra = state.settings.read().await.extra_java_paths.clone();
    Ok(crate::java::detect::scan_all(&state.paths.runtime_dir(), &extra).await)
}

#[tauri::command]
pub async fn java_install(
    state: State<'_, AppState>,
    major: u32,
) -> Result<JavaInstall> {
    let exe = crate::java::adoptium::install_jre(&state.http, &state.paths.runtime_dir(), major, |_, _| {})
        .await?;
    crate::java::detect::inspect(&exe, "adoptium").await
}

// ---------- настройки ----------

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Settings> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn settings_set(state: State<'_, AppState>, settings: Settings) -> Result<OkMsg> {
    settings.save(&state.paths.settings_file())?;
    *state.settings.write().await = settings;
    Ok(OkMsg { ok: true })
}

// ---------- события: мост bus → окно ----------

/// Запустить пересылку событий ядра в окно (единый канал `core`).
pub fn spawn_event_bridge(app: AppHandle, bus: EventBus) {
    tauri::async_runtime::spawn(async move {
        let mut rx = bus.subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Err(e) = emit_event(&app, &event) {
                        tracing::debug!("emit события: {e}");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("мост событий отстал на {n}");
                }
                Err(_) => break,
            }
        }
    });
}

fn emit_event(app: &AppHandle, event: &LauncherEvent) -> tauri::Result<()> {
    let name = match event {
        LauncherEvent::DlProgress(_) => "dl_progress",
        LauncherEvent::DlQueueState(_) => "dl_queue_state",
        LauncherEvent::LaunchState { .. } => "launch_state",
        LauncherEvent::GameLogLine { .. } => "game_log_line",
        LauncherEvent::AccountRefreshFailed { .. } => "account_refresh_failed",
    };
    app.emit(name, event)
}

/// Каталог данных лаунчера (для онбординга).
#[tauri::command]
pub async fn data_dir(state: State<'_, AppState>) -> Result<String> {
    Ok(state.paths.root().to_string_lossy().into_owned())
}

/// Диагностика: хвост файла лога лаунчера.
#[tauri::command]
pub async fn logs_tail(state: State<'_, AppState>, lines: u32) -> Result<String> {
    let log = state.paths.logs_dir().join("launcher.log");
    if !log.exists() {
        return Ok(String::new());
    }
    let data = std::fs::read(crate::util::fs::long_path(&log))?;
    let text = String::from_utf8_lossy(&data);
    let tail: Vec<&str> = text.lines().skip(text.lines().count().saturating_sub(lines as usize)).collect();
    Ok(tail.join("\n"))
}

#[tauri::command]
pub async fn dir_open(state: State<'_, AppState>) -> Result<OkMsg> {
    opener::open(state.paths.root().clone())
        .map_err(|e| LauncherError::Internal(format!("открытие папки: {e}")))?;
    Ok(OkMsg { ok: true })
}

#[allow(unused_imports)]
use opener as _opener_reexport;
