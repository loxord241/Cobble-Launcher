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
use std::os::windows::process::CommandExt;
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

/// Строка игры одновременно уходит в `latest.log` и в событие UI — ОБА вывода
/// обязаны проходить redaction (A15): логи модов/чата печатают токен доступа,
/// а реестр секретов наполняется при авторизации (спека §11, «секреты не
/// покидают процесс»). Логировался без redact только launcher.log — этого мало.
fn write_game_line(
    log_path: &std::path::Path,
    bus: &EventBus,
    instance_id: &str,
    line: &str,
    stream: crate::events::LogStream,
) {
    let safe = crate::util::redact::redact(line);
    // ENV-4: handle лога живёт только внутри этой функции — игра (Log4j)
    // пишет и РОТИРУЕТ тот же latest.log, чужой открытый дескриптор ломал
    // ротацию и давал двойную запись с затиранием байтов. Append-режим на
    // Windows дописывает в конец атомарно, дескриптор почти всегда закрыт.
    // Открыть не удалось — запуск не роняем, событие в UI уходит как обычно.
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{safe}\n").as_bytes()));
    bus.emit(LauncherEvent::GameLogLine {
        instance_id: instance_id.to_string(),
        line: safe,
        stream,
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

    // 1. Версия: манифест → JSON → inheritsFrom (загрузчики: корень —
    //    version JSON загрузчика в versions/ инстанса, спека §6.3).
    let instance_versions = crate::instances::instance_versions_dir(paths, &inst.id);
    let manifest = fetch_manifest(&client, &paths.manifests_cache()).await?;
    let root_json = if let Some(vid) = &inst.version_id {
        let path = instance_versions.join(format!("{vid}.json"));
        if !path.exists() {
            return Err(LauncherError::NotFound(format!(
                "version JSON загрузчика {vid} — установите загрузчик заново"
            )));
        }
        VersionJson::load(&path)?
    } else {
        let entry = resolve_entry(&manifest, &inst.mc_version)?;
        fetch_version_json_cached(&client, entry, &paths.manifests_cache()).await?
    };
    // Родители цепочки ищутся в versions/ инстанса, затем в кэше Mojang.
    let rv = VersionJson::resolve_chain_dirs(
        &[instance_versions, paths.manifests_cache()],
        &root_json,
    )?;

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
                sha512: None,
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
                sha512: None,
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
            sha512: None,
            size: lc.file.size,
            group: group.clone(),
            priority: 20,
        });
    }

    // Движок — на план этого запуска: дедуп работает внутри плана, а уже
    // скачанные валидные файлы пропускаются мгновенно (сверка хэша на диске).
    engine.add_tasks(tasks);
    engine.run().await?;
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
    std::fs::create_dir_all(crate::util::fs::long_path(&game_dir))
        .map_err(|e| LauncherError::Internal(format!("create_dir_all {}: {e}", game_dir.display())))?;
    // A51: с этого шага подготовка успевает оставить следы на диске — уборка
    // висит на Drop и снимается только при успешном возврате (см. PrepareCleanup).
    let mut cleanup = PrepareCleanup::new(game_dir.clone());
    let mc_version_dir = game_dir.join("versions").join(&rv.id);
    let client_sha = rv
        .client_download
        .as_ref()
        .and_then(|c| c.sha1.clone())
        .ok_or_else(|| LauncherError::InvalidInput("нет клиента версии".into()))?;
    crate::instances::store::link_or_copy(
        &client_jar_in_store(&paths.clients_store(), &client_sha),
        &mc_version_dir.join(format!("{}.jar", rv.id)),
    )
    .map_err(|e| LauncherError::Internal(format!("hardlink клиента {}: {e}", rv.id)))?;
    for l in &libs {
        if let Some(a) = &l.artifact {
            crate::instances::store::link_or_copy(
                &paths.libraries_store().join(&a.rel_path),
                &game_dir.join("libraries").join(&a.rel_path),
            )
            .map_err(|e| {
                LauncherError::Internal(format!("hardlink библиотеки {}: {e}", a.rel_path))
            })?;
        }
        if let Some(n) = &l.native {
            crate::instances::store::link_or_copy(
                &paths.libraries_store().join(&n.rel_path),
                &game_dir.join("libraries").join(&n.rel_path),
            )
            .map_err(|e| {
                LauncherError::Internal(format!("hardlink натива {}: {e}", n.rel_path))
            })?;
        }
    }

    // 5. Нативы распаковываются в каталог версии (1.19+: пустой каталог).
    let natives_dir = game_dir.join("bin").join(&rv.id);
    cleanup.track_natives(natives_dir.clone());
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
    // Инсталляторы NeoForge/Forge кладут universal и binpatch-клиент в свой
    // maven-каталог; в version JSON их нет — официальный лаунчер добавляет их
    // в classpath сам (подтверждается `-DignoreList` с префиксами `neoforge-`).
    let mut extra_jars: Vec<PathBuf> = Vec::new();
    if let (Some(loader), Some(lv)) = (inst.loader.as_deref(), inst.loader_version.as_deref()) {
        let loader_dir = match loader {
            "neoforge" => Some(game_dir.join("libraries").join("net/neoforged/neoforge").join(lv)),
            "forge" => Some(game_dir.join("libraries").join("net/minecraftforge/forge").join(lv)),
            _ => None,
        };
        if let Some(dir) = loader_dir {
            if let Ok(entries) = std::fs::read_dir(crate::util::fs::long_path(&dir)) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "jar") {
                        extra_jars.push(p);
                    }
                }
            }
            extra_jars.sort();
        }
    }
    let classpath = build_classpath(
        &libs,
        &game_dir.join("libraries"),
        &mc_version_dir.join(format!("{}.jar", rv.id)),
        &extra_jars,
    );
    let mut jvm_extra_flags = Vec::new();
    if let Some(agent) = &account.javaagent {
        // -javaagent должен стоять до пользовательских флагов (порядок JVM-флагов
        // между собой не критичен, но agent — системный).
        jvm_extra_flags.push(agent.clone());
    }
    jvm_extra_flags.push(format!("-Xmx{}M", inst.ram_mb));
    jvm_extra_flags.extend(inst.jvm_flags.iter().cloned());
    // ВАЖНО: не добавлять -DlegacyClassPath с полным classpath — командная
    // строка Windows ограничена 32K и дублирование -cp даёт os error 206
    // (флаг нужен FML только в dev-режиме).
    // NeoForge: мод лежит в universal jar вне version JSON — официальный
    // лаунчер передаёт его через игровые аргументы --mavenRoot/--mods
    // (FMLServiceProvider, joptsimple: «Maven root directories» / «List of mods
    // to add»). Без них Mod List пуст и FML падает на findModule("neoforge").
    let mut game_args = inst.game_args_extra.clone();
    if inst.loader.as_deref() == Some("neoforge") {
        if let Some(lv) = &inst.loader_version {
            game_args.push("--mavenRoot".into());
            game_args.push(game_dir.join("libraries").to_string_lossy().into_owned());
            game_args.push("--mods".into());
            game_args.push(format!("net.neoforged:neoforge:{lv}:universal"));
        }
    }
    // F1: быстрый запуск — сразу в мир или на сервер (MC 1.20+). Пара
    // флаг+значение пушится в конец игровых аргументов, на старые версии
    // флаги не попадают вовсе (иначе игра их не поймёт).
    game_args.extend(quick_play_args(
        &inst.mc_version,
        inst.quick_play_world.as_deref(),
        inst.quick_play_server.as_deref(),
    ));
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
        // ${library_directory} обязан указывать на библиотеки ИНСТАНСА:
        // там и хардлинкнутые библиотеки, и артефакты инсталлятора
        // (universal/client/srg/extra), которые FML ищет maven-путём.
        library_directory: game_dir.join("libraries"),
        resolution: settings.default_resolution.map(|r| (r.width, r.height)),
        log_config_path: rv
            .logging
            .as_ref()
            .and_then(|l| l.client.as_ref())
            .map(|lc| paths.cache_dir().join("log_configs").join(&lc.file.id)),
        jvm_extra: jvm_extra_flags,
        game_args_extra: game_args,
    };
    let command = build_command(&rv, &values, &os_ctx, &java.java_exe)?;

    cleanup.disarm();
    Ok(PreparedLaunch {
        command,
        game_dir,
        version_id: rv.id,
        java,
    })
}

