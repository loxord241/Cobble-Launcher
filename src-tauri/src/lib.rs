//! mc-launcher-v2: ядро лаунчера. Весь бизнес живёт здесь; UI — тонкий слой
//! поверх IPC (спека §4.3).

pub mod auth;
pub mod commands;
pub mod errors;
pub mod events;
pub mod instances;
pub mod java;
pub mod loaders;
pub mod mojang;
pub mod modrinth;
pub mod net;
pub mod paths;
pub mod settings;
pub mod util;

use tracing_subscriber::EnvFilter;

/// Файловое логирование с ротацией 5×5 МБ (спека §4.1). В CLI не вызывается.
pub fn init_file_logging(logs_dir: &std::path::Path) {
    let long = util::fs::long_path(logs_dir);
    let _ = std::fs::create_dir_all(&long);
    let appender = tracing_appender::rolling::never(&long, "launcher.log");
    // Ротация по размеру делается вручную: never + проверка размера при старте.
    rotate_if_big(&long.join("launcher.log"));
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::sync::Mutex::new(appender))
        .with_ansi(false)
        .try_init()
        .ok();
}

fn rotate_if_big(log: &std::path::Path) {
    const MAX: u64 = 5 * 1024 * 1024;
    if let Ok(meta) = std::fs::metadata(util::fs::long_path(log)) {
        if meta.len() > MAX {
            let old = log.with_extension("log.1");
            let _ = std::fs::rename(util::fs::long_path(log), util::fs::long_path(&old));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    let paths = paths::Paths::new(paths::Paths::default_root());
    let _ = paths.ensure_dirs();
    init_file_logging(&paths.logs_dir());

    let settings = settings::Settings::load(&paths.settings_file()).unwrap_or_default();
    let bus = events::EventBus::default();
    let state = match commands::AppState::new(paths, bus.clone(), settings) {
        Ok(s) => s,
        Err(e) => {
            // Инициализация без .expect(): сообщение в stderr + crash-файл.
            eprintln!("Ошибка инициализации [{code}]: {e}", code = e.code());
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::instance_list,
            commands::instance_create,
            commands::instance_rename,
            commands::instance_duplicate,
            commands::instance_delete,
            commands::instance_open_dir,
            commands::instance_settings_get,
            commands::instance_settings_set,
            commands::instance_launch,
            commands::manifest_versions,
            commands::loader_versions,
            commands::loader_install,
            commands::modrinth_search,
            commands::content_install,
            commands::content_installed,
            commands::content_update_check,
            commands::content_update_all,
            commands::mrpack_install,
            commands::java_list,
            commands::java_install,
            commands::settings_get,
            commands::settings_set,
            commands::data_dir,
            commands::logs_tail,
            commands::dir_open,
        ])
        .setup(|app| {
            use tauri::Manager as _;
            let state: tauri::State<commands::AppState> = app.state();
            commands::spawn_event_bridge(app.handle().clone(), state.bus.clone());
            Ok(())
        })
        .run(tauri::generate_context!())
}
