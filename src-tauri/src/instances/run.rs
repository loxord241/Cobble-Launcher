//! Подготовка и запуск инстанса (спека §6.10, §6.12). Оркестрация над доменами:
//! версия → загрузки в стор → hardlink в инстанс → Java → команда → спавн.

use crate::errors::{LauncherError, Result};
use crate::events::{EventBus, LaunchPhase, LauncherEvent};
use crate::instances::{minecraft_dir, Instance, InstanceLock};
use crate::java::detect::{pick_java, scan_all, JavaInstall};
use crate::mojang::assets::{
    fetch_asset_index, materialize_virtual, needs_virtual, plan_asset_tasks,
};
use crate::mojang::client::{client_jar_in_store, plan_client_task};
use crate::mojang::launch::{build_command, LaunchCommand, LaunchValues};
use crate::mojang::libraries::{build_classpath, extract_native, plan_libraries};
use crate::mojang::manifest::{fetch_manifest, fetch_version_json_cached, resolve_entry};
use crate::mojang::rules::OsContext;
use crate::mojang::version::VersionJson;
use crate::net::download::{DownloadEngine, DownloadTask};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Всё, что нужно для спавна игры инстанса.
pub struct PreparedLaunch {
    pub command: LaunchCommand,
    pub game_dir: PathBuf,
    pub version_id: String,
    pub java: JavaInstall,
}

/// Фаза → событие UI.
fn emit(bus: &EventBus, instance_id: &str, phase: LaunchPhase) {
    bus.emit(LauncherEvent::LaunchState {
        instance_id: instance_id.to_string(),
        phase,
        exit_code: None,
    });
}