/// Уборка недоделанной подготовки запуска (A51): до фикса сбой на шагах
/// 5–7 (нативы, virtual-ассеты, сборка команды) оставлял в инстансе
/// полураспакованные нативы и `.part`-хвосты — следующий запуск считал их
/// готовыми. Убираем best-effort (ошибки только в лог) и отдаём наружу
/// исходную причину сбоя.
///
/// Hardlink'и библиотек/клиента НЕ трогаем: ссылка не бывает частичной
/// (файл в сторе уже прошёл проверку хэша), а `link_or_copy` идемпотентен.
struct PrepareCleanup {
    game_dir: PathBuf,
    natives_dir: Option<PathBuf>,
    armed: bool,
}

impl PrepareCleanup {
    fn new(game_dir: PathBuf) -> Self {
        Self {
            game_dir,
            natives_dir: None,
            armed: true,
        }
    }

    /// Запомнить каталог нативов ДО его создания/распаковки.
    fn track_natives(&mut self, dir: PathBuf) {
        self.natives_dir = Some(dir);
    }

    /// Подготовка прошла успешно — убирать нечего.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PrepareCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        tracing::warn!(
            "подготовка запуска прервана: чищу недоделанное в {}",
            self.game_dir.display()
        );
        if let Some(natives) = self.natives_dir.clone() {
            if natives.exists() {
                if let Err(e) = std::fs::remove_dir_all(crate::util::fs::long_path(&natives)) {
                    tracing::warn!("нативы {} не удалены: {e}", natives.display());
                }
            }
        }
        // `.part` пишутся рядом с целевым файлом — обходим сам каталог игры и
        // каталоги контента (глубже в libraries/assets `.part` не бывает).
        for dir in [
            self.game_dir.clone(),
            self.game_dir.join("mods"),
            self.game_dir.join("resourcepacks"),
            self.game_dir.join("shaderpacks"),
        ] {
            remove_part_files(&dir);
        }
    }
}

