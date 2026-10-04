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
    /// Хэндлер приложения (заполняется в setup): применение логотипа к окну
    /// (D38) без прокидки AppHandle через каждую команду.
    pub app: std::sync::OnceLock<tauri::AppHandle>,
    pub bus: EventBus,
    pub settings: RwLock<Settings>,
    pub http: RwLock<Arc<HttpClient>>,
    pub launching: Arc<tokio::sync::Mutex<std::collections::HashSet<String>>>,
    /// Активные движки загрузок по группе (`instance:<id>`): instance_kill
    /// отменяет РЕАЛЬНЫЙ движок группы, а не свежесозданный no-op (D1).
    pub engines: std::sync::Mutex<std::collections::HashMap<String, Arc<DownloadEngine>>>,
}

impl AppState {
    pub fn new(paths: Paths, bus: EventBus, settings: Settings) -> Result<Self> {
        paths.ensure_dirs()?;
        let app = std::sync::OnceLock::new();
        let http = match HttpClient::new(settings.proxy_url.as_deref()) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                tracing::warn!("Не удалось инициализировать прокси: {e}, запуск напрямую");
                Arc::new(HttpClient::new(None)?)
            }
        };
        // F16: сохранённый режим «Работать офлайн» применяется к клиенту
        // сразу при старте (settings читаются раньше создания http).
        http.set_offline(settings.work_offline);
        Ok(Self {
            app,
            paths,
            bus,
            settings: RwLock::new(settings),
            http: RwLock::new(http),
            launching: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
            engines: std::sync::Mutex::new(std::collections::HashMap::new()),
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

    /// Движок для группы с регистрацией в реестре. Работа с группой
    /// завершена → `engine_done(group)`, чтобы не держать старый HttpClient.
    pub async fn engine_for(&self, group: &str) -> Arc<DownloadEngine> {
        if let Some(e) = self.engines.lock().unwrap().get(group) {
            return e.clone();
        }
        // F17: лимит скорости живёт на HttpClient (общем) — применим текущие
        // настройки при создании движка (как set_offline в AppState::new).
        let limit = self.settings.read().await.speed_limit_kbps;
        self.http().await.set_speed_limit(limit);
        let e = self.engine().await;
        self.engines
            .lock()
            .unwrap()
            .insert(group.to_string(), e.clone());
        e
    }

    /// Отменить активную загрузку группы (если движок ещё жив).
    pub fn cancel_engine_group(&self, group: &str) {
        if let Some(e) = self.engines.lock().unwrap().remove(group) {
            e.cancel_group(group);
        }
    }

    /// Снять группу с регистрации (работа завершена — успех или ошибка).
    pub fn engine_done(&self, group: &str) {
        self.engines.lock().unwrap().remove(group);
    }
}

#[derive(Serialize)]
pub struct OkMsg {
    pub ok: bool,
}

// ---------- инстансы ----------

#[tauri::command]
pub async fn instance_list(state: State<'_, AppState>) -> Result<Vec<Instance>> {
    // Чтение всех instance.json — тяжёлый дисковый I/O: не блокируем
    // рабочий поток Tokio (A20/D14).
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || instances::list(&paths))
        .await
        .map_err(join_err)
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
    // Дисковые операции инстанса — в блокирующий пул (A27/D14).
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || instances::rename(&paths, &id, &name))
        .await
        .map_err(join_err)?
}

#[tauri::command]
pub async fn instance_duplicate(state: State<'_, AppState>, id: String) -> Result<Instance> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || instances::duplicate(&paths, &id))
        .await
        .map_err(join_err)?
}

#[tauri::command]
pub async fn instance_delete(state: State<'_, AppState>, id: String, wipe: bool) -> Result<OkMsg> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || instances::delete(&paths, &id, wipe))
        .await
        .map_err(join_err)??;
    Ok(OkMsg { ok: true })
}

#[tauri::command]
pub async fn instance_open_dir(state: State<'_, AppState>, id: String) -> Result<OkMsg> {
    // Без валидации id `..\..\Windows` открывал бы произвольный каталог/файл
    // через opener (A23).
    instances::valid_id(&id)?;
    // B10: несуществующий id не должен плодить фантомные папки — сначала
    // проверяем, что инстанс есть (`load` отдаёт NotFound).
    instances::load(&state.paths, &id)?;
    let dir = instances::instance_dir(&state.paths, &id);
    std::fs::create_dir_all(crate::util::fs::long_path(&dir))?;
    tauri_plugin_opener::open_path(dir, None::<&str>).map_err(|e| LauncherError::Internal(format!("открытие папки: {e}")))?;
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
    // IPC-граница: флаги JVM, аргументы, RAM и путь к Java приходят из рендера
    // и до старта игры не проверяются нигде — валидируем здесь (A3).
    instances::validate_instance_settings(&instance)?;
    let paths = state.paths.clone();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        // B6: снапшот фронтенда устаревает (супервизор успевает дописать
        // статистику между Get и Set) — сохраняем слиянием: с диска берём
        // актуальный инстанс и переносим из payload только пользовательские
        // поля. Заодно несуществующий id даёт NotFound, а не фантомный инстанс.
        instances::update(&paths, &instance.id, |disk| merge_user_settings(disk, &instance))?;
        instances::load(&paths, &instance.id)
    })
    .await
    .map_err(join_err)??;
    Ok(saved)
}

