//! Обнаружение системных Java и разбор версий (спека §6.5).

use crate::errors::{LauncherError, Result};
use crate::util::win::hide_console;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
// A6: в UI (src/api/types.ts) поля ждут camelCase — без rename список Java
// приходил как java_exe/arch_bits и выпадающий список в настройках пустел.
#[serde(rename_all = "camelCase")]
pub struct JavaInstall {
    /// Путь к исполнимому файлу java.
    pub java_exe: PathBuf,
    pub major: u32,
    pub arch_bits: u32,
    /// Откуда найдено (PATH, Program Files, runtime лаунчера…).
    pub origin: String,
}

fn java_exe_name() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

/// Кандидаты-каталоги для сканирования (спека §6.5: PATH, Adoptium, Java,
/// Zulu, ~/.jdks; + каталог рантаймов лаунчера).
pub fn candidate_dirs(runtime_dir: &Path, extra: &[String]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    // PATH
    if let Some(paths) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&paths) {
            dirs.push(p);
        }
    }

    let mut push_glob = |root: PathBuf| {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                let bin = e.path().join("bin");
                if bin.is_dir() {
                    dirs.push(bin);
                }
                // Некоторые установки имеют java прямо в каталоге
                dirs.push(e.path());
            }
        }
    };

    // Каталог Program Files (dirs 7 его не экспортирует).
    if let Some(pf) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
        for vendor in ["Java", "Eclipse Adoptium", "Zulu", "Microsoft", "Amazon Corretto"] {
            push_glob(pf.join(vendor));
        }
    }
    if let Some(home) = dirs::home_dir() {
        push_glob(home.join(".jdks"));
    }
    // Рантайм лаунчера: runtime/jdk{N}/bin
    if let Ok(entries) = std::fs::read_dir(runtime_dir) {
        for e in entries.flatten() {
            dirs.push(e.path().join("bin"));
        }
    }
    for e in extra {
        let p = PathBuf::from(e);
        dirs.push(p.join("bin"));
        dirs.push(p);
    }

    dirs.retain(|d| d.is_dir());
    dirs.dedup();
    dirs
}

/// Список java-исполняемых файлов из кандидатов (без запуска).
pub fn find_java_exes(runtime_dir: &Path, extra: &[String]) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    for dir in candidate_dirs(runtime_dir, extra) {
        let exe = dir.join(java_exe_name());
        if exe.is_file() {
            let origin = dir
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| dir.to_string_lossy().into_owned());
            out.push((exe, origin));
        }
    }
    // Дедуп по каноническому пути
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// Разбор вывода `java -version`: `version "1.8.0_392"` → 8; `version "21.0.3"` → 21.
pub fn parse_java_version(text: &str) -> Option<u32> {
    let re = regex::Regex::new(r#"version "([^"]+)""#).ok()?;
    let cap = re.captures(text)?;
    let full = cap.get(1)?.as_str();
    let major_part = if let Some(rest) = full.strip_prefix("1.") {
        rest.split('.').next()?
    } else {
        full.split('.').next()?
    };
    major_part.parse::<u32>().ok()
}

/// Разбор `sun.arch.data.model` из `-XshowSettings:properties`.
pub fn parse_arch_bits(text: &str) -> u32 {
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("sun.arch.data.model") {
            let v = v.trim_start().trim_start_matches('=').trim();
            if let Ok(n) = v.parse::<u32>() {
                return n;
            }
        }
    }
    64
}

/// Результат пробного запуска конкретной java (F15; зеркало `JavaTestInfo`
/// в src/api/types.ts).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaTestInfo {
    pub major: i32,
    pub bits: u32,
    /// Строка вывода с `version "…"` (для диагностики в UI).
    pub version_line: String,
}

/// Чистый разбор вывода `java -version` (F15): (major, битность, строка
/// версии). Битность ищется по «64-Bit»/«32-Bit» (её печатает VM-строка);
/// метки нет — 64 (все актуальные сборки 64-битные). Строки версии нет — None.
pub fn parse_java_test_output(text: &str) -> Option<(i32, u32, String)> {
    let major = i32::try_from(parse_java_version(text)?).ok()?;
    let bits = if text.to_ascii_lowercase().contains("32-bit") {
        32
    } else {
        64
    };
    let version_line = text
        .lines()
        .find(|l| l.contains("version \""))
        .or_else(|| text.lines().find(|l| !l.trim().is_empty()))
        .unwrap_or("")
        .trim()
        .to_string();
    Some((major, bits, version_line))
}

