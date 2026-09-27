//! Тонкие #[tauri::command] над доменами (спека §4.2, §4.3). Никакого бизнеса
//! здесь: только вызовы доменов + AppState.

use crate::errors::{LauncherError, Result};
use crate::events::{EventBus, LauncherEvent};
use crate::instances::content::ArchiveResult;
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
    pub http: RwLock<Arc<HttpClient>>,
    pub launching: Arc<tokio::sync::Mutex<std::collections::HashSet<String>>>,
}

impl AppState {
    pub fn new(paths: Paths, bus: EventBus, settings: Settings) -> Result<Self> {
        paths.ensure_dirs()?;
        let http = match HttpClient::new(settings.proxy_url.as_deref()) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                tracing::warn!("Не удалось инициализировать прокси: {e}, запуск напрямую");
                Arc::new(HttpClient::new(None)?)
            }
        };
        Ok(Self {
            paths,
            bus,
            settings: RwLock::new(settings),
            http: RwLock::new(http),
            launching: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
        })
    }

    pub async fn http(&self) -> Arc<HttpClient> {
        self.http.read().await.clone()
    }

    pub async fn engine(&self) -> Arc<DownloadEngine> {
        let parallelism = {
            self.settings.read().await.download_parallelism as usize
        };
        let http = self.http().await;
        DownloadEngine::new(http, parallelism, self.bus.clone())
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
    let http = state.http().await;
    let manifest = fetch_manifest(&http, &state.paths.manifests_cache()).await?;
    resolve_entry(&manifest, &mc_version)?;
    let clean_name = crate::util::names::sanitize_user_name(&name, &mc_version);
    let inst = Instance::new(&clean_name, &mc_version);
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
    let inst_dir = instances::instance_dir(&state.paths, &id);
    if instances::running_pid(&inst_dir).is_some() {
        return Err(LauncherError::InvalidInput("инстанс уже запущен".into()));
    }
    {
        let mut launching = state.launching.lock().await;
        if launching.contains(&id) {
            return Err(LauncherError::InvalidInput("инстанс уже запускается".into()));
        }
        launching.insert(id.clone());
    }

    let inst_id_clone = id.clone();
    let res = async {
        let inst = instances::load(&state.paths, &id)?;
        let engine = state.engine().await;
        let settings_guard = state.settings.read().await.clone();
        let http = state.http().await;
        // Идентичность активного аккаунта (offline/MSA/ely) — спека §6.8.
        let identity = match crate::auth::launch_identity(&state.paths, &http).await {
            Ok(id) => id,
            Err(e) => {
                // Обновление токена не удалось (нет сети и пр.) — событие в UI + fallback.
                state.bus.emit(crate::events::LauncherEvent::AccountRefreshFailed {
                    account_id: crate::auth::active(&state.paths)
                        .map(|a| a.id)
                        .unwrap_or_default(),
                    reason: e.to_string(),
                });
                crate::auth::offline::identity(&player)
            }
        };
        // ely.by: скачать authlib-injector и подготовить -javaagent.
        let javaagent = match crate::auth::active(&state.paths).map(|a| a.kind) {
            Some(crate::auth::AccountKind::Ely) => {
                let jar =
                    crate::auth::ely::authlib_injector_jar(&http, &state.paths.cache_dir()).await?;
                Some(crate::auth::ely::javaagent_arg(&jar))
            }
            _ => None,
        };
        let prepared = instances::run::prepare(
            &state.paths,
            &settings_guard,
            http,
            engine,
            &state.bus,
            &inst,
            instances::run::AccountIdentity {
                player_name: identity.player_name,
                uuid: identity.uuid,
                access_token: identity.access_token,
                user_type: identity.user_type,
                javaagent,
            },
        )
        .await?;

        Ok::<_, LauncherError>((inst, prepared))
    }
    .await;

    // Проверяем, не был ли запуск отменён пользователем во время prepare
    let cancelled = {
        let launching = state.launching.lock().await;
        !launching.contains(&inst_id_clone)
    };

    if cancelled {
        tracing::info!("Запуск инстанса {inst_id_clone} был отменён до спавна процесса");
        state.bus.emit(crate::events::LauncherEvent::LaunchState {
            instance_id: inst_id_clone,
            phase: crate::events::LaunchPhase::Exited,
            exit_code: None,
        });
        return Ok(OkMsg { ok: true });
    }

    let (inst, prepared) = match res {
        Ok(pair) => pair,
        Err(e) => {
            state.launching.lock().await.remove(&inst_id_clone);
            state.bus.emit(crate::events::LauncherEvent::LaunchState {
                instance_id: inst_id_clone,
                phase: crate::events::LaunchPhase::Exited,
                exit_code: None,
            });
            return Err(e);
        }
    };

    // Спавн игры: дочерний процесс спавнится сразу с записью PID в .lock,
    // супервизия читает логи в фоне. Снимаем launching только ПОСЛЕ спавна!
    let instance_id = inst.id.clone();
    let spawn_res = instances::run::launch_detached(&state.paths, &instance_id, prepared, &state.bus);
    state.launching.lock().await.remove(&inst_id_clone);
    spawn_res?;
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
    let http = state.http().await;
    crate::modrinth::api::search(
        &http,
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
    let engine = state.engine().await;
    let http = state.http().await;
    crate::modrinth::install::install_project(
        &state.paths,
        http,
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

/// Вкл/выкл контента: `.disabled`-суффикс на диске + флаг в манифесте.
#[tauri::command]
pub async fn content_toggle(
    state: State<'_, AppState>,
    instance_id: String,
    file: String,
) -> Result<crate::instances::ContentEntry> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        instances::content::toggle(&paths, &instance_id, &file)
    })
    .await
    .map_err(join_err)?
}