/// Поля настроек, редактируемые пользователем в InstanceSettingsModal (D19):
/// только они переносятся из payload в дисковый инстанс. Служебные
/// (schema_version, id, name, mc_version, loader*, version_id, created_at,
/// last_played, play_seconds, launch_count, icon) остаются с диска.
fn merge_user_settings(disk: &mut Instance, payload: &Instance) {
    disk.ram_mb = payload.ram_mb;
    disk.java_path = payload.java_path.clone();
    disk.jvm_flags = payload.jvm_flags.clone();
    disk.game_args_extra = payload.game_args_extra.clone();
    disk.notes = payload.notes.clone();
    // F1/F11: быстрый запуск и профиль запуска — тоже пользовательские поля.
    disk.quick_play_world = payload.quick_play_world.clone();
    disk.quick_play_server = payload.quick_play_server.clone();
    disk.account_id = payload.account_id.clone();
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
    let group = format!("instance:{id}");
    let res = async {
        let inst = instances::load(&state.paths, &id)?;
        let engine = state.engine_for(&group).await;
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
        // ely.by и свой authlib-сервер: authlib-injector + -javaagent.
        let active_acc = crate::auth::active(&state.paths);
        let javaagent = match (&active_acc, active_acc.as_ref().and_then(|a| a.authlib_server.clone())) {
            (Some(crate::auth::Account { kind: crate::auth::AccountKind::Ely, .. }), _) => {
                let jar =
                    crate::auth::ely::authlib_injector_jar(&http, &state.paths.cache_dir()).await?;
                Some(crate::auth::ely::javaagent_arg(&jar))
            }
            (
                Some(crate::auth::Account {
                    kind: crate::auth::AccountKind::Authlib,
                    ..
                }),
                Some(server),
            ) => {
                let jar =
                    crate::auth::ely::authlib_injector_jar(&http, &state.paths.cache_dir()).await?;
                Some(crate::auth::custom::build_agent_arg(&jar, &server))
            }
            (Some(crate::auth::Account { kind: crate::auth::AccountKind::Authlib, .. }), None) => {
                return Err(LauncherError::InvalidInput(
                    "у authlib-аккаунта нет адреса сервера".into(),
                ));
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

    // B4: prepare завершён (успех или ошибка) — снимаем регистрацию движка
    // группы, как в content_install, иначе ссылка на HttpClient висит в
    // реестре до конца сессии. После этой точки движок больше не нужен.
    state.engine_done(&group);

    // Проверка отмены и спавн под ОДНИМ захватом launching-мьютекса (B5):
    // если instance_kill сработает в этом окне, он подождёт мьютекс, а после
    // снятия увидит живой PID в .lock и убьёт процесс. Раньше проверка
    // отпускала мьютекс до спавна — kill в зазоре не находил ни launching,
    // ни PID, рапортовал Exited, и игра стартовала зомби.
    let (inst, prepared) = {
        let mut launching = state.launching.lock().await;
        if !launching.contains(&inst_id_clone) {
            tracing::info!("Запуск инстанса {inst_id_clone} был отменён до спавна процесса");
            state.bus.emit(crate::events::LauncherEvent::LaunchState {
                instance_id: inst_id_clone,
                phase: crate::events::LaunchPhase::Exited,
                exit_code: None,
            });
            return Ok(OkMsg { ok: true });
        }
        match res {
            Ok(pair) => pair,
            Err(e) => {
                launching.remove(&inst_id_clone);
                state.bus.emit(crate::events::LauncherEvent::LaunchState {
                    instance_id: inst_id_clone,
                    phase: crate::events::LaunchPhase::Exited,
                    exit_code: None,
                });
                return Err(e);
            }
        }
    };

    // Спавн игры: дочерний процесс спавнится сразу с записью PID в .lock,
    // супервизия читает логи в фоне. Снимаем launching только ПОСЛЕ спавна —
    // guard держится захваченным через launch_detached и удаление id (B5).
    let instance_id = inst.id.clone();
    let spawn_res = {
        let mut launching = state.launching.lock().await;
        // F26: имя инстанса для Discord Rich Presence, если тумблер включён.
        let discord = if state.settings.read().await.discord_rpc {
            Some(inst.name.clone())
        } else {
            None
        };
        let res = instances::run::launch_detached(
            &state.paths,
            &instance_id,
            prepared,
            &state.bus,
            discord,
        );
        launching.remove(&inst_id_clone);
        res
    };
    spawn_res?;
    Ok(OkMsg { ok: true })
}

// ---------- контент (Modrinth, спека §6.6) ----------

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn modrinth_search(
    state: State<'_, AppState>,
    query: String,
    mc_version: Option<String>,
    loader: Option<String>,
    project_type: Option<String>,
    index: Option<String>,
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
        index.as_deref(),
        limit.unwrap_or(20),
        offset.unwrap_or(0),
    )
    .await
}

/// Метаданные проектов Modrinth (название/описание/иконка) батчем — одним
/// запросом на список, чтобы таб «Моды» не ходил в сеть по каждой строке.
#[tauri::command]
pub async fn modrinth_projects(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<Vec<crate::modrinth::api::ProjectMeta>> {
    // IPC-граница: список приходит из рендера — валидируем (A1/A23-принцип).
    crate::modrinth::api::validate_project_ids(&ids)?;
    let http = state.http().await;
    crate::modrinth::api::projects_meta(&http, &ids).await
}

/// Страница проекта целиком (D39): тело в markdown, галерея, лицензия —
/// окно проекта в стиле CurseForge.
#[tauri::command]
pub async fn modrinth_project(
    state: State<'_, AppState>,
    id_or_slug: String,
) -> Result<crate::modrinth::api::ProjectDetail> {
    let http = state.http().await;
    crate::modrinth::api::project_detail(&http, &id_or_slug).await
}

/// Версии проекта (D39): табы «Версии» и «Журнал изменений» окна проекта.
#[tauri::command]
pub async fn modrinth_versions(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<crate::modrinth::api::VersionInfo>> {
    let http = state.http().await;
    // IPC-граница: id приходит из рендера — та же валидация, что у батча.
    crate::modrinth::api::validate_project_ids(std::slice::from_ref(&project_id))?;
    Ok(crate::modrinth::api::project_versions(&http, &project_id)
        .await?
        .into_iter()
        .map(crate::modrinth::api::VersionInfo::from)
        .collect())
}

#[tauri::command]
pub async fn content_install(
    state: State<'_, AppState>,
    instance_id: String,
    project_id: String,
    version_id: Option<String>,
) -> Result<Vec<crate::instances::ContentEntry>> {
    let inst = instances::load(&state.paths, &instance_id)?;
    // Суффикс операции: параллельные операции над инстансом не должны делить
    // ключ реестра движков — engine_done первой снимал бы регистрацию второй.
    let group = format!("instance:{instance_id}:content");
    let engine = state.engine_for(&group).await;
    let http = state.http().await;
    let out = crate::modrinth::install::install_project(
        &state.paths,
        http,
        engine,
        &inst,
        &project_id,
        version_id.as_deref(),
    )
    .await;
    state.engine_done(&group);
    out
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
    // B9: переименование файлов занятой игры = Sharing Violation на Windows.
    if instances::running_pid(&instances::instance_dir(&state.paths, &instance_id)).is_some() {
        return Err(LauncherError::InstanceRunning(instance_id.clone()));
    }
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
    // B9: удаление файлов занятой игры = Sharing Violation на Windows.
    if instances::running_pid(&instances::instance_dir(&state.paths, &instance_id)).is_some() {
        return Err(LauncherError::InstanceRunning(instance_id.clone()));
    }
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

/// Установить иконку инстанса из локального файла (PNG/JPEG/WebP ≤ 300 КБ).
#[tauri::command]
pub async fn instance_icon_set(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<Instance> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        instances::content::set_icon(&paths, &id, &path)
    })
    .await
    .map_err(join_err)?
}

/// Убрать иконку инстанса.
#[tauri::command]
pub async fn instance_icon_remove(state: State<'_, AppState>, id: String) -> Result<Instance> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || instances::content::clear_icon(&paths, &id))
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

/// Дозаполнить projectId в манифесте (legacy-модпаки) — батч по sha1.
#[tauri::command]
pub async fn content_backfill_projects(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<usize> {
    let inst = instances::load(&state.paths, &instance_id)?;
    let http = state.http().await;
    crate::modrinth::updates::backfill_project_ids(&state.paths, &http, &inst).await
}

#[tauri::command]
pub async fn content_update_all(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<usize> {
    let inst = instances::load(&state.paths, &instance_id)?;
    // Суффикс операции — см. content_install: свой ключ реестра движков.
    let group = format!("instance:{instance_id}:update");
    let engine = state.engine_for(&group).await;
    let http = state.http().await;
    // F8: перед пакетным обновлением — авто-снапшот mods/ (ротация 3).
    {
        let snap_paths = state.paths.clone();
        let snap_id = instance_id.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            crate::instances::snapshots::create(&snap_paths, &snap_id)
        })
        .await
        .map_err(join_err);
    }
    let out = crate::modrinth::updates::update_all(&state.paths, http, engine, &inst).await;
    state.engine_done(&group);
    out
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
    // Ключ реестра движков = группа задач установки: иначе «Стоп» не нашёл бы,
    // что отменять (D1). У .mrpack до парсинга манифеста нет id — берём имя файла.
    let pack_path = std::path::Path::new(&path);
    let group = format!(
        "mrpack:{}",
        pack_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    let engine = state.engine_for(&group).await;
    let http = state.http().await;
    let settings = state.settings.read().await.clone();
    let out = crate::modrinth::mrpack::install_mrpack(
        &state.paths,
        &settings,
        engine,
        http,
        &group,
        pack_path,
        name.as_deref(),
    )
    .await;
    // Группу снимаем в любом исходе (успех/ошибка) — как в content_install.
    state.engine_done(&group);
    out
}

/// Установка модпака Modrinth по project_id: последняя версия → .mrpack →
/// тот же установщик (Главная → «Популярные модпаки»).
#[tauri::command]
pub async fn modpack_install(
    state: State<'_, AppState>,
    project_id: String,
    name: Option<String>,
) -> Result<Instance> {
    let group = format!("mrpack:{project_id}");
    let engine = state.engine_for(&group).await;
    let http = state.http().await;
    let settings = state.settings.read().await.clone();
    let out = crate::modrinth::mrpack::install_modpack_project(
        &state.paths,
        &settings,
        engine,
        http,
        &group,
        &project_id,
        name.as_deref(),
    )
    .await;
    state.engine_done(&group);
    out
}

// ---------- оптимизация и процессы (спека §6.9, §6.10) ----------

#[tauri::command]
pub async fn instance_optimize(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<String>> {
    // B9: оптимизация трогает файлы (память, Java, опции) занятой игры —
    // на Windows это Sharing Violation; проверяем до загрузок.
    if instances::running_pid(&instances::instance_dir(&state.paths, &instance_id)).is_some() {
        return Err(LauncherError::InstanceRunning(instance_id.clone()));
    }
    let inst = instances::load(&state.paths, &instance_id)?;
    // Суффикс операции — см. content_install: свой ключ реестра движков.
    let group = format!("instance:{instance_id}:optimize");
    let engine = state.engine_for(&group).await;
    let http = state.http().await;
    let out = crate::optimize::optimize(&state.paths, http, engine, &inst).await;
    state.engine_done(&group);
    out
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
        state.cancel_engine_group(&format!("instance:{instance_id}"));
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

/// Снять зависшую блокировку инстанса (F27): `.lock` без живого процесса.
/// Идемпотентно: если процесса нет — снимаем и рапортуем Exited; нет и
/// `.lock` — просто Ok. Живую игру не трогаем (InstanceRunning).
#[tauri::command]
pub async fn instance_force_unlock(state: State<'_, AppState>, id: String) -> Result<OkMsg> {
    instances::valid_id(&id)?;
    let inst_dir = instances::instance_dir(&state.paths, &id);
    if instances::running_pid(&inst_dir).is_some() {
        return Err(LauncherError::InstanceRunning(id));
    }
    let lock = crate::util::fs::long_path(&inst_dir.join(".lock"));
    if lock.exists() {
        std::fs::remove_file(&lock)?;
        state.bus.emit(crate::events::LauncherEvent::LaunchState {
            instance_id: id,
            phase: crate::events::LaunchPhase::Exited,
            exit_code: None,
        });
    }
    Ok(OkMsg { ok: true })
}

#[tauri::command]
pub async fn process_status(state: State<'_, AppState>) -> Result<Vec<(String, u32)>> {
    // Сканирование всех instance.json + живость PID — не в async-потоке (A20).
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || instances::running_instances(&paths))
        .await
        .map_err(join_err)
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
    // ENV-13: uuid считается от санитизированного имени — как его увидит игра.
    // Раньше uuid брали от сырого ввода («Steve\r\n»), а в аккаунт клалось
    // чистое имя: офлайн-uuid не совпадал с md5("OfflinePlayer:Steve") и
    // рассинхронизировал инвентарь в офлайн-мирах.
    let clean = crate::util::names::sanitize_user_name(&name, "Player");
    let uuid = crate::auth::offline::offline_uuid(&clean).to_string();
    let acc = crate::auth::Account {
        id: uuid::Uuid::new_v4().to_string(),
        kind: crate::auth::AccountKind::Offline,
        name: clean,
        uuid,
        refresh_ref: None,
        authlib_server: None,
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

    // Асинхронный listener (A5): по таймауту future отменяется и сокет
    // закрывается вместе с ней — блокирующий accept в spawn_blocking раньше
    // оставался жить и держал порт до конца процесса.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| LauncherError::network(format!("локальный порт для OAuth: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| LauncherError::network(format!("local_addr: {e}")))?
        .port();

    // CSRF: случайный state в authorize-URL; листнер примет redirect только
    // с ним же (чужой код от другого процесса/вкладки — 400 и ожидание).
    let oauth_state = uuid::Uuid::new_v4().to_string();
    let auth_url = crate::auth::msa::authorize_url(&client_id, port, &oauth_state);
    tauri_plugin_opener::open_url(auth_url, None::<&str>)
        .map_err(|e| LauncherError::Internal(format!("не удалось открыть браузер: {e}")))?;

    let code = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        crate::auth::msa::wait_auth_code_async(listener, &oauth_state),
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
        authlib_server: None,
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
        authlib_server: None,
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
        authlib_server: None,
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

/// Скин профиля для показа в лаунчере (D34): ely.by / Mojang session server;
/// None — скина нет (offline, 404) — UI рисует дефолт. Дисковый кэш 1 час.
#[tauri::command]
pub async fn account_skin(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Option<crate::auth::skins::SkinInfo>> {
    let accounts = crate::auth::list(&state.paths);
    let account = accounts.into_iter().find(|a| a.id == account_id).ok_or_else(|| {
        crate::errors::LauncherError::NotFound(format!("аккаунт {account_id}"))
    })?;
    let http = state.http().await;
    crate::auth::skins::fetch_skin(&http, &state.paths, &account).await
}

// ---------- скины (D59) ----------

/// Библиотека дефолтных скинов из клиент-jar стора: Steve/Alex × classic/slim
/// (D54-механизм извлечения); jar в сторе нет — пустой список.
#[tauri::command]
pub async fn skin_library_list(
    state: State<'_, AppState>,
) -> Result<Vec<crate::auth::skins::SkinInfo>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Ok(crate::auth::skins::list_library_skins(&paths))
    })
    .await
    .map_err(join_err)?
}

/// Пользовательские скины (каталог skins/ в данных лаунчера).
#[tauri::command]
pub async fn skin_user_list(state: State<'_, AppState>) -> Result<Vec<crate::auth::skins::SkinInfo>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || Ok(crate::auth::skins::list_user_skins(&paths)))
        .await
        .map_err(join_err)?
}

/// Сохранить PNG как пользовательский скин. Файл выбирается нативным
/// диалогом на фронте (path), чтение — с ограничением домашним каталогом,
/// как у иконок (A16). Валидация: PNG 64×64/64×32, ≤ 50 КБ.
#[tauri::command]
pub async fn skin_user_save(
    state: State<'_, AppState>,
    name: String,
    path: String,
) -> Result<crate::auth::skins::SkinInfo> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let p = std::path::Path::new(&path);
        crate::instances::content::ensure_icon_source_allowed(p)?;
        let data = std::fs::read(crate::util::fs::long_path(p))
            .map_err(|_| LauncherError::NotFound(format!("файл скина: {path}")))?;
        crate::auth::skins::save_user_skin(&paths, &name, &data)
    })
    .await
    .map_err(join_err)?
}

/// Удалить пользовательский скин по id (id = имя файла каталога skins/).
#[tauri::command]
pub async fn skin_user_delete(state: State<'_, AppState>, skin_id: String) -> Result<OkMsg> {
    crate::auth::skins::delete_user_skin(&state.paths, &skin_id)?;
    Ok(OkMsg { ok: true })
}

/// Применить скин (по id из каталога пользователя или библиотеки) аккаунту:
/// msa → PUT Mojang profile/skins; ely → API ely.by; offline/authlib —
/// честный отказ (D59).
#[tauri::command]
pub async fn skin_apply(
    state: State<'_, AppState>,
    account_id: String,
    skin_id: String,
    model: crate::auth::skins::SkinModel,
) -> Result<OkMsg> {
    let skin = crate::auth::skins::find_skin_by_id(&state.paths, &skin_id)
        .ok_or_else(|| LauncherError::NotFound(format!("скин {skin_id}")))?;
    let http = state.http().await;
    crate::auth::skins::apply_skin(&state.paths, &http, &account_id, &skin, model).await?;
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

/// Версия в каталоге (`manifest_versions`): camelCase — фронт читает
/// `releaseTime` (A7), `type` задан явным rename.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
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
        "forge" => {
            // promotions_slim.json содержит только «продвинутые» версии
            // (recommended/latest) — все стабильные по определению. Это
            // словарь по версиям MC (ключи `1.20.1-recommended`) со многими
            // повторами значений: собираем уникальные и сортируем по сегментам
            // версии, отдаём последние 30 (как у neoforge).
            let promos = crate::loaders::forge::promotions(&http).await?;
            let mut versions: Vec<String> = promos.into_values().collect();
            versions.sort();
            versions.dedup();
            versions.sort_by_key(|v| {
                v.split('.')
                    .map(|p| p.parse::<u64>().unwrap_or(0))
                    .collect::<Vec<u64>>()
            });
            Ok(versions
                .into_iter()
                .rev()
                .take(30)
                .map(|version| LoaderVersionEntry { version, stable: true })
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

/// Рекомендуемая мажорная Java для версии MC (F14): тонкая обёртка над
/// таблицей `fallback_major` (instances/run.rs, спека §6.4):
/// ≤1.16 → 8, 1.17–1.20 → 17, 1.20+ (и пост-1.x) → 21.
#[tauri::command]
pub async fn java_recommended(mc_version: String) -> Result<u8> {
    // Таблица возвращает только 8/17/21 — u8 всегда достаточен.
    Ok(crate::instances::run::fallback_major(&mc_version) as u8)
}

// ---------- настройки ----------

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Settings> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn settings_set(state: State<'_, AppState>, settings: Settings) -> Result<OkMsg> {
    let mut merged = settings;
    // ENV-8: пустая строка/одни пробелы — легитимное «без прокси» (UI шлёт
    // undefined, но пустую строку мог сохранить старый билд settings.json);
    // нормализуем до None, чтобы не превращать «без прокси» в ошибку.
    let normalized = merged.proxy_url.take().filter(|s| !s.trim().is_empty());
    merged.proxy_url = normalized;
    let proxy = merged.proxy_url.clone();
    let old_proxy = state.settings.read().await.proxy_url.clone();
    if proxy != old_proxy {
        match HttpClient::new(proxy.as_deref()) {
            Ok(new_http) => {
                *state.http.write().await = Arc::new(new_http);
                tracing::info!("HTTP-клиент обновлен с новым proxy_url");
            }
            // ENV-8: раньше ошибка конструктора только логировалась, битый
            // proxy_url СОХРАНЯЛСЯ и все сетевые вызовы продолжали падать.
            // Теперь настройки не сохраняются — пользователь сразу видит ошибку.
            Err(e) => {
                return Err(LauncherError::InvalidInput(format!(
                    "некорректный proxy_url: {e}"
                )));
            }
        }
    }
    // accounts_active_id принадлежит auth-потоку (account_active_set и пр.),
    // а не странице настроек: её снимок мог устареть, и слепое сохранение
    // откатывало бы смену активного аккаунта — берём значение с диска.
    if let Ok(current) = Settings::load(&state.paths.settings_file()) {
        merged.accounts_active_id = current.accounts_active_id;
    }
    merged.save(&state.paths.settings_file())?;
    // F16/F17: офлайн-режим и лимит скорости применяются к актуальному
    // HTTP-клиенту (в т.ч. к созданному при смене прокси — swap уже был).
    let work_offline = merged.work_offline;
    let speed_limit = merged.speed_limit_kbps;
    *state.settings.write().await = merged;
    let http = state.http().await;
    http.set_offline(work_offline);
    http.set_speed_limit(speed_limit);
    Ok(OkMsg { ok: true })
}

// ---------- волна D37 (фаза 2 аудита паритета) ----------

/// Список миров инстанса (F24).
#[tauri::command]
pub async fn instance_worlds(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<crate::instances::worlds::WorldInfo>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::instances::worlds::list_worlds(&paths, &id))
        .await
        .map_err(join_err)?
}

/// Бэкап мира в zip (F24).
#[tauri::command]
pub async fn world_backup(
    state: State<'_, AppState>,
    id: String,
    world: String,
) -> Result<crate::instances::worlds::ArchiveResult> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::instances::worlds::backup_world(&paths, &id, &world)
    })
    .await
    .map_err(join_err)?
}

/// Удаление мира/Незера/Энда (F24).
#[tauri::command]
pub async fn world_delete_data(
    state: State<'_, AppState>,
    id: String,
    world: String,
    scope: crate::instances::worlds::WorldScope,
) -> Result<OkMsg> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::instances::worlds::delete_world_data(&paths, &id, &world, scope)
    })
    .await
    .map_err(join_err)??;
    Ok(OkMsg { ok: true })
}