/// Подготовить инстанс к запуску: файлы, Java, команда (без спавна).
#[allow(clippy::too_many_arguments)]
pub async fn prepare(
    paths: &Paths,
    settings: &crate::settings::Settings,
    client: Arc<HttpClient>,
    engine: Arc<DownloadEngine>,
    bus: &EventBus,
    inst: &Instance,
    account: AccountIdentity,
) -> Result<PreparedLaunch> {
    emit(bus, &inst.id, LaunchPhase::Preparing);
    paths.ensure_dirs()?;

    // 1. Версия: манифест → JSON → inheritsFrom (для загрузчиков M3+).
    let manifest = fetch_manifest(&client, &paths.manifests_cache()).await?;
    let entry = resolve_entry(&manifest, &inst.mc_version)?;
    let vjson = fetch_version_json_cached(&client, entry, &paths.manifests_cache()).await?;
    let rv = VersionJson::resolve_chain(&paths.manifests_cache(), &vjson)?;

    // 2. Java: путь инстанса → системная → Adoptium (спека §6.1.5, M2).
    let need_major = rv
        .java_version
        .as_ref()
        .map(|j| j.major_version)
        .unwrap_or_else(|| fallback_major(&inst.mc_version));
    let java = resolve_java(paths, settings, client.clone(), need_major, &inst.java_path).await?;
    let is_arm = std::env::consts::ARCH.contains("arm");
    let os_ctx = OsContext::current(java.arch_bits, is_arm);

    // 3. Загрузки в стор (группа инстанса с высоким приоритетом).
    emit(bus, &inst.id, LaunchPhase::Downloading);
    let group = format!("instance:{}", inst.id);
    let mut tasks: Vec<DownloadTask> = vec![plan_client_task(&rv, &paths.clients_store(), &group)?];
    let libs = plan_libraries(&rv, &os_ctx, &BTreeMap::new())?;
    for l in &libs {
        if let Some(a) = &l.artifact {
            tasks.push(DownloadTask {
                id: format!("lib:{}", a.rel_path),
                url: a.url.clone(),
                dest: paths.libraries_store().join(&a.rel_path),
                sha1: a.sha1.clone(),
                size: a.size,
                group: group.clone(),
                priority: 10,
            });
        }
        if let Some(n) = &l.native {
            tasks.push(DownloadTask {
                id: format!("native:{}", n.rel_path),
                url: n.url.clone(),
                dest: paths.libraries_store().join(&n.rel_path),
                sha1: n.sha1.clone(),
                size: n.size,
                group: group.clone(),
                priority: 15,
            });
        }
    }
    let ai = rv
        .asset_index
        .clone()
        .ok_or_else(|| LauncherError::InvalidInput(format!("в версии {} нет assetIndex", rv.id)))?;
    let index = fetch_asset_index(
        &client,
        &ai.url,
        &ai.sha1,
        &ai.id,
        &paths.assets_indexes(),
    )
    .await?;
    tasks.extend(plan_asset_tasks(&index, &paths.assets_objects(), &group));
    if let Some(lc) = rv.logging.as_ref().and_then(|l| l.client.as_ref()) {
        tasks.push(DownloadTask {
            id: format!("log-config:{}", lc.file.id),
            url: lc.file.url.clone(),
            dest: paths.cache_dir().join("log_configs").join(&lc.file.id),
            sha1: Some(lc.file.sha1.clone()),
            size: lc.file.size,
            group: group.clone(),
            priority: 20,
        });
    }

    // Движок — на план этого запуска: дедуп работает внутри плана, а уже
    // скачанные валидные файлы пропускаются мгновенно (сверка хэша на диске).
    // Восстановление прерванной очереди (queue.json) — отдельный UI-поток (M4+):
    // старые Failed-задачи не должны блокировать новые запуски.
    engine.add_tasks(tasks);
    engine.run().await?;
    engine.persist_queue(&paths.queue_file())?;
    let st = engine.queue_state();
    if st.failed > 0 {
        for (url, reason) in &st.failed_items {
            tracing::error!("загрузка не удалась: {url}: {reason}");
        }
        return Err(LauncherError::network(format!(
            "{} загрузок не удалось (см. список в UI)",
            st.failed
        )));
    }

    // 4. Hardlink контента стора в каталог игры инстанса (спека §6.12).
    let game_dir = minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
    std::fs::create_dir_all(crate::util::fs::long_path(&game_dir))?;
    let mc_version_dir = game_dir.join("versions").join(&rv.id);
    let client_sha = rv
        .client_download
        .as_ref()
        .and_then(|c| c.sha1.clone())
        .ok_or_else(|| LauncherError::InvalidInput("нет клиента версии".into()))?;
    crate::instances::store::link_or_copy(
        &client_jar_in_store(&paths.clients_store(), &client_sha),
        &mc_version_dir.join(format!("{}.jar", rv.id)),
    )?;
    for l in &libs {
        if let Some(a) = &l.artifact {
            crate::instances::store::link_or_copy(
                &paths.libraries_store().join(&a.rel_path),
                &game_dir.join("libraries").join(&a.rel_path),
            )?;
        }
        if let Some(n) = &l.native {
            crate::instances::store::link_or_copy(
                &paths.libraries_store().join(&n.rel_path),
                &game_dir.join("libraries").join(&n.rel_path),
            )?;
        }
    }

    // 5. Нативы распаковываются в каталог версии (1.19+: пустой каталог).
    let natives_dir = game_dir.join("bin").join(&rv.id);
    std::fs::create_dir_all(crate::util::fs::long_path(&natives_dir))?;
    let mut natives_count = 0;
    for l in &libs {
        if let Some(n) = &l.native {
            let zip = paths.libraries_store().join(&n.rel_path);
            if zip.exists() {
                let exclude = rv
                    .libraries
                    .iter()
                    .find(|lib| lib.name == l.name)
                    .and_then(|lib| lib.extract.clone())
                    .map(|e| e.exclude)
                    .unwrap_or_default();
                natives_count += extract_native(&zip, &natives_dir, &exclude)?;
            }
        }
    }
    tracing::info!("инстанс {}: нативов распаковано {natives_count}", inst.id);

    // 6. virtual/legacy ассеты (спека §6.4).
    let game_assets = if needs_virtual(&rv.assets, &index) {
        let vd = paths.assets_virtual(&rv.assets);
        materialize_virtual(&index, &paths.assets_objects(), &vd)?;
        Some(vd)
    } else {
        None
    };

    // 7. Команда запуска.
    emit(bus, &inst.id, LaunchPhase::Launching);
    let classpath = build_classpath(
        &libs,
        &game_dir.join("libraries"),
        &mc_version_dir.join(format!("{}.jar", rv.id)),
    );
    let values = LaunchValues {
        auth_player_name: account.player_name,
        auth_uuid: account.uuid,
        auth_access_token: account.access_token,
        user_type: account.user_type,
        version_name: rv.id.clone(),
        version_type: rv.version_type.clone().unwrap_or_else(|| "release".into()),
        game_directory: game_dir.clone(),
        assets_root: paths.assets_dir(),
        assets_index_name: ai.id.clone(),
        game_assets,
        natives_directory: natives_dir,
        launcher_name: "mc-launcher-v2".into(),
        launcher_version: env!("CARGO_PKG_VERSION").into(),
        classpath,
        library_directory: paths.libraries_store(),
        resolution: settings.default_resolution.map(|r| (r.width, r.height)),
        log_config_path: rv
            .logging
            .as_ref()
            .and_then(|l| l.client.as_ref())
            .map(|lc| paths.cache_dir().join("log_configs").join(&lc.file.id)),
        jvm_extra: {
            let mut flags = vec![format!("-Xmx{}M", inst.ram_mb)];
            flags.extend(inst.jvm_flags.iter().cloned());
            flags
        },
    };
    let command = build_command(&rv, &values, &os_ctx, &java.java_exe)?;

    Ok(PreparedLaunch {
        command,
        game_dir,
        version_id: rv.id,
        java,
    })
}