/// Удалить контент инстанса (файл + запись манифеста).
#[tauri::command]
pub async fn content_remove(
    state: State<'_, AppState>,
    instance_id: String,
    file: String,
) -> Result<()> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        instances::content::remove(&paths, &instance_id, &file)
    })
    .await
    .map_err(join_err)?
}

/// Бэкап инстанса целиком в zip (каталог backups/ в данных лаунчера).
#[tauri::command]
pub async fn instance_backup(state: State<'_, AppState>, id: String) -> Result<ArchiveResult> {
    let paths = state.paths.clone();
    let inst = instances::load(&state.paths, &id)?;
    tauri::async_runtime::spawn_blocking(move || {
        instances::content::backup(&paths, &inst)
    })
    .await
    .map_err(join_err)?
}

/// Экспорт инстанса в .mrpack (формат Modrinth) в каталог exports/.
#[tauri::command]
pub async fn instance_export(state: State<'_, AppState>, id: String) -> Result<ArchiveResult> {
    let paths = state.paths.clone();
    let inst = instances::load(&state.paths, &id)?;
    tauri::async_runtime::spawn_blocking(move || {
        instances::content::export_mrpack(&paths, &inst)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn content_update_check(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<crate::modrinth::updates::UpdateCheck>> {
    let inst = instances::load(&state.paths, &instance_id)?;
    let http = state.http().await;
    crate::modrinth::updates::check_updates(&state.paths, &http, &inst).await
}

#[tauri::command]
pub async fn content_update_all(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<usize> {
    let inst = instances::load(&state.paths, &instance_id)?;
    let engine = state.engine().await;
    let http = state.http().await;
    crate::modrinth::updates::update_all(&state.paths, http, engine, &inst)
        .await
}


fn join_err(e: tauri::Error) -> LauncherError {
    LauncherError::Io(std::io::Error::other(format!("фоновая задача: {e}")))
}


/// Установка .mrpack в новый инстанс (путь выбирается диалогом/drag&drop).
#[tauri::command]
pub async fn mrpack_install(
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Result<Instance> {
    let http = state.http().await;
    crate::modrinth::mrpack::install_mrpack(
        &state.paths,
        &state.settings.read().await.clone(),
        http,
        std::path::Path::new(&path),
        name.as_deref(),
    )
    .await
}

/// Установка модпака Modrinth по project_id: последняя версия → .mrpack →
/// тот же установщик (Главная → «Популярные модпаки»).
#[tauri::command]
pub async fn modpack_install(
    state: State<'_, AppState>,
    project_id: String,
    name: Option<String>,
) -> Result<Instance> {
    let http = state.http().await;
    crate::modrinth::mrpack::install_modpack_project(
        &state.paths,
        &state.settings.read().await.clone(),
        http,
        &project_id,
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
    let engine = state.engine().await;
    let http = state.http().await;
    crate::optimize::optimize(&state.paths, http, engine, &inst).await
}

#[tauri::command]
pub async fn ram_guide() -> Result<crate::optimize::RamGuide> {
    Ok(crate::optimize::ram_guide())
}

#[tauri::command]
pub async fn instance_kill(state: State<'_, AppState>, instance_id: String) -> Result<OkMsg> {
    let was_launching = {
        let mut launching = state.launching.lock().await;
        launching.remove(&instance_id)
    };
    if was_launching {
        let engine = state.engine().await;
        engine.cancel_group(&format!("instance:{instance_id}"));
        state.bus.emit(crate::events::LauncherEvent::LaunchState {
            instance_id: instance_id.clone(),
            phase: crate::events::LaunchPhase::Exited,
            exit_code: None,
        });
        // Если процесс уже успел запуститься — гарантированно гасим его тоже
        let inst_dir = instances::instance_dir(&state.paths, &instance_id);
        if instances::running_pid(&inst_dir).is_some() {
            let _ = instances::kill(&inst_dir);
        }
        return Ok(OkMsg { ok: true });
    }

    let inst_dir = instances::instance_dir(&state.paths, &instance_id);
    if instances::running_pid(&inst_dir).is_some() {
        instances::kill(&inst_dir)?;
    } else {
        // Если PID уже неактивен, гарантируем очистку .lock и отправку события Exited в UI
        let _ = std::fs::remove_file(crate::util::fs::long_path(&inst_dir.join(".lock")));
        state.bus.emit(crate::events::LauncherEvent::LaunchState {
            instance_id,
            phase: crate::events::LaunchPhase::Exited,
            exit_code: None,
        });
    }
    Ok(OkMsg { ok: true })
}

#[tauri::command]
pub async fn process_status(state: State<'_, AppState>) -> Result<Vec<(String, u32)>> {
    Ok(instances::running_instances(&state.paths))
}

// ---------- аккаунты (спека §6.8) ----------

#[tauri::command]
pub async fn account_list(state: State<'_, AppState>) -> Result<Vec<crate::auth::Account>> {
    Ok(crate::auth::list(&state.paths))
}

#[tauri::command]
pub async fn account_add_offline(
    state: State<'_, AppState>,
    name: String,
) -> Result<crate::auth::Account> {
    let uuid = crate::auth::offline::offline_uuid(&name).to_string();
    let acc = crate::auth::Account {
        id: uuid::Uuid::new_v4().to_string(),
        kind: crate::auth::AccountKind::Offline,
        name: crate::util::names::sanitize_user_name(&name, "Player"),
        uuid,
        refresh_ref: None,
    };
    let added = crate::auth::add(&state.paths, acc)?;
    let _ = crate::auth::set_active(&state.paths, &added.id);
    Ok(added)
}

/// Вход Microsoft через системный браузер (auth-code flow, спека §6.8).
#[tauri::command]
pub async fn account_add_msa_browser(
    state: State<'_, AppState>,
) -> Result<crate::auth::Account> {
    let client_id = state
        .settings
        .read()
        .await
        .azure_client_id
        .clone()
        .ok_or_else(|| {
            crate::errors::LauncherError::InvalidInput(
                "Укажите azureClientId в Настройках (инструкция в README)".into(),
            )
        })?;

    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| LauncherError::network(format!("локальный порт для OAuth: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| LauncherError::network(format!("local_addr: {e}")))?
        .port();

    let auth_url = crate::auth::msa::authorize_url(&client_id, port);
    opener::open(&auth_url)
        .map_err(|e| LauncherError::Internal(format!("не удалось открыть браузер: {e}")))?;

    let code = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        crate::auth::msa::wait_auth_code(listener),
    )
    .await
    .map_err(|_| LauncherError::network("таймаут ожидания входа в браузере (5 мин)"))??;

    let http = state.http().await;
    let session = crate::auth::msa::finish_web_login(&http, &client_id, &code, port).await?;
    let ref_name = format!("msa-refresh-{}", uuid::Uuid::new_v4());
    crate::auth::keyring_set(&ref_name, &session.refresh_token)?;
    let acc = crate::auth::Account {
        id: uuid::Uuid::new_v4().to_string(),
        kind: crate::auth::AccountKind::Msa,
        name: session.player_name.clone(),
        uuid: session.uuid.clone(),
        refresh_ref: Some(ref_name),
    };
    crate::auth::add(&state.paths, acc.clone())?;
    let _ = crate::auth::set_active(&state.paths, &acc.id);
    Ok(acc)
}

/// Начало MSA device-code: UI показывает user_code и открывает браузер.
#[tauri::command]
pub async fn account_add_msa_start(
    state: State<'_, AppState>,
) -> Result<crate::auth::msa::DeviceCodeStart> {
    let client_id = state
        .settings
        .read()
        .await
        .azure_client_id
        .clone()
        .ok_or_else(|| {
            crate::errors::LauncherError::InvalidInput(
                "Укажите azureClientId в Настройках (инструкция в README)".into(),
            )
        })?;
    let http = state.http().await;
    crate::auth::msa::device_code_start(&http, &client_id).await
}

/// Опрос: Ok(Some) — вход завершён, аккаунт добавлен (refresh → keyring).
#[tauri::command]
pub async fn account_add_msa_poll(
    state: State<'_, AppState>,
    device_code: String,
) -> Result<Option<crate::auth::Account>> {
    let client_id = state
        .settings
        .read()
        .await
        .azure_client_id
        .clone()
        .ok_or_else(|| {
            crate::errors::LauncherError::InvalidInput("нет azureClientId в настройках".into())
        })?;
    let http = state.http().await;
    let Some(session) = crate::auth::msa::device_code_poll(&http, &client_id, &device_code).await? else {
        return Ok(None);
    };
    let ref_name = format!("msa-refresh-{}", uuid::Uuid::new_v4());
    crate::auth::keyring_set(&ref_name, &session.refresh_token)?;
    let acc = crate::auth::Account {
        id: uuid::Uuid::new_v4().to_string(),
        kind: crate::auth::AccountKind::Msa,
        name: session.player_name.clone(),
        uuid: session.uuid.clone(),
        refresh_ref: Some(ref_name),
    };
    crate::auth::add(&state.paths, acc.clone())?;
    let _ = crate::auth::set_active(&state.paths, &acc.id);
    Ok(Some(acc))
}

/// Логин ely.by (пароль передаётся только по TLS в authserver; в keyring — токен).
#[tauri::command]
pub async fn account_add_ely(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> Result<crate::auth::Account> {
    let http = state.http().await;
    let session = crate::auth::ely::authenticate(&http, &username, &password).await?;
    let ref_name = format!("ely-refresh-{}", uuid::Uuid::new_v4());
    crate::auth::keyring_set(&ref_name, &session.refresh_token)?;
    let acc = crate::auth::Account {
        id: uuid::Uuid::new_v4().to_string(),
        kind: crate::auth::AccountKind::Ely,
        name: session.player_name.clone(),
        uuid: session.uuid.clone(),
        refresh_ref: Some(ref_name),
    };
    crate::auth::add(&state.paths, acc.clone())?;
    let _ = crate::auth::set_active(&state.paths, &acc.id);
    Ok(acc)
}

#[tauri::command]
pub async fn account_remove(state: State<'_, AppState>, id: String) -> Result<OkMsg> {
    crate::auth::remove(&state.paths, &id)?;
    Ok(OkMsg { ok: true })
}

#[tauri::command]
pub async fn account_active_set(state: State<'_, AppState>, id: String) -> Result<OkMsg> {
    crate::auth::set_active(&state.paths, &id)?;
    Ok(OkMsg { ok: true })
}

// ---------- импорт и диагностика (спека §6.11, §6.10) ----------

#[tauri::command]
pub async fn instance_import(
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Result<Instance> {
    let http = state.http().await;
    let (mut inst, loader) = crate::import::import_archive(
        &state.paths,
        &state.settings.read().await.clone(),
        http.clone(),
        std::path::Path::new(&path),
        name.as_deref(),
    )
    .await?;
    // Распознанный загрузчик ставим сразу.
    if let Some((kind, ver)) = loader {
        inst = instances::run::install_loader(
            &state.paths,
            &state.settings.read().await.clone(),
            http,
            &inst,
            &kind,
            Some(&ver),
        )
        .await?;
    }
    Ok(inst)
}

/// Краш-анализ текста лога (расширяемые правила, спека §6.10).
#[tauri::command]
pub async fn crash_analyze(log_text: String) -> Result<Vec<crate::process::crash::Diagnosis>> {
    Ok(crate::process::crash::analyze(&log_text))
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
    let http = state.http().await;
    let manifest = fetch_manifest(&http, &state.paths.manifests_cache()).await?;
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
    let http = state.http().await;
    match loader.as_str() {
        "fabric" => Ok(crate::loaders::fabric::loader_versions(&http)
            .await?
            .into_iter()
            .map(|l| LoaderVersionEntry { version: l.version, stable: l.stable })
            .collect()),
        "quilt" => Ok(crate::loaders::quilt::loader_versions(&http)
            .await?
            .into_iter()
            .map(|l| LoaderVersionEntry { version: l.version, stable: l.stable })
            .collect()),
        "neoforge" => {
            // Список длинный и плоский — отдаём последние 30 стабильных.
            let all = crate::loaders::neoforge::versions(&http).await?;
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
    let http = state.http().await;
    instances::run::install_loader(
        &state.paths,
        &settings_guard,
        http,
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
    let http = state.http().await;
    let exe = crate::java::adoptium::install_jre(&http, &state.paths.runtime_dir(), major, |_, _| {})
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
    let old_proxy = state.settings.read().await.proxy_url.clone();
    if settings.proxy_url != old_proxy {
        match HttpClient::new(settings.proxy_url.as_deref()) {
            Ok(new_http) => {
                *state.http.write().await = Arc::new(new_http);
                tracing::info!("HTTP-клиент обновлен с новым proxy_url");
            }
            Err(e) => {
                tracing::warn!("Некорректный proxy_url: {e}");
            }
        }
    }
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