/// Проверка целостности и починка игровых файлов (F2). Суффикс операции
/// в группе — не делит ключ реестра движков с запуском и контентными
/// операциями (см. content_install).
#[tauri::command]
pub async fn instance_repair(
    state: State<'_, AppState>,
    id: String,
) -> Result<crate::instances::repair::RepairReport> {
    instances::load(&state.paths, &id)?;
    let group = format!("instance:{id}:repair");
    let engine = state.engine_for(&group).await;
    let http = state.http().await;
    let paths = state.paths.clone();
    let out =
        crate::instances::repair::repair_instance(&paths, http, engine, &id).await;
    state.engine_done(&group);
    out
}

/// Запуск `java -version` для проверки пути (F15).
#[tauri::command]
pub async fn java_test_path(java_exe: String) -> Result<crate::java::detect::JavaTestInfo> {
    tauri::async_runtime::spawn_blocking(move || crate::java::detect::test_java(&java_exe))
        .await
        .map_err(join_err)?
}

/// Ответ mclo.gs-шеринга (зеркало client.ts logShareMclogs).
#[derive(Debug, serde::Serialize)]
pub struct ShareResult {
    pub url: String,
}

/// Отправить последний лог инстанса на mclo.gs (F22): redact → upload.
#[tauri::command]
pub async fn log_share_mclogs(
    state: State<'_, AppState>,
    id: String,
) -> Result<ShareResult> {
    instances::valid_id(&id)?;
    let paths = state.paths.clone();
    let text = tauri::async_runtime::spawn_blocking(move || {
        let dir = crate::instances::instance_dir(&paths, &id)
            .join("minecraft")
            .join("logs");
        // Хвост 512 КБ — mclo.gs сам режет большие, нам хватит с запасом.
        // ENV-9: читаем через seek-хвост, а не `std::fs::read` целиком —
        // гигантский latest.log (500 МБ+) при panic=abort ронял лаунчер OOM-ом.
        let data = crate::mclogs::read_tail(
            &crate::util::fs::long_path(&dir.join("latest.log")),
            512 * 1024,
        )?;
        Ok::<_, LauncherError>(String::from_utf8_lossy(&data).into_owned())
    })
    .await
    .map_err(join_err)??;
    let redacted = crate::util::redact::redact(&text);
    let http = state.http().await;
    let url = crate::mclogs::upload(&http, &redacted).await?;
    Ok(ShareResult { url })
}

