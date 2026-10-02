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
pub mod mclogs;
pub mod branding;
pub mod paths;
pub mod storage;
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

/// Куда пишет логгер: файл, а при недоступности файла — стандартный вывод.
/// Лог не важнее запуска: при установке в `Program Files` (или занятом файле)
/// прежний `.unwrap()` ронял процесс при первом же сообщении (A21/D22).
enum LogSink {
    File(std::fs::File),
    Stdout(std::io::Stdout),
}

impl std::io::Write for LogSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            LogSink::File(f) => f.write(buf),
            LogSink::Stdout(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            LogSink::File(f) => f.flush(),
            LogSink::Stdout(s) => s.flush(),
        }
    }
}

/// Открыть `launcher.log` в каталоге логов; любая ошибка → stdout-fallback
/// (без unwrap).
fn log_sink(dir: &std::path::Path) -> LogSink {
    let path = util::fs::long_path(&dir.join("launcher.log"));
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(f) => LogSink::File(f),
        Err(e) => {
            eprintln!("файловый лог недоступен ({e}) — пишу в стандартный вывод");
            LogSink::Stdout(std::io::stdout())
        }
    }
}

struct RedactingMakeWriter {
    dir: std::path::PathBuf,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RedactingMakeWriter {
    type Writer = RedactingWriter<LogSink>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: log_sink(&self.dir),
        }
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
        .plugin(tauri_plugin_dialog::init())
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
            commands::modrinth_projects,
            commands::modrinth_project,
            commands::modrinth_versions,
            commands::content_install,
            commands::content_installed,
            commands::content_toggle,
            commands::content_remove,
            commands::content_update_check,
            commands::content_update_all,
            commands::content_backfill_projects,
            commands::mrpack_install,
            commands::modpack_install,
            commands::instance_backup,
            commands::instance_export,
            commands::instance_icon_set,
            commands::instance_icon_remove,
            commands::instance_optimize,
            commands::ram_guide,
            commands::instance_kill,
            commands::instance_force_unlock,
            commands::process_status,
            commands::account_list,
            commands::account_add_offline,
            commands::account_add_msa_start,
            commands::account_add_msa_poll,
            commands::account_add_msa_browser,
            commands::account_add_ely,
            commands::account_remove,
            commands::account_active_set,
            commands::account_skin,
            commands::instance_worlds,
            commands::world_backup,
            commands::world_delete_data,
            commands::instance_repair,
            commands::java_test_path,
            commands::log_share_mclogs,
            commands::instance_logs_list,
            commands::instance_log_read,
            commands::content_snapshots,
            commands::content_rollback,
            commands::content_copy,
            commands::storage_stats,
            commands::storage_clean,
            commands::settings_export,
            commands::settings_import,
            commands::instance_screens,
            commands::screenshot_delete,
            commands::instance_screens_open,
            commands::content_metadata,
            commands::pack_art,
            commands::instance_shortcut,
            commands::game_resources,
            commands::instance_launch_safe,
            commands::instance_configs,
            commands::config_read,
            commands::config_write,
            commands::authlib_server_info,
            commands::account_add_authlib,
            commands::logo_set_custom,
            commands::apply_logo,
            commands::instance_import,
            commands::crash_analyze,
            commands::java_list,
            commands::java_install,
            commands::java_recommended,
            commands::settings_get,
            commands::settings_set,
            commands::data_dir,
            commands::logs_tail,
            commands::dir_open,
            commands::update_check,
        ])
        .setup(|app| {
            use tauri::Manager as _;
            // Состояние регистрируется здесь, чтобы startup-блок ниже видел его.
            app.manage(state);
            let state: tauri::State<commands::AppState> = app.state();
            let _ = state.app.set(app.handle().clone());
            commands::spawn_event_bridge(app.handle().clone(), state.bus.clone());
            // D38: логотип владельца — иконка окна при старте.
            let logo = state.settings.blocking_read().logo.clone();
            let paths = state.paths.clone();
            crate::branding::apply_window_icon(app.handle(), &logo, &paths);
            Ok(())
        })
        .run(tauri::generate_context!())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A21: недоступный файл лога не должен ронять процесс (был `.unwrap()`):
    /// писатель уходит на стандартный вывод.
    #[test]
    fn log_sink_falls_back_to_stdout() {
        let dir = tempfile::tempdir().unwrap();
        // `launcher.log` — каталог: открыть его как файл нельзя.
        std::fs::create_dir(dir.path().join("launcher.log")).unwrap();
        let sink = log_sink(dir.path());
        assert!(
            matches!(sink, LogSink::Stdout(_)),
            "падение файлового лога → stdout, а не паника"
        );
        // Fallback-канал тоже рабочий и с redaction.
        let mut writer = RedactingWriter { inner: sink };
        assert!(std::io::Write::write_all(&mut writer, b"info: token=secret\n").is_ok());
        assert!(std::io::Write::flush(&mut writer).is_ok());
    }

    /// Обычный случай: каталог существует — пишем в файл.
    #[test]
    fn log_sink_uses_file_when_available() {
        let dir = tempfile::tempdir().unwrap();
        let sink = log_sink(dir.path());
        assert!(matches!(sink, LogSink::File(_)), "лог пишется в launcher.log");
        drop(sink);
        assert!(dir.path().join("launcher.log").exists());
    }
}
