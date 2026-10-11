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
use mc_launcher_v2_lib::util::win::hide_console;
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead as _, Write};
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
    /// Скачать JRE с Adoptium (спека §6.5)
    JavaInstall {
        #[arg(long)]
        major: u32,
    },
    /// Создать инстанс (M2)
    InstanceCreate {
        #[arg(long)]
        mc: String,
        #[arg(long, default_value = "Инстанс")]
        name: String,
        /// Куча в МБ
        #[arg(long, default_value_t = 2048)]
        ram: u32,
    },
    /// Список инстансов
    InstanceList,
    /// Дублировать инстанс (shared — hardlink, unique — копия)
    InstanceDuplicate { #[arg(long)] id: String },
    /// Удалить инстанс (в корзину ОС; --wipe — насовсем)
    InstanceDelete {
        #[arg(long)]
        id: String,
        #[arg(long)]
        wipe: bool,
    },
    /// Запустить инстанс (M2 приёмка)
    InstanceLaunch {
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "Player")]
        player: String,
        #[arg(long, default_value_t = 300)]
        menu_timeout: u64,
        /// Не ждать главного меню: спавн и выход (игра продолжает жить)
        #[arg(long)]
        no_wait: bool,
    },
    /// Версии загрузчика
    LoaderVersions {
        #[arg(long)]
        loader: String,
    },
    /// Установить загрузчик в инстанс (спека §6.3)
    LoaderInstall {
        #[arg(long)]
        id: String,
        /// fabric | quilt | neoforge
        #[arg(long)]
        loader: String,
        /// Версия загрузчика (по умолчанию — последняя стабильная)
        #[arg(long)]
        loader_version: Option<String>,
    },
    /// Установить модпак .mrpack в новый инстанс (спека §6.6)
    MrpackInstall {
        /// Путь к .mrpack файлу
        #[arg(long)]
        file: String,
        /// Имя инстанса (по умолчанию из модпака)
        #[arg(long)]
        name: Option<String>,
    },
    /// Поиск по Modrinth (отладка API)
    ModrinthSearch {
        #[arg(long)]
        query: String,
        #[arg(long)]
        mc: Option<String>,
        #[arg(long)]
        loader: Option<String>,
    },
    /// Добавить офлайн-профиль в реестр аккаунтов
    AuthOffline {
        #[arg(long)]
        nick: String,
    },
    /// Начать MSA device-code вход (код + URL для браузера)
    AuthMsaStart,
    /// Вход через браузер (auth-code + localhost redirect) — рекомендуется
    AuthMsaWeb,
    /// Дописать MSA-вход после авторизации в браузере
    AuthMsaPoll {
        #[arg(long)]
        device_code: String,
    },
    /// Список аккаунтов
    AuthList,
    /// Сделать аккаунт активным
    AuthActive {
        #[arg(long)]
        id: String,
    },
    /// Импорт архива (CF-zip / MultiMC / свой формат) в новый инстанс
    InstanceImport {
        #[arg(long)]
        file: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Краш-анализ лога
    CrashAnalyze {
        /// Путь к файлу лога
        #[arg(long)]
        log: String,
    },
    /// Очистить кэш стора от объектов без ссылок
    StoreCleanup,
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
            // LOC#2: код подсказки → короткий текст (CLI — служебная
            // утилита владельца, язык один).
            let hint_text: Option<&str> = match e.hint_code() {
                Some("hash_retry") => Some("проверьте соединение или зеркало"),
                Some("java_settings") => {
                    Some("установите Java в настройках лаунчера или укажите путь вручную")
                }
                Some("instance_running") => Some("остановите игру этого инстанса и повторите"),
                Some("offline_toggle") => Some("выключите «Работать офлайн», чтобы качать из сети"),
                Some("cert_time") => Some("проверьте дату/время на компьютере"),
                _ => None,
            };
            if let Some(hint) = hint_text {
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
        Cmd::JavaInstall { major } => cmd_java_install(&paths, &settings, major).await,
        Cmd::Launch {
            mc,
            player,
            xmx_mb,
            no_run,
            menu_timeout,
        } => cmd_launch(&paths, &settings, &mc, &player, xmx_mb, no_run, menu_timeout).await,
        Cmd::InstanceCreate { mc, name, ram } => cmd_instance_create(&paths, &settings, &mc, &name, ram).await,
        Cmd::InstanceList => cmd_instance_list(&paths),
        Cmd::InstanceDuplicate { id } => cmd_instance_duplicate(&paths, &id),
        Cmd::InstanceDelete { id, wipe } => cmd_instance_delete(&paths, &id, wipe),
        Cmd::InstanceLaunch {
            id,
            player,
            menu_timeout,
            no_wait,
        } => cmd_instance_launch(&paths, &settings, &id, &player, menu_timeout, no_wait).await,
        Cmd::LoaderVersions { loader } => cmd_loader_versions(&paths, &settings, &loader).await,
        Cmd::LoaderInstall {
            id,
            loader,
            loader_version,
        } => cmd_loader_install(&paths, &settings, &id, &loader, loader_version.as_deref()).await,
        Cmd::MrpackInstall { file, name } => {
            let client = std::sync::Arc::new(make_client(&settings)?);
            // У CLI шины нет — движок со своим EventBus (как в остальных командах mcl).
            let engine = DownloadEngine::new(
                client.clone(),
                settings.download_parallelism as usize,
                EventBus::default(),
            );
            let pack_path = Path::new(&file);
            // Ключ группы — как у GUI: имя файла (id пака до парсинга неизвестен).
            let group = format!(
                "mrpack:{}",
                pack_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            );
            let inst = mc_launcher_v2_lib::modrinth::mrpack::install_mrpack(
                &paths,
                &settings,
                engine,
                client,
                &group,
                pack_path,
                name.as_deref(),
            )
            .await?;
            println!(
                "Модпак установлен: {} [{}] id={}",
                inst.name, inst.mc_version, inst.id
            );
            Ok(())
        }
        Cmd::ModrinthSearch { query, mc, loader } => {
            let client = make_client(&settings)?;
            let r = mc_launcher_v2_lib::modrinth::api::search(
                &client,
                &query,
                mc.as_deref(),
                loader.as_deref(),
                Some("mod"),
                None,
                None,
                10,
                0,
            )
            .await?;
            for h in &r.hits {
                println!("{} | {} | {} скач. | {}", h.title, h.slug, h.downloads, h.project_id);
            }
            println!("всего: {}", r.total_hits);
            Ok(())
        }
        Cmd::AuthOffline { nick } => {
            // D62: повторный вызов с тем же ником не должен плодить дубли —
            // CLI генерировал новый uuid на каждый запуск и получал N
            // одинаковых офлайн-аккаунтов. Ищем существующий по name+kind.
            if let Some(acc) = mc_launcher_v2_lib::auth::list(&paths).into_iter().find(|a| {
                a.kind == mc_launcher_v2_lib::auth::AccountKind::Offline && a.name == nick
            }) {
                println!("Офлайн-профиль {nick} уже добавлен (id {})", acc.id);
                return Ok(());
            }
            let uuid = mc_launcher_v2_lib::auth::offline::offline_uuid(&nick).to_string();
            let acc = mc_launcher_v2_lib::auth::Account {
                id: uuid::Uuid::new_v4().to_string(),
                kind: mc_launcher_v2_lib::auth::AccountKind::Offline,
                name: nick.clone(),
                uuid,
                refresh_ref: None,
                authlib_server: None,
            };
            mc_launcher_v2_lib::auth::add(&paths, acc)?;
            println!("Офлайн-профиль {nick} добавлен");
            Ok(())
        }
        Cmd::AuthMsaStart => {
            let client = make_client(&settings)?;
            let client_id = settings.azure_client_id.clone().ok_or_else(|| {
                LauncherError::InvalidInput(
                    "нет azureClientId в settings.json (инструкция в README, M8)".into(),
                )
            })?;
            let start = mc_launcher_v2_lib::auth::msa::device_code_start(&client, &client_id).await?;
            println!("1) Откройте: {}", start.verification_url);
            println!("2) Введите код: {}", start.user_code);
            println!("device_code: {}", start.device_code);
            println!("3) Затем: mcl auth-msa-poll --device-code <значение_device_code>");
            Ok(())
        }
        Cmd::AuthMsaPoll { device_code } => {
            let client = std::sync::Arc::new(make_client(&settings)?);
            let client_id = settings.azure_client_id.clone().ok_or_else(|| {
                LauncherError::InvalidInput("нет azureClientId в settings.json".into())
            })?;
            for _ in 0..30 {
                match mc_launcher_v2_lib::auth::msa::device_code_poll(&client, &client_id, &device_code).await? {
                    Some(session) => {
                        let ref_name = format!("msa-refresh-{}", uuid::Uuid::new_v4());
                        mc_launcher_v2_lib::auth::keyring_set(&ref_name, &session.refresh_token)?;
                        let acc = mc_launcher_v2_lib::auth::Account {
                            // D64: id = uuid профиля из сессии — стабилен между
                            // перелогинами (раньше каждый вход плодил новый
                            // MSA-аккаунт: auth::add дедуплицирует по id).
                            id: session.uuid.clone(),
                            kind: mc_launcher_v2_lib::auth::AccountKind::Msa,
                            name: session.player_name.clone(),
                            uuid: session.uuid.clone(),
                            refresh_ref: Some(ref_name),
                            authlib_server: None,
                        };
                        println!("Вход выполнен: {} ({})", session.player_name, session.uuid);
                        mc_launcher_v2_lib::auth::add(&paths, acc)?;
                        return Ok(());
                    }
                    None => tokio::time::sleep(std::time::Duration::from_secs(3)).await,
                }
            }
            // D62: 30 итераций (~90 с) без завершения — это провал, а не успех:
            // exit 0 обманывал скрипты-обёртки, проверяющие код возврата.
            eprintln!("авторизация не завершена — повторите poll");
            std::process::exit(2);
        }
        Cmd::AuthMsaWeb => {
            let client = std::sync::Arc::new(make_client(&settings)?);
            let client_id = settings.azure_client_id.clone().ok_or_else(|| {
                LauncherError::InvalidInput("нет azureClientId в settings.json".into())
            })?;
            let listener = std::net::TcpListener::bind("127.0.0.1:0")
                .map_err(|e| LauncherError::network(format!("bind: {e}")))?;
            let port = listener.local_addr()?.port();
            // CSRF-защита: листнер принимает только redirect с этим state.
            let state = uuid::Uuid::new_v4().to_string();
            let url = mc_launcher_v2_lib::auth::msa::authorize_url(&client_id, port, &state);
            println!("Открываю браузер для входа (localhost:{port})...");
            // D62: URL печатаем в stdout ДО попытки открыть браузер — если окно
            // не появилось или explorer недоступен, ссылка уже видна и её можно
            // открыть вручную.
            println!("Открой в браузере, если окно не появилось: {url}");
            if let Err(e) = std::process::Command::new("explorer").arg(&url).spawn() {
                // D62: молча проглоченная ошибка спавна оставляла пользователя
                // ждать ответа, который никогда не придёт.
                eprintln!("Не удалось открыть браузер: {e} — открой ссылку выше вручную");
            }
            // D62: accept_timeout 600 с — разумный предел на ручной вход;
            // без него листнер висел бы вечно, а скрипт-обёртка никогда не
            // возвращался бы.
            let code =
                match mc_launcher_v2_lib::auth::msa::wait_auth_code(listener, state, Duration::from_secs(600))
                    .await
                {
                    Ok(code) => code,
                    // D62: таймаут/ошибка ожидания — человекочитаемое пояснение,
                    // а не только голый код из main.
                    Err(e) => {
                        eprintln!("Вход через браузер не завершён за 600 с: {e}");
                        return Err(e);
                    }
                };
            let session = mc_launcher_v2_lib::auth::msa::finish_web_login(&client, &client_id, &code, port).await?;
            let ref_name = format!("msa-refresh-{}", uuid::Uuid::new_v4());
            mc_launcher_v2_lib::auth::keyring_set(&ref_name, &session.refresh_token)?;
            let acc = mc_launcher_v2_lib::auth::Account {
                // D64: id = uuid профиля из сессии — стабилен между
                // перелогинами (см. комментарий в auth-msa-poll).
                id: session.uuid.clone(),
                kind: mc_launcher_v2_lib::auth::AccountKind::Msa,
                name: session.player_name.clone(),
                uuid: session.uuid.clone(),
                refresh_ref: Some(ref_name),
                authlib_server: None,
            };
            println!("Вход выполнен: {} ({})", session.player_name, session.uuid);
            mc_launcher_v2_lib::auth::add(&paths, acc)?;
            Ok(())
        }
        Cmd::AuthList => {
            for a in mc_launcher_v2_lib::auth::list(&paths) {
                println!("{} [{}] {} ({}) id={}", a.name, a.kind_label(), a.uuid, a.refresh_ref.as_deref().unwrap_or("без keyring"), a.id);
            }
            Ok(())
        }
        Cmd::AuthActive { id } => {
            mc_launcher_v2_lib::auth::set_active(&paths, &id)?;
            println!("Активный аккаунт: {id}");
            Ok(())
        }
        Cmd::InstanceImport { file, name } => {
            let client = std::sync::Arc::new(make_client(&settings)?);
            let (inst, loader) = mc_launcher_v2_lib::import::import_archive(
                &paths,
                &settings,
                client,
                std::path::Path::new(&file),
                name.as_deref(),
            )
            .await?;
            println!("Импортирован: {} [{}] id={}", inst.name, inst.mc_version, inst.id);
            // D62: установку загрузчика теперь делает сам import_archive
            // (внутри транзакции с откатом; D64: при офлайн-клиенте пак с
            // загрузчиком — честный OfflineMode-отказ с откатом импорта).
            // Дублирующий install_loader в CLI убран — оставлена только
            // обработка результата.
            if let Some((kind, ver)) = loader {
                println!("Загрузчик {kind} {ver} установлен");
            }
            Ok(())
        }
        Cmd::CrashAnalyze { log } => {
            let text = crash_log_tail(&log)?;
            let d = mc_launcher_v2_lib::process::crash::analyze(&text);
            if d.is_empty() {
                println!("Известных причин не найдено");
            }
            for x in &d {
                println!("[{}] {}: {}", x.rule_id, x.title, x.advice);
            }
            Ok(())
        }
        Cmd::StoreCleanup => {
            let n = mc_launcher_v2_lib::instances::store::cleanup_store(&paths)?;
            println!("Удалено объектов без ссылок: {n}");
            Ok(())
        }
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
    let client = std::sync::Arc::new(make_client(settings)?);
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
    let vjson = mc_launcher_v2_lib::mojang::manifest::fetch_version_json_cached(&client, entry, &paths.manifests_cache()).await?;

    // 2. inheritsFrom-резолвинг (для vanilla — тривиальный).
    let rv = VersionJson::resolve_chain(&paths.manifests_cache(), &vjson)?;

    // 3. Java: системная подходящей версии (спека §6.1.5; Adoptium — M2).
    let installs = scan_all(&paths.runtime_dir(), &settings.extra_java_paths).await;
    let need_major = rv
        .java_version
        .as_ref()
        .map(|j| j.major_version)
        .unwrap_or_else(|| mc_launcher_v2_lib::instances::run::fallback_major(&rv.id));
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
    let asset_index_ref = rv.asset_index.clone().ok_or_else(|| {
        LauncherError::InvalidInput(format!("в версии {} нет assetIndex", rv.id))
    })?;
    let index = fetch_asset_index(
        &client,
        &asset_index_ref.url,
        &asset_index_ref.sha1,
        &asset_index_ref.id,
        &paths.assets_indexes(),
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
                sha512: None,
                size: lc.file.size,
                group: group.clone(),
                priority: 20,
            });
        }
    }

    // 5. Загрузка: движок — на план этого запуска; валидные файлы на диске
    // пропускаются мгновенно (resume, спека §4.4).
    engine.add_tasks(tasks);
    engine.run().await?;
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
    let assets_root = paths.assets_dir();
    let game_assets = if needs_virtual(&rv.assets, &index) {
        let vd = paths.assets_virtual(&rv.assets);
        let n = materialize_virtual(&index, &paths.assets_objects(), &vd)?;
        println!("Virtual-ассеты: {n} → {}", vd.display());
        Some(vd)
    } else {
        None
    };

    // 8. Команда запуска.
    let client_jar = client_jar_path(&rv, &paths.clients_store()).ok_or_else(|| {
        LauncherError::InvalidInput(format!("нет клиента для {}", rv.id))
    })?;
    let classpath = build_classpath(&libs, &paths.libraries_store(), &client_jar, &[]);
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
            game_args_extra: Vec::new(),
    };
    let cmd = build_command(&rv, &values, &os_ctx, &java.java_exe)?;
    println!("Подготовка заняла {:.1} с", t0.elapsed().as_secs_f64());

    if no_run {
        // D62: join(" ") ломал аргументы с пробелами (classpath под Windows —
        // один аргумент через «;», но ники и пути бывают с пробелами) —
        // для показа квотируем такие аргументы.
        let shown = std::iter::once(cmd.program.display().to_string())
            .chain(cmd.args.iter().map(|a| quote_for_display(a)))
            .collect::<Vec<_>>()
            .join(" ");
        println!("Команда: {shown}");
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

/// D64: CLI уважает work_offline из settings.json — тумблер применяется при
/// старте к каждому клиенту (как AppState::new в GUI, commands/mod.rs).
/// Прежде CLI ходил в сеть даже при включённом «Работать офлайн».
fn make_client(settings: &Settings) -> Result<HttpClient> {
    let client = HttpClient::new(settings.proxy_url.as_deref())?;
    client.set_offline(settings.work_offline);
    Ok(client)
}

/// D62: простая эвристика для ЧЕЛОВЕЧЕСКОГО показа команды (не для исполнения):
/// аргумент с пробелом или кавычкой берём в кавычки, внутренние кавычки
/// экранируем. Пустой аргумент тоже квотируем, иначе он теряется в показе.
fn quote_for_display(arg: &str) -> String {
    if arg.is_empty() || arg.contains(' ') || arg.contains('"') {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

async fn cmd_java_install(paths: &Paths, settings: &Settings, major: u32) -> Result<()> {
    let client = make_client(settings)?;
    println!("Скачиваю JRE {major} с Adoptium…");
    let java = mc_launcher_v2_lib::java::adoptium::install_jre(&client, &paths.runtime_dir(), major, |done, total| {
        if total > 0 {
            eprint!("\r  {done}/{total} байт   ");
        }
    })
    .await?;
    let _ = std::io::stderr().write_all(b"\n");
    let inst = mc_launcher_v2_lib::java::detect::inspect(&java, "adoptium").await?;
    println!("Готово: {} (Java {} {}-бит)", java.display(), inst.major, inst.arch_bits);
    Ok(())
}

fn find_instance(paths: &Paths, id_or_name: &str) -> Result<mc_launcher_v2_lib::instances::Instance> {
    let all = mc_launcher_v2_lib::instances::list(paths);
    // Точный id однозначен по определению (id уникальны) — приоритет.
    if let Some(i) = all.iter().find(|i| i.id == id_or_name) {
        return Ok(i.clone());
    }
    // D62: eq_ignore_ascii_case не работает для кириллицы (дефолтное имя
    // «Инстанс») — сравниваем через to_lowercase(), он юникодный и для ASCII,
    // и для не-ASCII.
    let needle = id_or_name.to_lowercase();
    let matches: Vec<&mc_launcher_v2_lib::instances::Instance> = all
        .iter()
        .filter(|i| i.name.to_lowercase() == needle || i.id.starts_with(id_or_name))
        .collect();
    match matches.as_slice() {
        [] => Err(LauncherError::NotFound(format!("инстанс {id_or_name}"))),
        [only] => Ok((*only).clone()),
        // D62: неоднозначный префикс --id раньше молча брал первого кандидата —
        // теперь возвращаем ошибку со списком, кого имели в виду.
        many => Err(LauncherError::InvalidInput(format!(
            "инстанс {id_or_name:?} — неоднозначно, уточните: {}",
            many.iter().map(|i| i.id.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

async fn cmd_instance_create(
    paths: &Paths,
    settings: &Settings,
    mc: &str,
    name: &str,
    ram: u32,
) -> Result<()> {
    let client = make_client(settings)?;
    // Проверяем, что версия существует в манифесте (сеть нужна один раз).
    let manifest = mc_launcher_v2_lib::mojang::manifest::fetch_manifest(&client, &paths.manifests_cache()).await?;
    mc_launcher_v2_lib::mojang::manifest::resolve_entry(&manifest, mc)?;
    let mut inst = mc_launcher_v2_lib::instances::Instance::new(name, mc);
    inst.ram_mb = ram;
    mc_launcher_v2_lib::instances::save(paths, &inst)?;
    println!("Инстанс создан: {} ({}) id={}", inst.name, inst.mc_version, inst.id);
    Ok(())
}

async fn cmd_loader_versions(_paths: &Paths, settings: &Settings, loader: &str) -> Result<()> {
    let client = make_client(settings)?;
    match loader {
        "fabric" => {
            for l in mc_launcher_v2_lib::loaders::fabric::loader_versions(&client)
                .await?
                .iter()
                .take(10)
            {
                println!("{}{}", l.version, if l.stable { "" } else { "  (нестабильная)" });
            }
        }
        "quilt" => {
            for l in mc_launcher_v2_lib::loaders::quilt::loader_versions(&client)
                .await?
                .iter()
                .take(10)
            {
                println!("{}{}", l.version, if l.stable { "" } else { "  (нестабильная)" });
            }
        }
        "neoforge" => {
            for v in mc_launcher_v2_lib::loaders::neoforge::versions(&client).await? {
                println!("{v}");
            }
        }
        other => {
            return Err(LauncherError::InvalidInput(format!(
                "неизвестный загрузчик {other}"
            )))
        }
    }
    Ok(())
}

async fn cmd_loader_install(
    paths: &Paths,
    settings: &Settings,
    id: &str,
    loader: &str,
    loader_version: Option<&str>,
) -> Result<()> {
    let inst = find_instance(paths, id)?;
    let client = std::sync::Arc::new(make_client(settings)?);
    let updated = mc_launcher_v2_lib::instances::run::install_loader(
        paths,
        settings,
        client,
        &inst,
        loader,
        loader_version,
    )
    .await?;
    println!(
        "Загрузчик {}: версия {}, version_id = {}",
        loader,
        updated.loader_version.as_deref().unwrap_or("?"),
        updated.version_id.as_deref().unwrap_or("?")
    );
    Ok(())
}

fn cmd_instance_list(paths: &Paths) -> Result<()> {
    let all = mc_launcher_v2_lib::instances::list(paths);
    if all.is_empty() {
        println!("Инстансов нет.");
        return Ok(());
    }
    for i in &all {
        println!(
            "{}  [{}] mc={} loader={:?} ram={}МБ запусков={}",
            i.name,
            i.id,
            i.mc_version,
            i.loader.as_deref().unwrap_or("vanilla"),
            i.ram_mb,
            i.launch_count
        );
    }
    Ok(())
}

fn cmd_instance_duplicate(paths: &Paths, id: &str) -> Result<()> {
    let found = find_instance(paths, id)?;
    let copy = mc_launcher_v2_lib::instances::duplicate(paths, &found.id)?;
    println!("Дубликат: {} id={}", copy.name, copy.id);
    Ok(())
}

fn cmd_instance_delete(paths: &Paths, id: &str, wipe: bool) -> Result<()> {
    let found = find_instance(paths, id)?;
    mc_launcher_v2_lib::instances::delete(paths, &found.id, wipe)?;
    println!(
        "Инстанс {} удалён {}",
        found.name,
        if wipe { "насовсем" } else { "в корзину ОС" }
    );
    Ok(())
}

async fn cmd_instance_launch(
    paths: &Paths,
    settings: &Settings,
    id: &str,
    player: &str,
    menu_timeout: u64,
    no_wait: bool,
) -> Result<()> {
    let t0 = Instant::now();
    let inst = find_instance(paths, id)?;
    let client = std::sync::Arc::new(make_client(settings)?);
    let bus = EventBus::default();
    let engine = DownloadEngine::new(
        client.clone(),
        settings.download_parallelism as usize,
        bus.clone(),
    );

    let mut progress_rx = bus.subscribe();
    let progress_task = tokio::spawn(async move {
        loop {
            match progress_rx.recv().await {
                Ok(LauncherEvent::DlProgress(p)) => {
                    let mb_s = p.bytes_per_sec / 1_048_576.0;
                    let line = format!(
                        "\r  загрузка: {}/{} файлов, {:.1} МБ/с   ",
                        p.done_files, p.total_files, mb_s
                    );
                    let _ = std::io::stderr().write_all(line.as_bytes());
                    let _ = std::io::stderr().flush();
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    // Идентичность активного аккаунта (offline/MSA/ely) — спека §6.8.
    // Нет активного аккаунта → офлайн с указанным ником (token=0).
    let identity = if mc_launcher_v2_lib::auth::active(paths).is_none() {
        mc_launcher_v2_lib::auth::offline::identity(player)
    } else {
        mc_launcher_v2_lib::auth::launch_identity(paths, &client).await?
    };
    let javaagent = match mc_launcher_v2_lib::auth::active(paths).map(|a| a.kind) {
        Some(mc_launcher_v2_lib::auth::AccountKind::Ely) => {
            let jar = mc_launcher_v2_lib::auth::ely::authlib_injector_jar(&client, &paths.cache_dir()).await?;
            Some(mc_launcher_v2_lib::auth::ely::javaagent_arg(&jar))
        }
        _ => None,
    };
    let prepared = mc_launcher_v2_lib::instances::run::prepare(
        paths,
        settings,
        client,
        engine,
        &bus,
        &inst,
        mc_launcher_v2_lib::instances::run::AccountIdentity {
            player_name: identity.player_name,
            uuid: identity.uuid,
            access_token: identity.access_token,
            user_type: identity.user_type,
            javaagent,
        },
    )
    .await?;
    progress_task.abort();
    let _ = std::io::stderr().write_all(b"\n");
    println!(
        "Подготовка {:.1} с; Java {} ({}); версия {}",
        t0.elapsed().as_secs_f64(),
        prepared.java.major,
        prepared.java.arch_bits,
        prepared.version_id
    );
    if no_wait {
        // Спавн без супервизии: закрытие mcl игру не убивает (спека §6.10).
        let mut command = std::process::Command::new(&prepared.command.program);
        command
            .args(&prepared.command.args)
            .current_dir(crate_long(&prepared.game_dir));
        let child = command.spawn()?;
        println!(
            "Игра запущена в отрыве (PID {}). Закрытие mcl её не убьёт.",
            child.id()
        );
        return Ok(());
    }

    let found = mc_launcher_v2_lib::instances::run::launch_blocking(
        paths,
        &inst,
        prepared,
        &bus,
        menu_timeout,
        true,
    )?;
    if found {
        println!(
            "ДОКАЗАТЕЛЬСТВО: игра из инстанса дошла до 'Sound engine started'; общее время {:.1} с",
            t0.elapsed().as_secs_f64()
        );
        Ok(())
    } else {
        Err(LauncherError::Internal(format!(
            "игра не дошла до главного меню за {menu_timeout} с"
        )))
    }
}



/// D64: декод строки из потока игры — по образцу `lossy_line` в
/// instances/run.rs (не копируем из libcrate — там он приватный): не-UTF-8
/// байты → U+FFFD вместо обрыва чтения, хвостовой \r от CRLF отрезается
/// (split(b'\n') его оставляет).
fn lossy_line(buf: &[u8]) -> String {
    let mut line = String::from_utf8_lossy(buf).into_owned();
    while line.ends_with('\r') || line.ends_with('\n') {
        line.pop();
    }
    line
}

/// Спавн игры: stdout/stderr → лог-файл, ожидание маркера главного меню.
/// Игра запускается С окном (без CREATE_NO_WINDOW — это не вспомогательный процесс).
fn launch_and_wait(cmd: &LaunchCommand, game_dir: &Path, timeout_secs: u64) -> Result<bool> {
    let logs_dir = game_dir.join("logs");
    std::fs::create_dir_all(crate_long(&logs_dir))?;
    // D62: latest.log уже открыт самой игрой (Log4j держит хэндл и ротирует) —
    // писать в него из CLI нельзя: задвоение строк и «дыры» вокруг ротации,
    // а ведь именно его печатаем как «полный лог» и советуем для crash-analyze.
    // Свой строчный лог — в отдельный файл, который принадлежит только нам.
    let cli_log = crate_long(&logs_dir.join("mcl-launch.log"));
    let latest_log = crate_long(&logs_dir.join("latest.log"));

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
    println!("Игра запущена (PID {}). Лог: {}", child.id(), cli_log.display());

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let streams: Vec<Box<dyn std::io::Read + Send>> = vec![Box::new(stdout), Box::new(stderr)];
    for stream in streams {
        let tx = tx.clone();
        std::thread::spawn(move || {
            // D64: `lines().map_while(Result::ok)` обрывал поток на первом
            // не-UTF-8 байте — нить умирала молча, хвост лога терялся.
            // Построчное чтение байтов с lossy-декодом (см. lossy_line).
            let reader = std::io::BufReader::new(stream);
            for buf in reader.split(b'\n') {
                let Ok(buf) = buf else { break };
                if tx.send(lossy_line(&buf)).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);

    // D62: наш файл можно смело пересоздавать (обрезаем лог прошлого запуска),
    // хэндл не держим; строки дальше пишем append-ом на каждую.
    // latest.log не создаём и не открываем вовсе — он принадлежит игре.
    std::fs::File::create(&cli_log)?;
    let mut tail: VecDeque<String> = VecDeque::with_capacity(64);
    let start = Instant::now();
    let mut found_at: Option<Instant> = None;
    let mut atlas_line: Option<String> = None;

    loop {
        if start.elapsed() > Duration::from_secs(timeout_secs) {
            eprintln!("  таймаут ожидания главного меню");
            break;
        }
        // После «Sound engine started» даём игре ещё 8 с: атласы логируются
        // сразу после звука (спека требует оба доказательства).
        if let Some(t) = found_at {
            if t.elapsed() > Duration::from_secs(8) {
                break;
            }
        }
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(line) => {
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&cli_log) {
                    let _ = writeln!(f, "{line}");
                }
                if found_at.is_none() && line.contains("Sound engine started") {
                    found_at = Some(Instant::now());
                }
                if atlas_line.is_none() && line.contains("textures/atlas") {
                    atlas_line = Some(line.clone());
                }
                tail.push_back(line);
                if tail.len() > 64 {
                    tail.pop_front();
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
    let found = found_at.is_some();

    // Завершаем дерево процессов игры (мы — супервизор приёмки).
    // D64: порядок критичен — дерево убивается ДО выяснения смерти/kill/wait
    // ребёнка и всегда по завершении супервизии. Прежде kill_tree шёл ПОСЛЕ
    // child.kill()+wait(): родитель к тому моменту был мёртв, taskkill /T
    // не находил дерево — и дети java переживали супервизию. Пока хэндл
    // ребёнка открыт, ОС не переиспользует его PID, так что taskkill по PID
    // не может попасть в чужой процесс (страх D62 снят порядком операций);
    // на уже завершившегося ребёнка taskkill просто не находит процесс.
    // child.kill()+wait() остаются как страховка и для репы (нет zombie).
    let pid = child.id();
    kill_tree(pid);
    if child.try_wait()?.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }

    // D62: «полный лог» для диагностики — latest.log, который писала сама
    // игра; читаем его с диска ПОСЛЕ завершения процесса (к тому моменту
    // Log4j сбросил буферы). Файла может не быть (падение до инициализации
    // логгера) — обрабатываем отсутствие без ошибки.
    let full_log = std::fs::read_to_string(&latest_log).ok();

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
        // D62: путь для crash-analyze печатаем только тот, чьё содержимое мы
        // реально прочитали выше — иначе пользователь отправит в анализ файл,
        // которого нет. Не читается latest.log → указываем на наш лог.
        match &full_log {
            Some(text) => println!(
                "  полный лог: {} ({} строк)",
                latest_log.display(),
                text.lines().count()
            ),
            None => println!(
                "  полный лог: {} (latest.log не найден или не читается)",
                cli_log.display()
            ),
        }
    }
    Ok(found)
}

/// D64: лог для crash-analyze читаем с капом — только ХВОСТ (последние ~2 МБ):
/// стектрейс и секция причин краша всегда в конце, а гигабайтный latest.log
/// целиком в память тянуть не зачем (seek к len-капу, память тоже ограничена).
/// Декод lossy: прежде read_to_string падал на первом не-UTF-8 байте («поток
/// содержит не UTF-8»), теперь мусорные байты (и разрезанный на границе хвоста
/// UTF-8-символ) дают U+FFFD — анализатор работает по строкам, для него это
/// безразлично.
const CRASH_LOG_TAIL_BYTES: u64 = 2 * 1024 * 1024;

fn crash_log_tail(path: &str) -> Result<String> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(CRASH_LOG_TAIL_BYTES)))?;
    let mut raw = Vec::new();
    file.read_to_end(&mut raw)?;
    Ok(String::from_utf8_lossy(&raw).into_owned())
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
        // D62: утилиты убийства дерева в libcrate нет (instances::kill делает
        // тот же одиночный kill), /proc-обход выдумывать в CLI не стал —
        // честное ограничение: дети java, если завелись, остаются жить.
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
    }
}