/// Список архивных логов инстанса (F23).
#[tauri::command]
pub async fn instance_logs_list(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<crate::mclogs::LogFileInfo>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::mclogs::list_archived(&paths, &id))
        .await
        .map_err(join_err)?
}

/// Прочитать архивный лог (F23), хвост до 512 КБ.
#[tauri::command]
pub async fn instance_log_read(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<String> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::mclogs::read_archived(&paths, &id, &name, 512 * 1024)
    })
    .await
    .map_err(join_err)?
}

/// Снапшоты модов инстанса (F8).
#[tauri::command]
pub async fn content_snapshots(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<crate::instances::snapshots::SnapshotInfo>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::instances::snapshots::list(&paths, &id))
        .await
        .map_err(join_err)?
}

/// Откатить моды к снапшоту (F8).
#[tauri::command]
pub async fn content_rollback(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<Vec<crate::instances::ContentEntry>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::instances::snapshots::restore(&paths, &id, &name)?;
        Ok(crate::instances::load_content_manifest(&paths, &id))
    })
    .await
    .map_err(join_err)?
}

/// Скопировать контент в другой инстанс (F10).
#[tauri::command]
pub async fn content_copy(
    state: State<'_, AppState>,
    from_id: String,
    to_id: String,
    file: String,
) -> Result<crate::instances::ContentEntry> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::instances::content::copy_entry(&paths, &from_id, &to_id, &file)
    })
    .await
    .map_err(join_err)?
}