/// Пробный запуск java (F15): `java -version`, таймаут 5 с. Вывод версии
/// java печатает в stderr. Ошибка старта (битый путь/не исполнимый) —
/// InvalidInput с текстом, молчание дольше 5 с — Timeout.
pub fn test_java(java_exe: &str) -> Result<JavaTestInfo> {
    let exe = java_exe.trim();
    if exe.is_empty() {
        return Err(LauncherError::InvalidInput("путь к java пуст".into()));
    }
    let mut cmd = Command::new(crate::util::fs::long_path(Path::new(exe)));
    cmd.arg("-version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| LauncherError::InvalidInput(format!("не удалось запустить {exe}: {e}")))?;

    // java -version печатает ~200 байт — пайп не переполнится, пока поллим
    // try_wait (читать и ждать одновременно на std::process нельзя без потока).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(LauncherError::Timeout(format!(
                        "java -version не ответил за 5 с: {exe}"
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(LauncherError::Internal(format!("try_wait java: {e}")));
            }
        }
    }

    let mut buf = Vec::new();
    {
        use std::io::Read as _;
        if let Some(mut s) = child.stdout.take() {
            let _ = s.read_to_end(&mut buf);
        }
        if let Some(mut s) = child.stderr.take() {
            let _ = s.read_to_end(&mut buf);
        }
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (major, bits, version_line) = parse_java_test_output(&text).ok_or_else(|| {
        LauncherError::InvalidInput(format!(
            "не удалось разобрать версию java: {}",
            text.lines().next().unwrap_or("").trim()
        ))
    })?;
    Ok(JavaTestInfo {
        major,
        bits,
        version_line,
    })
}

/// Определить версию и битность конкретной java (запуск процессов, скрытая консоль).
pub async fn inspect(java_exe: &Path, origin: &str) -> Result<JavaInstall> {
    let exe = crate::util::fs::long_path(java_exe);
    let mut vcmd = Command::new(&exe);
    vcmd.arg("-version");
    hide_console(&mut vcmd);
    let vout = tokio::task::spawn_blocking(move || {
        let out = vcmd.output().map_err(|e| {
            LauncherError::JavaNotFound(format!("не удалось запустить java: {e}"))
        })?;
        Ok::<std::process::Output, LauncherError>(out)
    })
    .await
    .map_err(|e| LauncherError::Internal(format!("join java -version: {e}")))??;

    let text = String::from_utf8_lossy(&vout.stderr).into_owned();
    let major = parse_java_version(&text).ok_or_else(|| {
        LauncherError::JavaNotFound(format!(
            "не удалось разобрать версию java: {}",
            text.lines().next().unwrap_or("")
        ))
    })?;

    let mut scmd = Command::new(&exe);
    scmd.args(["-XshowSettings:properties", "-version"]);
    hide_console(&mut scmd);
    let sout = tokio::task::spawn_blocking(move || scmd.output())
        .await
        .map_err(|e| LauncherError::Internal(format!("join showSettings: {e}")))??;
    let stext = format!(
        "{}{}",
        String::from_utf8_lossy(&sout.stdout),
        String::from_utf8_lossy(&sout.stderr)
    );
    let arch_bits = parse_arch_bits(&stext);

    Ok(JavaInstall {
        java_exe: java_exe.to_path_buf(),
        major,
        arch_bits,
        origin: origin.to_string(),
    })
}

/// Просканировать все кандидаты и вернуть найденные Java.
pub async fn scan_all(runtime_dir: &Path, extra: &[String]) -> Vec<JavaInstall> {
    let mut installs = Vec::new();
    for (exe, origin) in find_java_exes(runtime_dir, extra) {
        match inspect(&exe, &origin).await {
            Ok(i) => installs.push(i),
            Err(e) => tracing::debug!("java {exe:?} не разобрана: {e}"),
        }
    }
    installs
}

