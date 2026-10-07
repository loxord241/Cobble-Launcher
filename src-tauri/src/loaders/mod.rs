//! Загрузчики модов (спека §6.3): Fabric/Quilt — profile JSON с inheritsFrom,
//! NeoForge — headless-инсталлятор. Все ставятся В ИНСТАНС, не глобально.

pub mod fabric;
pub mod forge;
pub mod neoforge;
pub mod quilt;

use std::time::Duration;

/// D64: лимит выполнения headless-инсталлятора. Forge/NeoForge installer
/// обычно укладывается в минуты; 10 минут — щедрый потолок. Зависший ребёнок
/// убивается по дереву (kill_tree) из синхронного дедлайна в run_installer.
pub const INSTALLER_TIMEOUT: Duration = Duration::from_secs(600);
/// D64: async-страховка поверх spawn_blocking — чуть больше INSTALLER_TIMEOUT,
/// чтобы первым срабатывал синхронный дедлайн (он умеет убить ребёнка), а не
/// этот guard.
pub const INSTALLER_EXEC_GUARD: Duration = Duration::from_secs(660);

/// D64: убить дерево процессов по PID (как instances::kill для игры):
/// taskkill /F /T на Windows, kill -9 иначе. Для зависшего инсталлятора.
pub(crate) fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("taskkill");
        c.args(["/F", "/T", "/PID", &pid.to_string()]);
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let _ = c.output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }
}

/// Общий headless-путь инсталляторов Forge-семейства (NeoForge/Forge):
/// shim `launcher_profiles.json` → `--installClient` → id новой версии.
/// Логика была в neoforge::run_installer; Forge использует ту же.
pub fn run_headless_installer(
    java_exe: &std::path::Path,
    installer: &std::path::Path,
    game_dir: &std::path::Path,
    loader: &str,
    expected_version: &str,
    jvm_extra: &[String],
) -> crate::errors::Result<String> {
    neoforge::run_installer(
        installer,
        java_exe,
        game_dir,
        loader,
        expected_version,
        jvm_extra,
    )
}

/// D64: headless-инсталлятор под двойным лимитом — синхронный дедлайн
/// INSTALLER_TIMEOUT внутри run_installer (убивает ребёнка по дереву) плюс
/// этот async-guard на случай, если застряло что-то кроме ребёнка.
pub(crate) async fn run_installer_guarded(
    java_exe: std::path::PathBuf,
    installer: std::path::PathBuf,
    game_dir: std::path::PathBuf,
    loader: &'static str,
    expected_version: String,
    jvm_extra: Vec<String>,
) -> crate::errors::Result<String> {
    match tokio::time::timeout(
        INSTALLER_EXEC_GUARD,
        tokio::task::spawn_blocking(move || {
            run_headless_installer(
                &java_exe,
                &installer,
                &game_dir,
                loader,
                &expected_version,
                &jvm_extra,
            )
        }),
    )
    .await
    {
        Ok(joined) => {
            joined.map_err(|e| crate::errors::LauncherError::Internal(format!("join инсталлятора: {e}")))?
        }
        Err(_) => Err(crate::errors::LauncherError::Timeout(format!(
            "установка {loader}: инсталлятор завис и остановлен (нет завершения за {INSTALLER_EXEC_GUARD:?})"
        ))),
    }
}

/// D64: JVM-аргументы прокси для headless-инсталлятора. Прокси берём из
/// настроек: вызывающий передаёт cache_dir = `<root>/cache` (paths.cache_dir),
/// settings.json лежит в корне данных лаунчера — восстанавливаем его путь как
/// родителя cache_dir. Файла нет/не читается — без прокси (установку это
/// не ломает). Инсталляторы JAVA_OPTS не читают — только собственные -D-флаги,
/// собираются так же, как jvm-аргументы игры (плоский Vec<String>, см.
/// mojang/launch.rs).
pub(crate) fn installer_jvm_args(cache_dir: &std::path::Path) -> Vec<String> {
    let proxy = cache_dir
        .parent()
        .map(|root| root.join("settings.json"))
        .and_then(|p| crate::settings::Settings::load(&p).ok())
        .and_then(|s| s.proxy_url)
        .filter(|p| !p.trim().is_empty());
    match proxy {
        Some(p) => proxy_jvm_args(&p),
        None => Vec::new(),
    }
}