/// Размеры областей каталога данных (F18).
#[tauri::command]
pub async fn storage_stats(state: State<'_, AppState>) -> Result<crate::storage::Stats> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::storage::stats(&paths))
        .await
        .map_err(join_err)?
}

/// Очистить кэш и корзину ядра (F18).
#[tauri::command]
pub async fn storage_clean(state: State<'_, AppState>) -> Result<OkMsg> {
    let paths = state.paths.clone();
    let freed = tauri::async_runtime::spawn_blocking(move || crate::storage::clean(&paths))
        .await
        .map_err(join_err)??;
    Ok(OkMsg { ok: freed > 0 })
}

/// Экспорт настроек в файл без секретов (F20).
#[tauri::command]
pub async fn settings_export(
    state: State<'_, AppState>,
    path: String,
) -> Result<OkMsg> {
    let settings = state.settings.read().await.clone();
    let json = crate::settings::export_json(&settings);
    // SEC-5: ограничить примитив записи — абсолютный путь и только .json
    // (иначе IPC позволял бы перезаписать произвольный файл настройками).
    let target = std::path::PathBuf::from(&path);
    if !target.is_absolute()
        || target
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("json"))
            != Some(true)
    {
        return Err(LauncherError::InvalidInput(
            "экспорт настроек — в абсолютный файл .json".into(),
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(dir) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(crate::util::fs::long_path(dir))?;
        }
        std::fs::write(crate::util::fs::long_path(std::path::Path::new(&path)), json)?;
        Ok::<_, LauncherError>(())
    })
    .await
    .map_err(join_err)??;
    Ok(OkMsg { ok: true })
}