/// Личность аккаунта для запуска (M2: offline; M8 добавит MSA/ely).
pub struct AccountIdentity {
    pub player_name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
}

pub fn offline_identity(nick: &str) -> AccountIdentity {
    AccountIdentity {
        player_name: nick.to_string(),
        uuid: crate::auth::offline::offline_uuid(nick).to_string(),
        access_token: "0".into(), // offline: token="0" (спека §6.8)
        user_type: "legacy".into(),
    }
}

/// Java для инстанса: путь из инстанса (валидация) → системная → Adoptium.
pub async fn resolve_java(
    paths: &Paths,
    settings: &crate::settings::Settings,
    client: Arc<HttpClient>,
    need_major: u32,
    instance_java_path: &Option<String>,
) -> Result<JavaInstall> {
    // 1. Явный путь в инстансе
    if let Some(p) = instance_java_path {
        let exe = java_exe_of(PathBuf::from(p));
        if exe.exists() {
            let inst = crate::java::detect::inspect(&exe, "инстанс").await?;
            if inst.major < need_major {
                return Err(LauncherError::JavaNotFound(format!(
                    "указанная Java {} < требуемой {need_major}",
                    inst.major
                )));
            }
            return Ok(inst);
        }
        return Err(LauncherError::JavaNotFound(format!(
            "java_path не существует: {p}"
        )));
    }
    // 2. Системная / рантаймы лаунчера
    let installs = scan_all(&paths.runtime_dir(), &settings.extra_java_paths).await;
    if let Some(found) = pick_java(need_major, &installs) {
        return Ok(found.clone());
    }
    // 3. Adoptium (спека §6.5, M2)
    tracing::info!("Java {need_major} не найдена — скачиваю с Adoptium");
    let exe = crate::java::adoptium::install_jre(&client, &paths.runtime_dir(), need_major, |_, _| {})
        .await?;
    crate::java::detect::inspect(&exe, "adoptium").await
}

fn java_exe_of(dir_or_exe: PathBuf) -> PathBuf {
    let name = if cfg!(windows) { "java.exe" } else { "java" };
    if dir_or_exe.is_file() {
        dir_or_exe
    } else {
        dir_or_exe.join("bin").join(name)
    }
}