/// Proxy URL → -D-флаги JVM инсталлятора: `http(s)://host:port` →
/// http+https proxyHost/proxyPort, `socks…` → socksProxyHost/Port.
/// Без хоста или порта — пусто: установка молча без прокси, а не падение
/// из-за кривой строки в настройках.
fn proxy_jvm_args(proxy_url: &str) -> Vec<String> {
    let url: reqwest::Url = match proxy_url.trim().parse() {
        Ok(u) => u,
        Err(_) => return Vec::new(),
    };
    let host = match url.host_str() {
        Some(h) if !h.is_empty() => h.to_string(),
        _ => return Vec::new(),
    };
    let Some(port) = url.port() else {
        return Vec::new();
    };
    match url.scheme() {
        "http" | "https" => vec![
            format!("-Dhttp.proxyHost={host}"),
            format!("-Dhttp.proxyPort={port}"),
            format!("-Dhttps.proxyHost={host}"),
            format!("-Dhttps.proxyPort={port}"),
        ],
        "socks5" | "socks5h" | "socks4" => vec![
            format!("-DsocksProxyHost={host}"),
            format!("-DsocksProxyPort={port}"),
        ],
        _ => Vec::new(),
    }
}

/// id версии безопасен как имя файла `<id>.json` (аудит 2026-10-06): только
/// [A-Za-z0-9._-]. id приходит из удалённых API (Fabric/Quilt meta) — до
/// склейки пути проверяем набор символов; хранение легитимных id не меняется.
pub(crate) fn is_safe_version_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Аудит 2026-10-06: легитимные id (Fabric «fabric-loader-0.15.11-1.20.1»,
    /// ваниль «1.20.1») проходят; разделители пути и прочая экзотика — нет.
    #[test]
    fn safe_version_id_charset() {
        for ok in [
            "fabric-loader-0.15.11-1.20.1",
            "quilt-loader-0.26.0-1.20.1",
            "1.20.1",
            "1.21.1_neoforge-21.1.100",
            "a",
        ] {
            assert!(is_safe_version_id(ok), "должно пройти: {ok}");
        }
        for bad in ["", "../evil", r"..\evil", "a/b", r"a\b", "a b", "версия", "a\nb"] {
            assert!(!is_safe_version_id(bad), "должно быть отклонено: {bad:?}");
        }
    }

    /// D64: proxy URL → -D-флаги инсталлятора: http(s) даёт http+https,
    /// socks — socksProxy; без порта/хоста или мусор — пусто (молча без прокси).
    #[test]
    fn proxy_jvm_args_http_socks_and_garbage() {
        assert_eq!(
            proxy_jvm_args("http://proxy.lan:8080"),
            vec![
                "-Dhttp.proxyHost=proxy.lan".to_string(),
                "-Dhttp.proxyPort=8080".to_string(),
                "-Dhttps.proxyHost=proxy.lan".to_string(),
                "-Dhttps.proxyPort=8080".to_string(),
            ]
        );
        assert_eq!(
            proxy_jvm_args("socks5://10.0.0.2:1080"),
            vec![
                "-DsocksProxyHost=10.0.0.2".to_string(),
                "-DsocksProxyPort=1080".to_string(),
            ]
        );
        // Без порта хост не восстановить — молча без прокси-флагов.
        assert!(proxy_jvm_args("http://host-only").is_empty());
        assert!(proxy_jvm_args("не url").is_empty());
        assert!(proxy_jvm_args("   ").is_empty());
    }

    /// D64: installer_jvm_args читает settings.json рядом с cache_dir
    /// (`<root>/cache` → `<root>/settings.json`); файла нет — пусто.
    #[test]
    fn installer_jvm_args_reads_settings_next_to_cache_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("settings.json"),
            br#"{"proxyUrl": "http://p.local:3128"}"#,
        )
        .unwrap();
        let args = installer_jvm_args(&root.join("cache"));
        assert!(args.contains(&"-Dhttp.proxyHost=p.local".to_string()), "{args:?}");
        assert!(args.contains(&"-Dhttps.proxyPort=3128".to_string()), "{args:?}");

        // Нет settings.json — дефолт без прокси, флагов нет.
        let dir2 = tempfile::tempdir().unwrap();
        assert!(installer_jvm_args(&dir2.path().join("cache")).is_empty());
    }
}