/// Импорт настроек из файла (F20): валидация, секреты не переносятся.
#[tauri::command]
pub async fn settings_import(
    state: State<'_, AppState>,
    path: String,
) -> Result<Settings> {
    let data = tauri::async_runtime::spawn_blocking(move || {
        std::fs::read_to_string(crate::util::fs::long_path(std::path::Path::new(&path)))
            .map_err(LauncherError::from)
    })
    .await
    .map_err(join_err)??;
    let settings = crate::settings::import_json(&data)?;
    // Тот же путь, что settings_set: файл + свап стейта + офлайн-флаг.
    settings.save(&state.paths.settings_file())?;
    let work_offline = settings.work_offline;
    *state.settings.write().await = settings.clone();
    state.http().await.set_offline(work_offline);
    Ok(settings)
}

/// Скриншоты инстанса (F3).
#[tauri::command]
pub async fn instance_screens(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<crate::instances::screens::ShotInfo>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::instances::screens::list(&paths, &id))
        .await
        .map_err(join_err)?
}

/// Удалить скриншот (F3).
#[tauri::command]
pub async fn screenshot_delete(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<OkMsg> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::instances::screens::delete(&paths, &id, &name))
        .await
        .map_err(join_err)??;
    Ok(OkMsg { ok: true })
}

/// Открыть папку скриншотов (F3).
#[tauri::command]
pub async fn instance_screens_open(
    state: State<'_, AppState>,
    id: String,
) -> Result<OkMsg> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::instances::screens::open_folder(&paths, &id))
        .await
        .map_err(join_err)??;
    Ok(OkMsg { ok: true })
}

/// Метаданные модов: зависимости и конфликты (F6/F9).
#[tauri::command]
pub async fn content_metadata(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<crate::instances::modmeta::ModMetadata>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::instances::modmeta::analyze_all(&paths, &id))
        .await
        .map_err(join_err)?
}

/// Обложки ресурспаков/шейдеров из zip (F25).
#[tauri::command]
pub async fn pack_art(
    state: State<'_, AppState>,
    id: String,
    kinds: Vec<crate::instances::ContentKind>,
) -> Result<Vec<crate::instances::packart::PackArt>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::instances::packart::analyze(&paths, &id, &kinds)
    })
    .await
    .map_err(join_err)?
}

/// Ярлык запуска на рабочем столе (F4).
#[tauri::command]
pub async fn instance_shortcut(
    state: State<'_, AppState>,
    id: String,
) -> Result<ShortcutResult> {
    let paths = state.paths.clone();
    let path = tauri::async_runtime::spawn_blocking(move || {
        crate::instances::shortcut::create_desktop(&paths, &id)
    })
    .await
    .map_err(join_err)??;
    Ok(ShortcutResult { path })
}

#[derive(Debug, serde::Serialize)]
pub struct ShortcutResult {
    pub path: String,
}

/// CPU/RAM запущенной игры (F28); None — игра не запущена.
#[tauri::command]
pub async fn game_resources(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<crate::process::monitor::GameResourceSample>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::process::sample_for_instance(&paths, &id))
        .await
        .map_err(join_err)
}

/// Запуск в безопасном режиме (F30): сброс JVM-флагов + отключение шейдеров.
#[tauri::command]
pub async fn instance_launch_safe(
    state: State<'_, AppState>,
    id: String,
    player: String,
) -> Result<OkMsg> {
    let mut inst = instances::load(&state.paths, &id)?;
    let launching_check = {
        let launching = state.launching.lock().await;
        launching.contains(&id)
    };
    if launching_check {
        return Err(LauncherError::InvalidInput("запуск уже идёт".into()));
    }
    {
        let launch_id = id.clone();
        state.launching.lock().await.insert(launch_id);
    }
    let paths = state.paths.clone();
    let measures = tauri::async_runtime::spawn_blocking(move || {
        let measures = crate::instances::run::apply_safe_mode(&paths, &mut inst);
        crate::instances::save(&paths, &inst)?;
        Ok::<_, LauncherError>(measures)
    })
    .await
    .map_err(join_err)??;
    tracing::info!("safe mode: {measures:?}");
    instance_launch(state, id, player).await
}

/// Конфиги модов: список (F29).
#[tauri::command]
pub async fn instance_configs(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<crate::instances::configs::ConfigFile>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::instances::configs::list(&paths, &id))
        .await
        .map_err(join_err)?
}

/// Полное содержимое конфига для редактора (ConfigFile — карточка списка).
#[derive(Serialize)]
pub struct ConfigContent {
    pub rel: String,
    pub text: String,
}

/// Прочитать конфиг (F29; cap 1 МБ).
#[tauri::command]
pub async fn config_read(
    state: State<'_, AppState>,
    id: String,
    rel: String,
) -> Result<ConfigContent> {
    let paths = state.paths.clone();
    let (rel_out, text) = tauri::async_runtime::spawn_blocking(move || {
        let text = crate::instances::configs::read(&paths, &id, &rel)?;
        Ok::<_, LauncherError>((rel, text))
    })
    .await
    .map_err(join_err)??;
    // Текст обязателен: раньше возвращали ConfigFile{bytes} и редактор
    // получал undefined, а «Сохранить» затирал файл пустотой.
    Ok(ConfigContent { rel: rel_out, text })
}

/// Сохранить конфиг (F29; игра запущена → instance_running).
#[tauri::command]
pub async fn config_write(
    state: State<'_, AppState>,
    id: String,
    rel: String,
    text: String,
) -> Result<OkMsg> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::instances::configs::write(&paths, &id, &rel, &text)
    })
    .await
    .map_err(join_err)??;
    Ok(OkMsg { ok: true })
}

/// Метаданные своего authlib-сервера (F13): проверка URL до входа.
#[tauri::command]
pub async fn authlib_server_info(
    state: State<'_, AppState>,
    server_url: String,
) -> Result<crate::auth::custom::ServerInfo> {
    let http = state.http().await;
    crate::auth::custom::server_info(&http, &server_url).await
}

