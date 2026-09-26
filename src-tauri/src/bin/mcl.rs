//! CLI-движок лаунчера (M1 proof-of-launch, спека §13 M1):
//! `mcl launch` скачивает версию Mojang и запускает игру до главного меню.
//! Оркестрация здесь; с M2 переехает в instances/run.rs, домены не меняются.

use clap::{Parser, Subcommand};
use mc_launcher_v2_lib::errors::{LauncherError, Result};
use mc_launcher_v2_lib::events::{EventBus, LauncherEvent};
use mc_launcher_v2_lib::java::detect::{pick_java, scan_all};
use mc_launcher_v2_lib::mojang::assets::{
    fetch_asset_index, materialize_virtual, needs_virtual, plan_asset_tasks,
};
use mc_launcher_v2_lib::mojang::client::{client_jar_path, plan_client_task};
use mc_launcher_v2_lib::mojang::launch::{build_command, LaunchCommand, LaunchValues};
use mc_launcher_v2_lib::mojang::libraries::{build_classpath, extract_native, plan_libraries};
use mc_launcher_v2_lib::mojang::manifest::{fetch_manifest, resolve_entry};
use mc_launcher_v2_lib::mojang::rules::OsContext;
use mc_launcher_v2_lib::mojang::version::VersionJson;
use mc_launcher_v2_lib::net::download::{DownloadEngine, DownloadTask};
use mc_launcher_v2_lib::net::http::HttpClient;
use mc_launcher_v2_lib::paths::Paths;
use mc_launcher_v2_lib::settings::Settings;
use mc_launcher_v2_lib::util::fs::atomic_write;
use mc_launcher_v2_lib::util::win::hide_console;
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "mcl", about = "CLI лаунчера mc-launcher-v2 (этап M1)")]
struct Cli {
    /// Каталог данных лаунчера (по умолчанию %LOCALAPPDATA%\mc-launcher-v2)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Скачать версию и запустить игру (доказательство конвейера M1)
    Launch {
        /// id версии или `latest`
        #[arg(long, default_value = "latest")]
        mc: String,
        /// Ник офлайн-профиля
        #[arg(long, default_value = "Player")]
        player: String,
        /// Куча JVM в МБ (по умолчанию из настроек)
        #[arg(long)]
        xmx_mb: Option<u32>,
        /// Только подготовить файлы, игру не запускать
        #[arg(long)]
        no_run: bool,
        /// Таймаут ожидания главного меню, секунд
        #[arg(long, default_value_t = 300)]
        menu_timeout: u64,
    },
    /// Показать найденные Java
    JavaScan,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    match runtime.block_on(run(cli)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ОШИБКА [{}]: {e}", e.code());
            if let Some(hint) = e.hint() {
                eprintln!("Подсказка: {hint}");
            }
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .try_init();

    // Разбираем Cli до match: поля cmd переезжают, data_dir нужен заранее.
    let Cli { data_dir, cmd } = cli;
    let paths = Paths::new(data_dir.unwrap_or_else(Paths::default_root));
    let settings = Settings::load(&paths.settings_file())?;

    match cmd {
        Cmd::JavaScan => cmd_java_scan(&paths, &settings).await,
        Cmd::Launch {
            mc,
            player,
            xmx_mb,
            no_run,
            menu_timeout,
        } => cmd_launch(&paths, &settings, &mc, &player, xmx_mb, no_run, menu_timeout).await,
    }
}

async fn cmd_java_scan(paths: &Paths, settings: &Settings) -> Result<()> {
    let list = scan_all(&paths.runtime_dir(), &settings.extra_java_paths).await;
    if list.is_empty() {
        println!("Java не найдена.");
        return Ok(());
    }
    for j in &list {
        println!(
            "{}  Java {} ({}-бит)  [{}]",
            j.java_exe.display(),
            j.major,
            j.arch_bits,
            j.origin
        );
    }
    Ok(())
}

async fn cmd_launch(
    paths: &Paths,
    settings: &Settings,
    mc: &str,
    player: &str,
    xmx_mb: Option<u32>,
    no_run: bool,
    menu_timeout: u64,
) -> Result<()> {
    let t0 = Instant::now();
    paths.ensure_dirs()?;
    let client = HttpClient::new(settings.proxy_url.as_deref())?;
    let client = std::sync::Arc::new(client);
    let bus = EventBus::default();
    let engine = DownloadEngine::new(
        client.clone(),
        settings.download_parallelism as usize,
        bus.clone(),
    );

    // Прогресс в stderr (агрегированный, из событий движка).
    let mut progress_rx = bus.subscribe();
    let progress_task = tokio::spawn(async move {
        let mut last_line = String::new();
        loop {
            match progress_rx.recv().await {
                Ok(LauncherEvent::DlProgress(p)) => {
                    let mb_s = p.bytes_per_sec / 1_048_576.0;
                    let line = format!(
                        "\r  загрузка: {}/{} файлов, {:.1} МБ/с, ETA {}с   ",
                        p.done_files, p.total_files, mb_s,
                        p.eta_secs.unwrap_or(0)
                    );
                    if line != last_line {
                        let _ = std::io::stderr().write_all(line.as_bytes());
                        let _ = std::io::stderr().flush();
                        last_line = line;
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    // 1. Манифест → версия → version JSON с проверкой sha1 (спека §6.1.1–6.1.2).
    let manifest = fetch_manifest(&client, &paths.manifests_cache()).await?;
    let entry = resolve_entry(&manifest, mc)?;
    println!("Версия: {} ({})", entry.id, entry.version_type);
    let vjson = fetch_version_json_cached(&client, entry, &paths.manifests_cache()).await?;

    // 2. inheritsFrom-резолвинг (для vanilla — тривиальный).
    let rv = VersionJson::resolve_chain(&paths.manifests_cache(), &vjson)?;

    // 3. Java: системная подходящей версии (спека §6.1.5; Adoptium — M2).
    let installs = scan_all(&paths.runtime_dir(), &settings.extra_java_paths).await;
    let need_major = rv
        .java_version
        .as_ref()
        .map(|j| j.major_version)
        .unwrap_or_else(|| fallback_major(&rv.id));
    let java = pick_java(need_major, &installs).ok_or_else(|| {
        LauncherError::JavaNotFound(format!(
            "нужна Java {need_major} (для {}), найдено: {}",
            rv.id,
            installs
                .iter()
                .map(|j| j.major.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })?;
    println!(
        "Java: {} ({} {}-бит)",
        java.java_exe.display(),
        java.major,
        java.arch_bits
    );
    let is_arm = std::env::consts::ARCH.contains("arm");
    let os_ctx = OsContext::current(java.arch_bits, is_arm);

    // 4. Планирование загрузок: клиент, библиотеки, нативы, ассеты, log-config.
    let game_dir = paths.cli_run_dir(&rv.id);
    let group = format!("launch:{}", rv.id);
    let mut tasks: Vec<DownloadTask> = vec![plan_client_task(&rv, &paths.clients_store(), &group)?];
    let libs = plan_libraries(&rv, &os_ctx, &BTreeMap::new())?;
    for l in &libs {
        tasks.push(DownloadTask {
            id: format!("lib:{}", l.artifact.rel_path),
            url: l.artifact.url.clone(),
            dest: paths.libraries_store().join(&l.artifact.rel_path),
            sha1: l.artifact.sha1.clone(),
            size: l.artifact.size,
            group: group.clone(),
            priority: 10,
        });
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
    let asset_index_ref = rv.asset_index.clone().ok_or_else(|| {
        LauncherError::InvalidInput(format!("в версии {} нет assetIndex", rv.id))
    })?;
    let index = fetch_asset_index(
        &client,
        &asset_index_ref.url,
        &asset_index_ref.sha1,
        &paths.cache_dir(),
    )
    .await?;
    let asset_tasks = plan_asset_tasks(&index, &paths.assets_store(), &group);
    println!(
        "К загрузке: клиент + {} библиотек + {} ассетов",
        libs.len(),
        asset_tasks.len()
    );
    tasks.extend(asset_tasks);
    if let Some(logging) = &rv.logging {
        if let Some(lc) = &logging.client {
            tasks.push(DownloadTask {
                id: "log-config".into(),
                url: lc.file.url.clone(),
                dest: paths.cache_dir().join("log_configs").join(&lc.file.id),
                sha1: Some(lc.file.sha1.clone()),
                size: lc.file.size,
                group: group.clone(),
                priority: 20,
            });
        }
    }

    // 5. Загрузка: очередь персистентна, файлы уже на диске с валидным хэшем —
    // пропускаются мгновенно (resume после рестарта, спека §4.4).
    engine.load_queue(&paths.queue_file())?;
    let added = engine.add_tasks(tasks);
    let _ = added; // дедуп может съесть часть задач (уже скачанные)
    engine.run().await?;
    engine.persist_queue(&paths.queue_file())?;
    progress_task.abort();
    let _ = std::io::stderr().write_all(b"\n");
    let st = engine.queue_state();
    if st.failed > 0 {
        for (url, reason) in &st.failed_items {
            eprintln!("  FAIL {url}: {reason}");
        }
        return Err(LauncherError::network(format!(
            "{failed} загрузок не удалось",
            failed = st.failed
        )));
    }

    // 6. Нативы: распаковка классификаторов (спека §6.1.6).
    let natives_dir = paths.natives_dir(&rv.id);
    std::fs::create_dir_all(crate_long(&natives_dir))?;
    let mut natives_count = 0usize;
    for l in &libs {
        if let Some(n) = &l.native {
            let zip_path = paths.libraries_store().join(&n.rel_path);
            if zip_path.exists() {
                let exclude = rv
                    .libraries
                    .iter()
                    .find(|lib| lib.name == l.name)
                    .and_then(|lib| lib.extract.clone())
                    .map(|e| e.exclude)
                    .unwrap_or_default();
                natives_count += extract_native(&zip_path, &natives_dir, &exclude)?;
            }
        }
    }
    println!("Нативы распакованы: {natives_count} файлов → {}", natives_dir.display());

    // 7. Ассеты virtual/legacy (спека §6.4).
    let assets_root = paths.root().join("assets");
    let game_assets = if needs_virtual(&rv.assets, &index) {
        let vd = paths.assets_virtual(&rv.assets);
        let n = materialize_virtual(&index, &paths.assets_store(), &vd)?;
        println!("Virtual-ассеты: {n} → {}", vd.display());
        Some(vd)
    } else {
        None
    };

    // 8. Команда запуска.
    let client_jar = client_jar_path(&rv, &paths.clients_store()).ok_or_else(|| {
        LauncherError::InvalidInput(format!("нет клиента для {}", rv.id))
    })?;
    let classpath = build_classpath(&libs, &paths.libraries_store(), &client_jar);
    let log_config_path = rv.logging.as_ref().and_then(|l| l.client.as_ref()).map(|lc| {
        paths.cache_dir().join("log_configs").join(&lc.file.id)
    });
    let values = LaunchValues {
        auth_player_name: player.to_string(),
        auth_uuid: mc_launcher_v2_lib::auth::offline::offline_uuid(player).to_string(),
        auth_access_token: "0".into(), // offline-профиль: token="0" (спека §6.8)
        user_type: "legacy".into(),
        version_name: rv.id.clone(),
        version_type: rv.version_type.clone().unwrap_or_else(|| "release".into()),
        game_directory: game_dir.clone(),
        assets_root,
        assets_index_name: asset_index_ref.id.clone(),
        game_assets,
        natives_directory: natives_dir,
        launcher_name: "mc-launcher-v2".into(),
        launcher_version: env!("CARGO_PKG_VERSION").into(),
        classpath,
        library_directory: paths.libraries_store(),
        resolution: settings.default_resolution.map(|r| (r.width, r.height)),
        log_config_path,
        jvm_extra: vec![format!("-Xmx{}M", xmx_mb.unwrap_or(settings.default_ram_mb))],
    };
    let cmd = build_command(&rv, &values, &os_ctx, &java.java_exe)?;
    println!("Подготовка заняла {:.1} с", t0.elapsed().as_secs_f64());

    if no_run {
        println!("Команда: {} {}", cmd.program.display(), cmd.args.join(" "));
        return Ok(());
    }

    // 9. Спавн и супервизия до главного меню.
    let ok = launch_and_wait(&cmd, &game_dir, menu_timeout)?;
    if ok {
        println!(
            "ДОКАЗАТЕЛЬСТВО M1: игра дошла до 'Sound engine started'; общее время {:.1} с",
            t0.elapsed().as_secs_f64()
        );
        Ok(())
    } else {
        Err(LauncherError::Internal(format!(
            "игра не дошла до главного меню за {menu_timeout} с — см. лог"
        )))
    }
}

fn crate_long(p: &Path) -> PathBuf {
    mc_launcher_v2_lib::util::fs::long_path(p)
}

/// Fallback Java-минимумов по версии MC (спека §6.4), если в JSON нет javaVersion.
fn fallback_major(mc_version: &str) -> u32 {
    let parse_parts = |v: &str| -> Vec<u32> {
        v.split('.')
            .map(|p| p.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("0"))
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect()
    };
    let parts = parse_parts(mc_version);
    let (major, minor) = (parts.first().copied().unwrap_or(0), parts.get(1).copied().unwrap_or(0));
    if major > 1 {
        return 21; // новые версионные схемы (25.x/26.x)
    }
    match (major, minor) {
        (1, m) if m >= 20 => 21, // 1.20.5+ по спеке; для 1.20.1–1.20.4 Java 21 совместима
        (1, m) if (17..=19).contains(&m) => 17,
        _ => 8,
    }
}

/// Version JSON: из кэша по sha1, иначе скачать с проверкой sha1 (спека §6.2).
async fn fetch_version_json_cached(
    client: &HttpClient,
    entry: &mc_launcher_v2_lib::mojang::manifest::ManifestVersion,
    cache_dir: &Path,
) -> Result<VersionJson> {
    let path = cache_dir.join(format!("{}.json", entry.id));
    let long = crate_long(&path);
    if long.exists() {
        let data = std::fs::read(&long)?;
        if mc_launcher_v2_lib::util::fs::sha1_bytes(&data) == entry.sha1 {
            return Ok(serde_json::from_slice(&data)?);
        }
        let _ = std::fs::remove_file(&long);
    }
    let v = VersionJson::fetch(client, &entry.url, &entry.sha1).await?;
    atomic_write(&path, &serde_json::to_vec_pretty(&v)?)?;
    Ok(v)
}

/// Спавн игры: stdout/stderr → лог-файл, ожидание маркера главного меню.
/// Игра запускается С окном (без CREATE_NO_WINDOW — это не вспомогательный процесс).
fn launch_and_wait(cmd: &LaunchCommand, game_dir: &Path, timeout_secs: u64) -> Result<bool> {
    let logs_dir = game_dir.join("logs");
    std::fs::create_dir_all(crate_long(&logs_dir))?;
    let log_path = crate_long(&logs_dir.join("latest.log"));

    let mut command = Command::new(&cmd.program);
    command
        .args(&cmd.args)
        .current_dir(crate_long(game_dir))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Игра — с окном: никаких CREATE_NO_WINDOW (это не вспомогательный процесс).
    let mut child = command.spawn().map_err(|e| {
        LauncherError::JavaNotFound(format!("не удалось запустить игру: {e}"))
    })?;
    println!("Игра запущена (PID {}). Лог: {}", child.id(), log_path.display());

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
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

    let mut log_file = std::fs::File::create(&log_path)?;
    let mut tail: VecDeque<String> = VecDeque::with_capacity(64);
    let start = Instant::now();
    let mut found = false;
    let mut atlas_line: Option<String> = None;

    loop {
        if start.elapsed() > Duration::from_secs(timeout_secs) {
            eprintln!("  таймаут ожидания главного меню");
            break;
        }
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(line) => {
                let _ = writeln!(log_file, "{line}");
                if line.contains("Sound engine started") {
                    found = true;
                }
                if atlas_line.is_none() && line.contains("minecraft:textures/atlas") {
                    atlas_line = Some(line.clone());
                }
                tail.push_back(line);
                if tail.len() > 64 {
                    tail.pop_front();
                }
                if found {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(status) = child.try_wait()? {
                    eprintln!("  игра завершилась раньше меню: {status}");
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    // Завершаем дерево процессов игры (мы — супервизор приёмки).
    let pid = child.id();
    let _ = child.kill();
    let _ = child.wait();
    kill_tree(pid);

    if found {
        if let Some(a) = &atlas_line {
            println!("  атласы: {a}");
        }
        println!("  --- хвост лога ---");
        for l in tail.iter().rev().take(5).rev() {
            println!("  {l}");
        }
    } else {
        println!("  --- хвост лога (последние 20 строк) ---");
        for l in tail.iter().rev().take(20).rev() {
            println!("  {l}");
        }
        println!("  полный лог: {}", log_path.display());
    }
    Ok(found)
}

/// Завершение дерева процессов (child java мог породить детей).
fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let mut c = Command::new("taskkill");
        c.args(["/F", "/T", "/PID", &pid.to_string()]);
        hide_console(&mut c);
        let _ = c.output();
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
    }
}