/// Полный запуск инстанса с `.lock`, обновлением статистики и супервизией.
/// Возвращает (найден маркер меню?, PID игры) — логика ожидания переиспользуется
/// из CLI; в UI (M4) заменяется на события GameLogLine.
pub fn launch_blocking(
    paths: &Paths,
    inst: &Instance,
    prepared: PreparedLaunch,
    bus: &EventBus,
    menu_timeout_secs: u64,
    wait_for_menu: bool,
) -> Result<bool> {
    let inst_dir = crate::instances::instance_dir(paths, &inst.id);
    let lock = InstanceLock::acquire(&inst_dir)?;

    // Статистика запуска
    let mut updated = inst.clone();
    updated.last_played = Some(crate::instances::now_secs());
    updated.launch_count += 1;
    crate::instances::save(paths, &updated)?;

    let mut command = std::process::Command::new(&prepared.command.program);
    command
        .args(&prepared.command.args)
        .current_dir(crate::util::fs::long_path(&prepared.game_dir))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| LauncherError::JavaNotFound(format!("не удалось запустить игру: {e}")))?;
    emit(bus, &inst.id, LaunchPhase::Running);

    let mut found = true;
    if wait_for_menu {
        found = supervise_to_menu(&mut child, &prepared.game_dir, bus, &inst.id, menu_timeout_secs)?;
    }
    emit(bus, &inst.id, LaunchPhase::Exited);
    drop(lock);
    Ok(found)
}

/// Чтение stdout/stderr → лог-файл + события, ожидание маркера меню.
fn supervise_to_menu(
    child: &mut std::process::Child,
    game_dir: &std::path::Path,
    bus: &EventBus,
    instance_id: &str,
    timeout_secs: u64,
) -> Result<bool> {
    use std::io::{BufRead, Write};
    let logs_dir = game_dir.join("logs");
    std::fs::create_dir_all(crate::util::fs::long_path(&logs_dir))?;
    let log_path = crate::util::fs::long_path(&logs_dir.join("latest.log"));
    let mut log_file = std::fs::File::create(&log_path)?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LauncherError::Internal("stdout отсутствует".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| LauncherError::Internal("stderr отсутствует".into()))?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let streams: Vec<Box<dyn std::io::Read + Send>> = vec![Box::new(stdout), Box::new(stderr)];
    for stream in streams {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stream);
            for line in reader.lines().map_while(std::result::Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);

    let start = std::time::Instant::now();
    let mut found_at: Option<std::time::Instant> = None;
    loop {
        if start.elapsed() > std::time::Duration::from_secs(timeout_secs) {
            break;
        }
        // После маркера даём 3 с на дочитывание (атласы) — для UI-запуска
        // маркер не обязателен, там супервизия живёт до выхода процесса.
        if let Some(t) = found_at {
            if t.elapsed() > std::time::Duration::from_secs(3) {
                break;
            }
        }
        match rx.recv_timeout(std::time::Duration::from_millis(250)) {
            Ok(line) => {
                let _ = writeln!(log_file, "{line}");
                bus.emit(LauncherEvent::GameLogLine {
                    instance_id: instance_id.to_string(),
                    line: line.clone(),
                    stream: crate::events::LogStream::Stdout,
                });
                if found_at.is_none() && line.contains("Sound engine started") {
                    found_at = Some(std::time::Instant::now());
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(found_at.is_some())
}

/// Fallback Java-минимумов по версии MC (спека §6.4).
pub fn fallback_major(mc_version: &str) -> u32 {
    let parts: Vec<u32> = mc_version
        .split('.')
        .map(|p| p.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("0"))
        .map(|p| p.parse::<u32>().unwrap_or(0))
        .collect();
    let (major, minor) = (parts.first().copied().unwrap_or(0), parts.get(1).copied().unwrap_or(0));
    if major > 1 {
        return 21;
    }
    match (major, minor) {
        (1, m) if m >= 20 => 21,
        (1, m) if (17..=19).contains(&m) => 17,
        _ => 8,
    }
}