/// Вход на свой authlib-сервер (F13): токен в keyring, сервер нормализован.
#[tauri::command]
pub async fn account_add_authlib(
    state: State<'_, AppState>,
    server_url: String,
    username: String,
    password: String,
) -> Result<crate::auth::Account> {
    let http = state.http().await;
    let server = crate::auth::custom::normalize_server_url(&server_url)?;
    let session = crate::auth::custom::login(&http, &server, &username, &password).await?;
    let acc = crate::auth::Account {
        id: uuid::Uuid::new_v4().to_string(),
        kind: crate::auth::AccountKind::Authlib,
        name: if session.player_name.is_empty() { username } else { session.player_name },
        uuid: session.uuid,
        refresh_ref: None,
        authlib_server: Some(server),
    };
    // Токен в keyring по образцу account_add_ely.
    let ref_name = format!("authlib-refresh-{}", acc.id);
    crate::auth::keyring_set(&ref_name, &session.access_token)?;
    let acc = crate::auth::Account { refresh_ref: Some(ref_name), ..acc };
    let added = crate::auth::add(&state.paths, acc)?;
    let _ = crate::auth::set_active(&state.paths, &added.id);
    Ok(added)
}

/// Выбранный пользователем PNG логотипа → каталог данных (D38).
#[tauri::command]
pub async fn logo_set_custom(state: State<'_, AppState>, path: String) -> Result<OkMsg> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::branding::set_custom(&paths, &path))
        .await
        .map_err(join_err)??;
    Ok(OkMsg { ok: true })
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogoResult {
    pub ok: bool,
    /// PNG логотипа как data-URL — для бренд-чипа интерфейса.
    pub data_url: String,
}

/// Применить логотип к иконке окна и вернуть его data-URL (бренд-чип).
#[tauri::command]
pub async fn apply_logo(state: State<'_, AppState>, logo: String) -> Result<LogoResult> {
    let app = state
        .app
        .get()
        .cloned()
        .ok_or_else(|| LauncherError::Internal("приложение ещё не готово".into()))?;
    let logo_for_apply = logo.clone();
    let paths = state.paths.clone();
    let data_url = tauri::async_runtime::spawn_blocking(move || {
        crate::branding::apply_window_icon(&app, &logo_for_apply, &paths);
        let bytes = crate::branding::resolve_png(&logo_for_apply, &paths);
        use base64::Engine as _;
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    })
    .await
    .map_err(join_err)?;
    Ok(LogoResult { ok: true, data_url })
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
    let data = crate::mclogs::read_tail(&crate::util::fs::long_path(&log), 2 * 1024 * 1024)?;
    // ENV-9: launcher.log может разрастись — читаем хвост до 2 МБ, а не весь файл.
    let text = String::from_utf8_lossy(&data);
    let all: Vec<&str> = text.lines().collect();
    let skip = all.len().saturating_sub(lines as usize);
    Ok(all[skip..].join("\n"))
}

#[tauri::command]
pub async fn dir_open(state: State<'_, AppState>) -> Result<OkMsg> {
    tauri_plugin_opener::open_path(state.paths.root().clone(), None::<&str>)
        .map_err(|e| LauncherError::Internal(format!("открытие папки: {e}")))?;
    Ok(OkMsg { ok: true })
}

// ---------- проверка обновлений лаунчера ----------

/// Последний релиз репозитория владельца. Репозиторий станет публичным; пока
/// закрыт — 404 это нормальный исход проверки, а не сбой лаунчера.
const RELEASES_API: &str = "https://api.github.com/repos/loxord241/Cobble-Launcher/releases/latest";
/// Запасная цель кнопки «открыть»: если GitHub не отдал `html_url` релиза.
const RELEASES_PAGE: &str = "https://github.com/loxord241/Cobble-Launcher/releases";
/// Проверка обновлений интерактивна (ждёт человек) — не дольше 10 с.
const UPDATE_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Ответ `releases/latest` — берём только тег и ссылку на страницу релиза.
#[derive(serde::Deserialize)]
struct GhRelease {
    #[serde(default)]
    tag_name: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
}

/// Итог проверки обновлений для карточки в настройках. Авто-обновления нет
/// (нет ключей подписи): ядро только сравнивает версии и даёт ссылку.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// Текущая версия лаунчера (`CARGO_PKG_VERSION`).
    pub current: String,
    /// Тег последнего релиза (`v0.2.0`); `None` — версию узнать не удалось.
    pub latest: Option<String>,
    /// Страница релиза на github.com.
    pub url: Option<String>,
    pub update_available: bool,
    /// Проверка состоялась (в том числе «не удалось» — это не ошибка команды).
    pub checked: bool,
    /// Код причины из `errors.json`, если версию узнать не удалось: UI переводит
    /// его своим языком (русский текст в ядре не подошёл бы en/uk).
    pub note: Option<String>,
}

/// Одна попытка достать последний релиз. Ошибка — код причины для UI:
/// `not_found` (404: репозиторий закрыт или релизов ещё нет), `http` (другой
/// статус), `json` (ответ не разобрался), `network` (транспорт).
async fn fetch_latest_release(
    http: &HttpClient,
) -> std::result::Result<(String, String), &'static str> {
    let resp = http
        .raw()
        .get(RELEASES_API)
        .send()
        .await
        .map_err(|_| "network")?;
    if !resp.status().is_success() {
        return Err(if resp.status().as_u16() == 404 {
            "not_found"
        } else {
            "http"
        });
    }
    let release: GhRelease = resp.json().await.map_err(|_| "json")?;
    let tag = release
        .tag_name
        .filter(|t| !t.trim().is_empty())
        .ok_or("json")?;
    // В opener ссылку из ответа пускаем только как github.com-https.
    let url = release
        .html_url
        .filter(|u| u.starts_with("https://github.com/"))
        .unwrap_or_else(|| RELEASES_PAGE.to_string());
    Ok((tag, url))
}

/// semver-lite: `v0.2.0` → `[0, 2, 0]`. Хвост после цифр в части (`0.2.0-rc1`)
/// отбрасывается, мусор → `None` — обновление не обещаем и не паникуем.
fn parse_version(version: &str) -> Option<Vec<u64>> {
    let trimmed = version.trim().trim_start_matches(['v', 'V']);
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .split('.')
        .map(|part| {
            let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().ok()
        })
        .collect()
}

