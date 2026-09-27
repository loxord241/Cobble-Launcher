//! mc-launcher-v2: ядро лаунчера. Весь бизнес живёт здесь; UI — тонкий слой
//! поверх IPC (спека §4.3).

pub mod auth;
pub mod commands;
pub mod errors;
pub mod events;
pub mod import;
pub mod instances;
pub mod java;
pub mod loaders;
pub mod mojang;
pub mod modrinth;
pub mod net;
pub mod optimize;
pub mod paths;
pub mod process;
pub mod settings;
pub mod util;

use tracing_subscriber::EnvFilter;

/// MakeWriter с redaction (спека §11): токены → [REDACTED] до записи на диск.
struct RedactingWriter<W: std::io::Write> {
    inner: W,
}

impl<W: std::io::Write> std::io::Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let safe = util::redact::redact(&text);
        self.inner.write_all(safe.as_bytes())?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

struct RedactingMakeWriter {
    dir: std::path::PathBuf,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RedactingMakeWriter {
    type Writer = RedactingWriter<std::fs::File>;

    fn make_writer(&'a self) -> Self::Writer {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(util::fs::long_path(&self.dir.join("launcher.log")))
            .unwrap_or_else(|_| std::fs::File::create(std::path::PathBuf::from("launcher.log")).unwrap());
        RedactingWriter { inner: file }
    }
}

/// Файловое логирование с redaction токенов (спека §4.1, §11). В CLI не вызывается.
pub fn init_file_logging(logs_dir: &std::path::Path) {
    let long = util::fs::long_path(logs_dir);
    let _ = std::fs::create_dir_all(&long);
    // Ротация по размеру — вручную: переименование при старте, если > 5 МБ.
    rotate_if_big(&long.join("launcher.log"));
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(RedactingMakeWriter { dir: long })
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
            commands::content_toggle,
            commands::content_remove,
            commands::content_update_check,
            commands::content_update_all,
            commands::mrpack_install,
            commands::modpack_install,
            commands::instance_backup,
            commands::instance_export,
            commands::instance_icon_set,
            commands::instance_icon_remove,
            commands::instance_optimize,
            commands::ram_guide,
            commands::instance_kill,
            commands::process_status,
            commands::account_list,
            commands::account_add_offline,
            commands::account_add_msa_start,
            commands::account_add_msa_poll,
            commands::account_add_msa_browser,
            commands::account_add_ely,
            commands::account_remove,
            commands::account_active_set,
            commands::instance_import,
            commands::crash_analyze,
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