/// Выбрать подходящую Java: точный major, затем ближайший больший.
pub fn pick_java(need_major: u32, list: &[JavaInstall]) -> Option<&JavaInstall> {
    list.iter()
        .find(|j| j.major == need_major)
        .or_else(|| {
            list.iter()
                .filter(|j| j.major > need_major)
                .min_by_key(|j| j.major)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_and_legacy_versions() {
        assert_eq!(
            parse_java_version(r#"openjdk version "21.0.3" 2024-04-16"#),
            Some(21)
        );
        assert_eq!(
            parse_java_version(r#"java version "1.8.0_392""#),
            Some(8)
        );
        assert_eq!(
            parse_java_version(r#"openjdk version "25" 2025-09-16"#),
            Some(25)
        );
        assert_eq!(parse_java_version("no version here"), None);
    }

    #[test]
    fn parses_arch_bits() {
        assert_eq!(
            parse_arch_bits("sun.arch.data.model = 64\nos.arch = x86_64"),
            64
        );
        assert_eq!(parse_arch_bits("garbage"), 64);
        assert_eq!(parse_arch_bits("sun.arch.data.model = 32"), 32);
    }

    /// F15: разбор вывода `java -version` — современные и легаси-сборки.
    #[test]
    fn parses_java_test_output_modern_and_legacy() {
        let modern = concat!(
            "openjdk version \"17.0.2\" 2022-01-18\n",
            "OpenJDK Runtime Environment Temurin-17.0.2+8 (build 17.0.2+8)\n",
            "OpenJDK 64-Bit Server VM Temurin-17.0.2+8 (build 17.0.2+8, mixed mode, sharing)"
        );
        let (major, bits, line) = parse_java_test_output(modern).unwrap();
        assert_eq!(major, 17);
        assert_eq!(bits, 64);
        assert!(line.contains("17.0.2"), "{line}");

        let legacy = concat!(
            "java version \"1.8.0_392\"\n",
            "Java(TM) SE Runtime Environment (build 1.8.0_392-b13)\n",
            "Java HotSpot(TM) 32-Bit Server VM (build 25.392-b13, mixed mode)"
        );
        let (major, bits, _) = parse_java_test_output(legacy).unwrap();
        assert_eq!(major, 8);
        assert_eq!(bits, 32);
    }

    /// F15: метки битности нет → 64 по умолчанию; версии нет → None.
    #[test]
    fn java_test_output_defaults_and_rejects_garbage() {
        let (major, bits, line) = parse_java_test_output("openjdk version \"21\" 2023-09-19").unwrap();
        assert_eq!((major, bits), (21, 64));
        assert_eq!(line, "openjdk version \"21\" 2023-09-19");
        assert!(parse_java_test_output("got garbage, no version").is_none());
    }

    /// F15: контракт с UI — camelCase (`versionLine`).
    #[test]
    fn java_test_info_serializes_camel_case_for_ui() {
        let info = JavaTestInfo {
            major: 21,
            bits: 64,
            version_line: "openjdk version \"21\"".into(),
        };
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(v["major"], 21);
        assert_eq!(v["bits"], 64);
        assert_eq!(v["versionLine"], "openjdk version \"21\"");
        assert!(v.get("version_line").is_none(), "snake_case не должен утекать в UI");
    }

    #[test]
    fn pick_prefers_exact_then_closest_higher() {
        let mk = |major: u32| JavaInstall {
            java_exe: PathBuf::from("java"),
            major,
            arch_bits: 64,
            origin: "t".into(),
        };
        let list = vec![mk(8), mk(17), mk(25)];
        assert_eq!(pick_java(17, &list).unwrap().major, 17);
        assert_eq!(pick_java(21, &list).unwrap().major, 25);
        assert_eq!(pick_java(26, &list).map(|j| j.major), None);
    }

    /// A6-регресс: контракт с UI — camelCase (`javaExe`/`archBits`).
    /// Убрав `rename_all`, получим пустой список Java в настройках.
    #[test]
    fn serializes_camel_case_for_ui() {
        let j = JavaInstall {
            java_exe: PathBuf::from(r"C:\jdk\bin\java.exe"),
            major: 21,
            arch_bits: 64,
            origin: "PATH".into(),
        };
        let v = serde_json::to_value(&j).unwrap();
        assert_eq!(v["javaExe"], r"C:\jdk\bin\java.exe");
        assert_eq!(v["archBits"], 64);
        assert_eq!(v["major"], 21);
        assert!(v.get("java_exe").is_none(), "snake_case не должен утекать в UI");
        assert!(v.get("arch_bits").is_none());
    }
}