/// `latest` строго новее `current`? Непонятный формат → `false`: честнее
/// промолчать, чем предложить обновление на мусорный тег.
fn is_newer(latest: &str, current: &str) -> bool {
    let (Some(latest), Some(current)) = (parse_version(latest), parse_version(current)) else {
        return false;
    };
    for i in 0..latest.len().max(current.len()) {
        let a = latest.get(i).copied().unwrap_or(0);
        let b = current.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

/// Проверить обновления лаунчера (GitHub releases/latest). Недоступность сети,
/// закрытый репозиторий или неразобранный ответ — не ошибка команды: UI получает
/// `checked: true` и `note` с кодом причины.
#[tauri::command]
pub async fn update_check(state: State<'_, AppState>) -> Result<UpdateInfo> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let http = state.http().await;
    let (latest, url, note) =
        match tokio::time::timeout(UPDATE_CHECK_TIMEOUT, fetch_latest_release(&http)).await {
            Ok(Ok((tag, url))) => (Some(tag), Some(url), None),
            Ok(Err(code)) => (None, None, Some(code.to_string())),
            Err(_) => (None, None, Some("timeout".to_string())),
        };
    let update_available = latest
        .as_deref()
        .is_some_and(|latest| is_newer(latest, &current));
    Ok(UpdateInfo {
        current,
        latest,
        url,
        update_available,
        checked: true,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A7: фронт (`src/api/types.ts`) читает `releaseTime` и `type` —
    /// snake_case `release_time` ломал каталог версий в UI.
    #[test]
    fn version_entry_serializes_camel_case() {
        let v = VersionEntry {
            id: "1.20.1".into(),
            version_type: "release".into(),
            release_time: "2023-06-12T12:00:00+00:00".into(),
        };
        let json = serde_json::to_string(&v).unwrap();
        assert!(
            json.contains("\"releaseTime\""),
            "фронт ждёт releaseTime: {json}"
        );
        assert!(
            json.contains("\"type\":\"release\""),
            "type сохраняется явным rename: {json}"
        );
        assert!(
            !json.contains("release_time") && !json.contains("version_type"),
            "snake_case ушёл из контракта: {json}"
        );
    }

    /// Сравнение версий: `v0.2.0` новее `0.1.0`, равные — не обновление.
    #[test]
    fn version_compare_semver_lite() {
        assert!(is_newer("v0.2.0", "0.1.0"), "ведущая v не мешает сравнению");
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"), "та же версия — не обновление");
        assert!(!is_newer("v0.1.0", "0.1.0"), "ведущая v не делает версию новой");
        assert!(!is_newer("0.1", "0.1.0"), "0.1 и 0.1.0 — одна версия");
        assert!(!is_newer("0.2.0", "1.0.0"), "старый тег — не обновление");
    }

    /// Мусор в теге: ни паники, ни ложного «есть обновление».
    #[test]
    fn version_parser_survives_garbage() {
        assert_eq!(parse_version("v0.2.0"), Some(vec![0, 2, 0]));
        assert_eq!(parse_version(" V1.20.1 "), Some(vec![1, 20, 1]), "пробелы и V");
        assert_eq!(
            parse_version("0.2.0-rc1"),
            Some(vec![0, 2, 0]),
            "хвост после цифр игнорируется"
        );
        for junk in ["", "   ", "v", "мусор", "1.2.x", "1..3", "1.99999999999999999999"] {
            assert!(parse_version(junk).is_none(), "мусор → None: {junk:?}");
            assert!(!is_newer(junk, "0.1.0"), "мусор не новее: {junk:?}");
            assert!(!is_newer("0.1.0", junk), "мусор не старее: {junk:?}");
        }
    }

    /// Фронт (`src/api/types.ts`) читает `updateAvailable`/`checked` — контракт
    /// camelCase (D25); `note` без причины уезжает как `null`.
    #[test]
    fn update_info_serializes_camel_case() {
        let info = UpdateInfo {
            current: "0.1.0".into(),
            latest: Some("v0.2.0".into()),
            url: Some(RELEASES_PAGE.into()),
            update_available: true,
            checked: true,
            note: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"updateAvailable\":true"), "{json}");
        assert!(json.contains("\"current\":\"0.1.0\""), "{json}");
        assert!(
            !json.contains("update_available") && !json.contains("tag_name"),
            "snake_case ушёл из контракта: {json}"
        );
        assert!(json.contains("\"note\":null"), "None приезжает как null: {json}");
    }

    /// B6: слияние настроек на сервере — из payload переносятся ТОЛЬКО
    /// пользовательские поля; статистика супервизора (play_seconds,
    /// launch_count, last_played), дописанная на диск после Get, не затирается
    /// устаревшим снапшотом фронтенда.
    #[test]
    fn settings_set_merges_user_fields_only() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut disk = Instance::new("x", "1.20.1");
        disk.play_seconds = 100;
        disk.launch_count = 3;
        disk.last_played = Some(12345);
        instances::save(&paths, &disk).unwrap();

        // Устаревший снапшот фронта: пользовательская RAM новая, статистика —
        // старая (superвизор уже дописал свои 100 секунд).
        let mut payload = disk.clone();
        payload.play_seconds = 50;
        payload.launch_count = 1;
        payload.last_played = None;
        payload.ram_mb = 4096;
        payload.notes = "заметка".into();

        // Тот же путь, что в команде: update под SAVE_LOCK + merge_user_settings.
        instances::update(&paths, &payload.id, |cur| merge_user_settings(cur, &payload)).unwrap();
        let saved = instances::load(&paths, &payload.id).unwrap();
        assert_eq!(saved.play_seconds, 100, "статистика с диска не затёрта");
        assert_eq!(saved.launch_count, 3, "launch_count с диска");
        assert_eq!(saved.last_played, Some(12345), "last_played с диска");
        assert_eq!(saved.ram_mb, 4096, "пользовательское поле перенесено");
        assert_eq!(saved.notes, "заметка", "notes перенесены");
    }

    /// ENV-13: офлайн-uuid считается от санитизированного имени — тот же путь,
    /// что в account_add_offline. Ник «Steve\r\n» должен дать тот же uuid,
    /// что чистый «Steve» (иначе инвентарь офлайн-миров рассинхронизируется).
    #[test]
    fn offline_account_uuid_uses_sanitized_name() {
        let clean = crate::util::names::sanitize_user_name("Steve\r\n", "Player");
        assert_eq!(clean, "Steve", "управляющие символы вырезаны");
        assert_eq!(
            crate::auth::offline::offline_uuid(&clean),
            crate::auth::offline::offline_uuid("Steve"),
            "uuid от чистого имени совпадает с vanilla-правилом"
        );
        assert_ne!(
            crate::auth::offline::offline_uuid("Steve\r\n"),
            crate::auth::offline::offline_uuid("Steve"),
            "сырой ввод давал бы другой uuid — это и был баг"
        );
    }

    /// Реальная сеть — #[ignore], гонять на приёмке (спека §9). Пока репозиторий
    /// закрыт, 404 (`not_found`) — тоже корректный исход проверки.
    #[tokio::test]
    #[ignore]
    async fn real_github_release_fetch() {
        let http = HttpClient::new(None).unwrap();
        match fetch_latest_release(&http).await {
            Ok((tag, url)) => {
                assert!(parse_version(&tag).is_some(), "тег релиза: {tag}");
                assert!(url.starts_with("https://github.com/"), "ссылка: {url}");
            }
            Err(code) => assert!(
                matches!(code, "not_found" | "http" | "network"),
                "неожиданная причина: {code}"
            ),
        }
    }
}