/// Удалить `*.part` на одном уровне каталога (best-effort, ошибки игнорируем).
fn remove_part_files(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(crate::util::fs::long_path(dir)) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "part") {
            crate::util::fs::remove_file_ignore(&p);
        }
    }
}

/// Личность аккаунта для запуска (offline/MSA/ely — спека §6.8).
pub struct AccountIdentity {
    pub player_name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    /// ely.by: `-javaagent:...=server` (ЕДИНСТВЕННОЕ исключение из ноль-инжекций).
    pub javaagent: Option<String>,
}

pub fn offline_identity(nick: &str) -> AccountIdentity {
    AccountIdentity {
        player_name: nick.to_string(),
        uuid: crate::auth::offline::offline_uuid(nick).to_string(),
        access_token: "0".into(), // offline: token="0" (спека §6.8)
        user_type: "legacy".into(),
        javaagent: None,
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

/// Каталог для `Command::current_dir` (A50): дочерний процесс получает его
/// через SetCurrentDirectoryW/CreateProcessW, которые расширенные пути `\\?\`
/// не принимают (в отличие от файловых API) — префикс снимаем. Для игры
/// длинный путь тут и не помогал: лимит Win32 на рабочий каталог не обойти.
fn spawn_dir(game_dir: &std::path::Path) -> PathBuf {
    crate::util::fs::strip_long_prefix(game_dir)
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
        .current_dir(spawn_dir(&prepared.game_dir))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // Консольное окно java.exe пугает пользователя («вирус»); окно самой
    // игры это не затрагивает — Minecraft рисует своё окно через JNI.
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command
        .spawn()
        .map_err(|e| LauncherError::JavaNotFound(format!("не удалось запустить игру: {e}")))?;
    lock.set_child_pid(child.id());
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
    use std::io::BufRead;
    let logs_dir = game_dir.join("logs");
    std::fs::create_dir_all(crate::util::fs::long_path(&logs_dir))?;
    let log_path = crate::util::fs::long_path(&logs_dir.join("latest.log"));
    // ENV-4: только создаём/усекаем свежий лог на запуск и СРАЗУ закрываем —
    // держать handle нельзя, его держит сама игра (см. write_game_line).
    drop(std::fs::File::create(&log_path)?);

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
                write_game_line(&log_path, bus, instance_id, &line, crate::events::LogStream::Stdout);
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

/// Установить загрузчик в инстанс (спека §6.3). Fabric/Quilt — профиль JSON
/// из meta; NeoForge — headless-инсталлятор. Инстанс обновляется и сохраняется.
pub async fn install_loader(
    paths: &Paths,
    settings: &crate::settings::Settings,
    client: Arc<HttpClient>,
    inst: &Instance,
    loader: &str,
    loader_version: Option<&str>,
) -> Result<Instance> {
    let mut updated = inst.clone();
    let instance_versions = crate::instances::instance_versions_dir(paths, &inst.id);
    std::fs::create_dir_all(crate::util::fs::long_path(&instance_versions))?;

    // Родительская версия MC должна быть в кэше (для inheritsFrom и Java).
    let manifest = fetch_manifest(&client, &paths.manifests_cache()).await?;
    let entry = resolve_entry(&manifest, &inst.mc_version)?;
    let parent = fetch_version_json_cached(&client, entry, &paths.manifests_cache()).await?;
    let need_major = parent
        .java_version
        .as_ref()
        .map(|j| j.major_version)
        .unwrap_or_else(|| fallback_major(&inst.mc_version));

    match loader {
        "fabric" => {
            let v = match loader_version {
                Some(v) => v.to_string(),
                None => crate::loaders::fabric::loader_versions(&client)
                    .await?
                    .into_iter()
                    .find(|l| l.stable)
                    .ok_or_else(|| LauncherError::NotFound("стабильный Fabric Loader".into()))?
                    .version,
            };
            let id = crate::loaders::fabric::install(&client, &instance_versions, &inst.mc_version, &v).await?;
            updated.loader = Some("fabric".into());
            updated.loader_version = Some(v);
            updated.version_id = Some(id);
        }
        "quilt" => {
            let v = match loader_version {
                Some(v) => v.to_string(),
                None => crate::loaders::quilt::loader_versions(&client)
                    .await?
                    .into_iter()
                    .find(|l| l.stable)
                    .ok_or_else(|| LauncherError::NotFound("стабильный Quilt Loader".into()))?
                    .version,
            };
            let id = crate::loaders::quilt::install(&client, &instance_versions, &inst.mc_version, &v).await?;
            updated.loader = Some("quilt".into());
            updated.loader_version = Some(v);
            updated.version_id = Some(id);
        }
        "forge" => {
            let v = match loader_version {
                Some(v) => v.to_string(),
                None => crate::loaders::forge::latest_for_mc(&client, &inst.mc_version).await?,
            };
            let java = resolve_java(paths, settings, client.clone(), need_major, &None).await?;
            let game_dir = minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
            std::fs::create_dir_all(crate::util::fs::long_path(&game_dir))?;
            let id = crate::loaders::forge::install(
                &client,
                &paths.cache_dir(),
                &java.java_exe,
                &game_dir,
                &instance_versions,
                &inst.mc_version,
                &v,
            )
            .await?;
            updated.loader = Some("forge".into());
            updated.loader_version = Some(v);
            updated.version_id = Some(id);
        }
        "neoforge" => {
            let v = match loader_version {
                Some(v) => v.to_string(),
                None => crate::loaders::neoforge::latest_for_mc(&client, &inst.mc_version).await?,
            };
            let java = resolve_java(paths, settings, client.clone(), need_major, &None).await?;
            let game_dir = minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
            std::fs::create_dir_all(crate::util::fs::long_path(&game_dir))?;
            let id = crate::loaders::neoforge::install(
                &client,
                &paths.cache_dir(),
                &java.java_exe,
                &game_dir,
                &instance_versions,
                &v,
            )
            .await?;
            updated.loader = Some("neoforge".into());
            updated.loader_version = Some(v);
            updated.version_id = Some(id);
        }
        other => {
            return Err(LauncherError::InvalidInput(format!(
                "неизвестный загрузчик {other} (fabric|quilt|neoforge)"
            )));
        }
    }
    crate::instances::save(paths, &updated)?;
    Ok(updated)
}

/// Спавн игры из UI: дочерний процесс спавнится сразу с записью PID в `.lock`,
/// супервизия читает логи в фоне до выхода игры.
pub fn launch_detached(
    paths: &Paths,
    instance_id: &str,
    prepared: PreparedLaunch,
    bus: &EventBus,
    // F26: Some(имя инстанса) — включить Discord Rich Presence.
    discord_rpc: Option<String>,
) -> Result<()> {
    let inst_dir = crate::instances::instance_dir(paths, instance_id);
    let lock = InstanceLock::acquire(&inst_dir)?;

    let mut command = std::process::Command::new(&prepared.command.program);
    command
        .args(&prepared.command.args)
        .current_dir(spawn_dir(&prepared.game_dir))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // Консольное окно java.exe пугает пользователя («вирус»); окно самой
    // игры это не затрагивает — Minecraft рисует своё окно через JNI.
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let child = command
        .spawn()
        .map_err(|e| LauncherError::JavaNotFound(format!("не удалось запустить игру: {e}")))?;
    lock.set_child_pid(child.id());
    // PID в .lock: instance_kill / process_status читают отсюда.
    let _ = std::fs::write(
        crate::util::fs::long_path(&inst_dir.join(".lock")),
        child.id().to_string(),
    );
    tracing::info!("инстанс {instance_id}: игра запущена (PID {})", child.id());
    bus.emit(LauncherEvent::LaunchState {
        instance_id: instance_id.to_string(),
        phase: LaunchPhase::Running,
        exit_code: None,
    });

    let paths_clone = paths.clone();
    let id_clone = instance_id.to_string();
    let bus_clone = bus.clone();
    let game_dir = prepared.game_dir;
    let pid = child.id();
    // F26: Discord Rich Presence — фоновый поток, живёт пока процесс игры;
    // Discord не запущен → connect вернёт None и поток сразу закончится.
    if let Some(details) = discord_rpc {
        let details = details.chars().take(128).collect::<String>();
        std::thread::spawn(move || {
            let conn = match crate::process::discord::RpcConnection::connect(
                pid,
                crate::process::discord::DISCORD_CLIENT_ID,
            ) {
                Ok(Some(c)) => {
                    tracing::info!("discord rpc: подключено (pid {pid})");
                    c
                }
                Ok(None) => {
                    tracing::warn!("discord rpc: pipe Discord не найден — статус не отправляется");
                    return;
                }
                Err(e) => {
                    tracing::warn!("discord rpc: подключение не удалось: {e}");
                    return;
                }
            };
            let mut conn = conn;
            match conn.set_activity(&details) {
                Ok(()) => tracing::info!("discord rpc: статус «{details}» отправлен"),
                Err(e) => tracing::warn!("discord rpc: set_activity: {e}"),
            }
            // Обновление раз в минуту; игра вышла — pipe закрывается.
            loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                if !super::running_pid_alive(pid) {
                    break;
                }
                let _ = conn.set_activity(&details);
            }
            conn.close();
        });
    }
    std::thread::spawn(move || {
        supervise_detached(&paths_clone, &id_clone, child, &game_dir, lock, &bus_clone);
    });
    Ok(())
}

fn supervise_detached(
    paths: &Paths,
    instance_id: &str,
    mut child: std::process::Child,
    game_dir: &std::path::Path,
    lock: InstanceLock,
    bus: &EventBus,
) {
    let logs_dir = game_dir.join("logs");
    let _ = std::fs::create_dir_all(crate::util::fs::long_path(&logs_dir));
    let log_path = crate::util::fs::long_path(&logs_dir.join("latest.log"));
    // ENV-4: свежий лог на запуск создаём/усекаем и сразу закрываем
    // (best-effort, как раньше) — handle за супервизией не остаётся.
    let _ = std::fs::File::create(&log_path);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (tx, rx) = std::sync::mpsc::channel::<(String, crate::events::LogStream)>();
    let streams: Vec<(Option<Box<dyn std::io::Read + Send>>, crate::events::LogStream)> = vec![
        (stdout.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), crate::events::LogStream::Stdout),
        (stderr.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), crate::events::LogStream::Stderr),
    ];
    for (stream, kind) in streams {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let Some(stream) = stream else { return };
            let reader = std::io::BufReader::new(stream);
            for line in std::io::BufRead::lines(reader).map_while(std::result::Result::ok) {
                if tx.send((line, kind)).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);

    let started = std::time::Instant::now();
    // F30: «игра дошла до запуска» — тот же маркер меню, что в
    // supervise_to_menu. Краш ДО него считается падением запуска (инкремент
    // crash_count); достигнутый running или нулевой выход — сброс счётчика.
    let mut saw_running = false;
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok((line, stream)) => {
                write_game_line(&log_path, bus, instance_id, &line, stream);
                if !saw_running && line.contains("Sound engine started") {
                    saw_running = true;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(Some(status)) = child.try_wait() {
                    let code = status.code();
                    bus.emit(LauncherEvent::LaunchState {
                        instance_id: instance_id.to_string(),
                        phase: LaunchPhase::Exited,
                        exit_code: code,
                    });
                    // A28: статистика пишется атомарно под SAVE_LOCK,
                    // чтобы не затереть настройки, сохранённые из UI во время игры.
                    let _ = crate::instances::update(paths, instance_id, |inst| {
                        inst.last_played = Some(crate::instances::now_secs());
                        inst.launch_count += 1;
                        inst.play_seconds += started.elapsed().as_secs();
                        bump_crash_counter(inst, code, saw_running);
                    });
                    drop(lock);
                    return;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let exit_code = child.wait().ok().and_then(|s| s.code());
    bus.emit(LauncherEvent::LaunchState {
        instance_id: instance_id.to_string(),
        phase: LaunchPhase::Exited,
        exit_code,
    });
    // A28: атомарно под SAVE_LOCK (см. instances::update).
    let _ = crate::instances::update(paths, instance_id, |inst| {
        inst.last_played = Some(crate::instances::now_secs());
        inst.launch_count += 1;
        inst.play_seconds += started.elapsed().as_secs();
        bump_crash_counter(inst, exit_code, saw_running);
    });
    drop(lock);
}

/// F30: счётчик краш-лупа внутри атомарного `instances::update` (поле
/// `crash_count` читается/пишется только здесь, в замыкании под SAVE_LOCK).
/// Ненулевой (или неизвестный — `code()` дал None, аварийное завершение)
/// exit-код ДО достижения running — падение запуска: насыщающий инкремент.
/// Нулевой выход или достигнутый running — успешный запуск: сброс в 0.
fn bump_crash_counter(inst: &mut Instance, exit_code: Option<i32>, saw_running: bool) {
    let crashed = exit_code.is_none_or(|code| code != 0);
    if crashed && !saw_running {
        inst.crash_count = inst.crash_count.saturating_add(1);
    } else {
        inst.crash_count = 0;
    }
}

/// F30: безопасный режим после серии падений запуска: сброс пользовательских
/// JVM-флагов (частая причина «игра не стартует») и выключение включённых
/// шейдеров (переименование в `.disabled` через content::toggle — ТОЛЬКО
/// kind=Shader). Разрешение окна не трогаем: в Instance его нет, оно задаётся
/// глобально (settings.default_resolution). Возвращает список принятых мер
/// для показа в UI; сбой выключения отдельного шейдера не фатален (warn,
/// запуск не блокируем). `&Paths` обязателен: content::toggle переименовывает
/// файлы на диске инстанса. Вызывающая IPC-команда сохраняет инстанс и
/// запускает игру обычной launch-цепочкой.
pub(crate) fn apply_safe_mode(paths: &Paths, inst: &mut Instance) -> Vec<String> {
    let mut actions = Vec::new();
    if !inst.jvm_flags.is_empty() {
        inst.jvm_flags = Vec::new();
        actions.push("сброшены пользовательские JVM-флаги".to_string());
    }
    let manifest = crate::instances::load_content_manifest(paths, &inst.id);
    for file in safe_mode_shader_targets(&manifest) {
        match crate::instances::content::toggle(paths, &inst.id, &file) {
            Ok(_) => actions.push(format!("выключен шейдер: {file}")),
            Err(e) => {
                tracing::warn!("безопасный режим {}: шейдер {file} не выключен: {e}", inst.id);
            }
        }
    }
    actions
}

/// Чистая часть безопасного режима (тестируется без диска): какие включённые
/// шейдеры из манифеста надо выключить. Моды/ресурспаки/датапаки не трогаем.
fn safe_mode_shader_targets(manifest: &[crate::instances::ContentEntry]) -> Vec<String> {
    manifest
        .iter()
        .filter(|e| e.enabled && e.kind == crate::instances::ContentKind::Shader)
        .map(|e| e.file.clone())
        .collect()
}

/// Поддерживает ли версия MC quick play (F1): `--quickPlaySingleplayer` /
/// `--quickPlayMultiplayer` появились в 1.20. Парсим major.minor как
/// `fallback_major`: снапшоты («24w14a»), беты («b1.7.3») и пустая строка
/// не дают major=1 → false.
pub fn mc_supports_quick_play(mc_version: &str) -> bool {
    let parts: Vec<u32> = mc_version
        .split('.')
        .map(|p| p.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("0"))
        .map(|p| p.parse::<u32>().unwrap_or(0))
        .collect();
    let (major, minor) = (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
    );
    major == 1 && minor >= 20
}

/// Аргументы quick play (F1): пара «флаг + значение» для MC 1.20+, на старых
/// версиях — пусто. Если заданы обе цели, берём мир: валидация на IPC-границе
/// уже запрещает одновременные мир+сервер, здесь просто детерминированный
/// приоритет, чтобы никогда не ушёл битый набор аргументов.
pub fn quick_play_args(mc_version: &str, world: Option<&str>, server: Option<&str>) -> Vec<String> {
    if !mc_supports_quick_play(mc_version) {
        return Vec::new();
    }
    match (world, server) {
        (Some(w), _) => vec!["--quickPlaySingleplayer".into(), w.to_string()],
        (None, Some(s)) => vec!["--quickPlayMultiplayer".into(), s.to_string()],
        (None, None) => Vec::new(),
    }
}

/// Fallback Java-минимумов по версии MC (спека §6.4).
pub fn fallback_major(mc_version: &str) -> u32 {    let parts: Vec<u32> = mc_version
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::LogStream;

    /// A15-регресс: строка игры с зарегистрированным секретом не должна
    /// попадать ни в `latest.log`, ни в событие UI. Секрет уникален для теста —
    /// реестр redact глобальный на процесс (A43).
    #[test]
    fn game_lines_are_redacted_in_log_and_bus() {
        let secret = "A15-GAMELOG-SECRET-7c1f";
        crate::util::redact::register_secret(secret);

        let bus = EventBus::new(64);
        let mut rx = bus.subscribe();
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("latest.log");
        // ENV-4: файл write_game_line открывает сам (append на каждую строку).
        write_game_line(
            &log_path,
            &bus,
            "inst-1",
            &format!("Authorization: Bearer {secret}"),
            LogStream::Stdout,
        );

        let text = std::fs::read_to_string(&log_path).unwrap();
        assert!(!text.contains(secret), "секрет утёк в latest.log: {text}");
        assert!(text.contains("[REDACTED]"), "{text}");

        let LauncherEvent::GameLogLine {
            line, instance_id, ..
        } = rx.try_recv().unwrap()
        else {
            panic!("ожидалось событие GameLogLine");
        };
        assert_eq!(instance_id, "inst-1");
        assert!(!line.contains(secret), "секрет утёк в событие: {line}");
        assert!(line.contains("[REDACTED]"), "{line}");

        // ENV-4: файл лога может не открыться (здесь: путь — каталог) —
        // паники нет, событие в UI всё равно уходит и остаётся чистым.
        let bad_log = dir.path().join("log-dir");
        std::fs::create_dir(&bad_log).unwrap();
        write_game_line(
            &bad_log,
            &bus,
            "inst-2",
            &format!("token={secret}"),
            LogStream::Stderr,
        );
        let LauncherEvent::GameLogLine { line, .. } = rx.try_recv().unwrap() else {
            panic!("ожидалось событие GameLogLine");
        };
        assert!(!line.contains(secret) && line.contains("[REDACTED]"), "{line}");
    }

    /// A50: рабочий каталог дочернего процесса не может быть `\\?\`-путём
    /// (SetCurrentDirectoryW), поэтому префикс снимается перед спавном.
    #[cfg(windows)]
    #[test]
    fn spawn_dir_has_no_extended_prefix() {
        let mut deep = PathBuf::from(r"C:\");
        let seg = "abcdefghijklmnopqrstuvwxyz0123456789";
        for _ in 0..8 {
            deep.push(seg);
        }
        let lp = crate::util::fs::long_path(&deep);
        assert!(lp.to_string_lossy().starts_with(r"\\?\"));
        let dir = spawn_dir(&lp);
        assert!(
            !dir.to_string_lossy().starts_with(r"\\?\"),
            "префикс обязан быть снят: {}",
            dir.display()
        );
        assert_eq!(dir, deep);
    }

    /// A51: сбой подготовки не оставляет полураспакованные нативы и `.part`.
    #[test]
    fn prepare_cleanup_removes_natives_and_part_files() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("minecraft");
        let natives = game.join("bin").join("1.20.1");
        std::fs::create_dir_all(&natives).unwrap();
        std::fs::write(natives.join("libnatives.so"), b"x").unwrap();
        let mods = game.join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("a.jar.part"), b"partial").unwrap();
        std::fs::write(mods.join("keep.jar"), b"ok").unwrap();

        {
            // Ошибка подготовки: guard уходит из scope без disarm.
            let mut c = PrepareCleanup::new(game.clone());
            c.track_natives(natives.clone());
        }
        assert!(!natives.exists(), "нативы сбойной версии удалены");
        assert!(!mods.join("a.jar.part").exists(), ".part удалён");
        assert!(mods.join("keep.jar").exists(), "готовый контент не тронут");
    }

    /// A51: успешная подготовка (disarm) не должна ничего удалять.
    #[test]
    fn prepare_cleanup_disarmed_keeps_everything() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("minecraft");
        let natives = game.join("bin").join("1.20.1");
        std::fs::create_dir_all(&natives).unwrap();
        std::fs::write(natives.join("keep.so"), b"x").unwrap();
        {
            let mut c = PrepareCleanup::new(game.clone());
            c.track_natives(natives.clone());
            c.disarm();
        }
        assert!(natives.join("keep.so").exists(), "успех → уборки нет");
    }

    /// F1: quick play доступен с MC 1.20 (major.minor); снапшоты/беты/старые — нет.
    #[test]
    fn mc_supports_quick_play_versions() {
        assert!(!mc_supports_quick_play("1.19.4"), "1.19.4 ещё не умеет");
        assert!(mc_supports_quick_play("1.20"), "1.20 — первая с quick play");
        assert!(mc_supports_quick_play("1.21.5"), "1.21.5 умеет");
        assert!(!mc_supports_quick_play("1.7.10"), "1.7.10 не умеет");
        assert!(mc_supports_quick_play("1.20.1"), "патч-релиз 1.20.x умеет");
        assert!(!mc_supports_quick_play("24w14a"), "снапшот не парсится как 1.x");
        assert!(!mc_supports_quick_play("b1.7.3"), "бета не парсится");
        assert!(!mc_supports_quick_play(""), "пустая версия — не умеет");
    }

    /// F1: пары «флаг+значение» только для 1.20+; обе цели — приоритет мира.
    #[test]
    fn quick_play_args_matrix() {
        // Старая версия: ничего, даже с заданными опциями.
        assert!(quick_play_args("1.19.4", Some("w"), Some("h:1")).is_empty());
        assert!(quick_play_args("1.7.10", Some("w"), None).is_empty());
        // Ничего не задано — аргументов нет.
        assert!(quick_play_args("1.20.1", None, None).is_empty());
        // Мир.
        assert_eq!(
            quick_play_args("1.20", Some("My_World"), None),
            vec!["--quickPlaySingleplayer".to_string(), "My_World".to_string()]
        );
        // Сервер.
        assert_eq!(
            quick_play_args("1.21.5", None, Some("mc.example.com:25565")),
            vec![
                "--quickPlayMultiplayer".to_string(),
                "mc.example.com:25565".to_string()
            ]
        );
        // Обе заданы (валидатор это уже запрещает) — безопасный приоритет мира.
        assert_eq!(
            quick_play_args("1.20.1", Some("w"), Some("h:2")),
            vec!["--quickPlaySingleplayer".to_string(), "w".to_string()]
        );
    }

    /// F30: таблица истинности счётчика краш-лупа. Краш до running —
    /// инкремент (насыщающий); нулевой выход или достигнутый running — сброс;
    /// неизвестный код (code() = None) считается аварийным выходом.
    #[test]
    fn bump_crash_counter_truth_table() {
        let mut inst = Instance::new("crashy", "1.20.1");
        assert_eq!(inst.crash_count, 0);
        bump_crash_counter(&mut inst, Some(0), false);
        assert_eq!(inst.crash_count, 0, "нулевой выход — не краш");
        bump_crash_counter(&mut inst, Some(1), false);
        assert_eq!(inst.crash_count, 1, "ненулевой выход до running — краш");
        bump_crash_counter(&mut inst, None, false);
        assert_eq!(inst.crash_count, 2, "нет кода завершения = аварийный выход");
        bump_crash_counter(&mut inst, Some(-1), false);
        assert_eq!(inst.crash_count, 3);
        // Игра дошла до running — даже последующий краш не «падение запуска».
        bump_crash_counter(&mut inst, Some(1), true);
        assert_eq!(inst.crash_count, 0, "running достигнут — сброс");
        inst.crash_count = u32::MAX;
        bump_crash_counter(&mut inst, Some(1), false);
        assert_eq!(inst.crash_count, u32::MAX, "насыщение без переполнения");
    }

    /// F30: чистый выбор целей безопасного режима — только включённые шейдеры;
    /// выключенные шейдеры и прочий контент не трогаются.
    #[test]
    fn safe_mode_targets_only_enabled_shaders() {
        let entry = |kind: crate::instances::ContentKind, file: &str, enabled: bool| {
            crate::instances::ContentEntry {
                kind,
                file: file.into(),
                source: crate::instances::ContentSource::Local,
                project_id: None,
                version_id: None,
                sha1: None,
                url: None,
                enabled,
            }
        };
        let manifest = vec![
            entry(crate::instances::ContentKind::Shader, "shaderpacks/on.zip", true),
            entry(crate::instances::ContentKind::Shader, "shaderpacks/off.zip", false),
            entry(crate::instances::ContentKind::Mod, "mods/a.jar", true),
            entry(crate::instances::ContentKind::ResourcePack, "resourcepacks/r.zip", true),
            entry(crate::instances::ContentKind::Datapack, "datapacks/d.zip", true),
        ];
        assert_eq!(
            safe_mode_shader_targets(&manifest),
            vec!["shaderpacks/on.zip".to_string()],
            "только включённые шейдеры"
        );
    }

    fn safe_mode_paths() -> (tempfile::TempDir, crate::paths::Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        (dir, paths)
    }

    /// F30: безопасный режим на реальном инстансе: JVM-флаги сброшены,
    /// включённый шейдер переименован в `.disabled` и выключен в манифесте,
    /// принятые меры перечислены в ответе.
    #[test]
    fn apply_safe_mode_clears_flags_and_disables_shader() {
        let (_d, paths) = safe_mode_paths();
        let mut inst = Instance::new("crashy", "1.20.1");
        inst.jvm_flags = vec!["-Xmx4096M".into(), "-XX:+UseG1GC".into()];
        crate::instances::save(&paths, &inst).unwrap();
        let mc = crate::instances::minecraft_dir(&crate::instances::instance_dir(&paths, &inst.id));
        let long = |p: &std::path::Path| crate::util::fs::long_path(p);
        std::fs::create_dir_all(long(&mc.join("shaderpacks"))).unwrap();
        std::fs::write(long(&mc.join("shaderpacks/bsl.zip")), b"S").unwrap();
        let shader = crate::instances::ContentEntry {
            kind: crate::instances::ContentKind::Shader,
            file: "shaderpacks/bsl.zip".into(),
            source: crate::instances::ContentSource::Local,
            project_id: None,
            version_id: None,
            sha1: None,
            url: None,
            enabled: true,
        };
        crate::instances::save_content_manifest(&paths, &inst.id, &[shader]).unwrap();

        let actions = apply_safe_mode(&paths, &mut inst);
        assert!(inst.jvm_flags.is_empty(), "JVM-флаги сброшены");
        assert!(actions.iter().any(|a| a.contains("JVM")), "мера о флагах: {actions:?}");
        assert!(actions.iter().any(|a| a.contains("bsl.zip")), "мера о шейдере: {actions:?}");
        let manifest = crate::instances::load_content_manifest(&paths, &inst.id);
        assert!(!manifest[0].enabled, "запись шейдера выключена в манифесте");
        assert!(!long(&mc.join("shaderpacks/bsl.zip")).exists(), "активный файл переименован");
        assert!(long(&mc.join("shaderpacks/bsl.zip.disabled")).exists(), "файл в .disabled");
    }

    /// F30: файла шейдера нет на диске — toggle ошибается, ошибка глотается
    /// с warn, паники нет; меры по флагам всё равно применяются.
    #[test]
    fn apply_safe_mode_tolerates_missing_shader_file() {
        let (_d, paths) = safe_mode_paths();
        let mut inst = Instance::new("crashy", "1.20.1");
        inst.jvm_flags = vec!["-Xmx1024M".into()];
        crate::instances::save(&paths, &inst).unwrap();
        let shader = crate::instances::ContentEntry {
            kind: crate::instances::ContentKind::Shader,
            file: "shaderpacks/ghost.zip".into(),
            source: crate::instances::ContentSource::Local,
            project_id: None,
            version_id: None,
            sha1: None,
            url: None,
            enabled: true,
        };
        crate::instances::save_content_manifest(&paths, &inst.id, &[shader]).unwrap();

        let actions = apply_safe_mode(&paths, &mut inst);
        assert!(inst.jvm_flags.is_empty(), "флаги сброшены независимо от шейдеров");
        assert_eq!(actions.len(), 1, "мера только о флагах: {actions:?}");
        assert!(actions[0].contains("JVM"), "{actions:?}");
    }
}
