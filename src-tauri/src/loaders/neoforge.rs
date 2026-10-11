//! NeoForge (спека §6.3): maven.neoforged.net — список версий + headless
//! `java -jar installer.jar --installClient <game_dir>`. Никаких `--mirror`
//! на чужие домены (антипаттерн 16Launcher, спека §15).

use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use crate::util::win::hide_console;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MAVEN_BASE: &str = "https://maven.neoforged.net";

/// Список релизных версий NeoForge с maven API.
/// Реальный ответ: `{"isSnapshot": …, "versions": ["21.1.251", …]}` —
/// схема именования: `21.1.x` → MC 1.21.1, `26.3.x` → MC 26.3.
pub async fn versions(client: &HttpClient) -> Result<Vec<String>> {
    let url = format!("{MAVEN_BASE}/api/maven/versions/releases/net/neoforged/neoforge");
    #[derive(serde::Deserialize)]
    struct Resp {
        versions: Vec<String>,
    }
    let r: Resp = client.get_json_retry(&url).await?;
    Ok(r.versions)
}

/// Префикс семейства NeoForge для версии MC (см. схему именования выше).
/// Точное семейство (аудит 2026-10-06): MC 1.21.1 → «21.1.», но MC 1.21 →
/// «21.0.» — прежний «21.» матчит и 21.0.x, и 21.1.x.
fn family_prefix(mc: &str) -> String {
    // Отбрасываем ведущее «1.»: NeoForge-мажор = minor MC («1.21.1» → 21.1.x).
    let rest = mc.strip_prefix("1.").unwrap_or(mc);
    let mut parts = rest.split('.');
    let minor = parts.next().unwrap_or("0");
    // Нет третьего компонента MC (просто «1.21») — патч-семейство 0.
    let patch = parts.next().unwrap_or("0");
    format!("{minor}.{patch}.")
}

/// Версия «major.minor.patch» → числовой кортеж: лексикографика String
/// ставит «21.1.9» выше «21.1.251» (аудит 2026-10-06). Некорректные — skip.
fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None; // лишние компоненты — не наш формат
    }
    Some((major, minor, patch))
}

/// Свежая стабильная версия в списке для семейства (чистая функция —
/// покрывается тестами без сети).
fn latest_in(prefix: &str, versions: &[String]) -> Option<String> {
    versions
        .iter()
        .filter(|v| v.starts_with(prefix) && !v.ends_with("-beta"))
        .filter_map(|v| parse_version(v).map(|t| (t, v)))
        .max_by_key(|(t, _)| *t)
        .map(|(_, v)| v.clone())
}

/// Свежая стабильная версия NeoForge для указанного MC.
pub async fn latest_for_mc(client: &HttpClient, mc: &str) -> Result<String> {
    let all = versions(client).await?;
    latest_in(&family_prefix(mc), &all)
        .ok_or_else(|| LauncherError::VersionNotFound(format!("NeoForge для {mc}")))
}

fn installer_path(cache_dir: &Path, version: &str) -> PathBuf {
    cache_dir.join("installers").join(format!("neoforge-{version}-installer.jar"))
}

/// Ожидаемый sha1 из maven (`.sha1` рядом с jar — крошечный текст, обычные
/// 60 с на тело). None — недоступен (сеть/статус): вызывающий решает,
/// запретить ли скачивание или использовать кэш без сверки.
async fn expected_sha1(client: &HttpClient, sha_url: &str) -> Option<String> {
    let resp = client.send_timed(client.raw().get(sha_url)).await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = client.body_timed(resp).await.ok()?;
    let sha = String::from_utf8_lossy(&body)
        .split_whitespace()
        .next()?
        .to_lowercase();
    (!sha.is_empty()).then_some(sha)
}

/// Скачать installer.jar с обязательной сверкой `.sha1` (спека §3, §6.3).
pub async fn download_installer(
    client: &HttpClient,
    cache_dir: &Path,
    version: &str,
) -> Result<PathBuf> {
    let dest = installer_path(cache_dir, version);
    let dest_long = crate::util::fs::long_path(&dest);
    let jar_url = format!(
        "{MAVEN_BASE}/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"
    );
    let sha_url = format!("{jar_url}.sha1");

    if dest_long.exists() {
        // F16: офлайн — кэш единственный источник, проверять нечем.
        if client.offline() {
            return Ok(dest);
        }
        // D64: кэш переиспользуем только после сверки с .sha1 maven —
        // побитый на диске jar иначе жил бы в кэше вечно.
        match expected_sha1(client, &sha_url).await {
            Some(expected) => {
                let actual = crate::util::fs::sha1_file(&dest_long).unwrap_or_default();
                if actual == expected {
                    return Ok(dest);
                }
                tracing::warn!("кэш installer {version} расходится с .sha1 — перекачиваю");
            }
            None => {
                tracing::warn!(".sha1 для installer {version} недоступен — использую кэш без сверки");
                return Ok(dest);
            }
        }
    }

    // F16: офлайн-режим — мгновенный честный отказ вместо сетевого таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode("загрузчик NeoForge".into()));
    }
    // D62: у «сырых» GET ниже своего таймаута нет — общий у клиента снят,
    // поэтому send и чтение тела страхуем локально.
    // D62: TTFB-предохранитель на send (30 с)…
    let resp = client.send_timed(client.raw().get(&jar_url)).await?;
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "HTTP {} для {jar_url}",
            resp.status()
        )));
    }
    // …и щедрый лимит на тело (600 с): installer.jar — крупный бинарник,
    // короткий лимит рвал бы его на медленном канале.
    let bytes = client.body_timed_big(resp).await?;

    // D62: .sha1 — крошечный текст, обычные 60 с на тело достаточны.
    let expected = expected_sha1(client, &sha_url)
        .await
        .ok_or_else(|| {
            LauncherError::network(format!(
                "нет .sha1 для installer {version} — скачивание запрещено (спека §3)"
            ))
        })?;
    let actual = crate::util::fs::sha1_bytes(&bytes);
    if actual != expected {
        return Err(LauncherError::HashMismatch {
            path: jar_url,
            expected,
            actual,
        });
    }
    std::fs::create_dir_all(crate::util::fs::long_path(dest.parent().unwrap()))?;
    crate::util::fs::atomic_write(&dest, &bytes)?;
    Ok(dest)
}

/// D64: выбрать id появившейся версии: новая (нет в `before`) ИЛИ — при
/// повторной установке — последняя по mtime СУЩЕСТВУЮЩАЯ, но обязательно
/// содержащая требуемую версию инсталлятора. Прежний fallback «последняя
/// вообще» мог отдать чужую версию (например, свежий ванильный JSON).
/// `after` отсортирован по mtime по возрастанию → последняя подходящая самая свежая.
fn pick_installed_version(before: &[String], after: &[String], expected: &str) -> Option<String> {
    after
        .iter()
        .find(|v| !before.contains(v) && v.contains(expected))
        .or_else(|| after.iter().rev().find(|v| v.contains(expected)))
        .cloned()
}

/// Headless-установка: `java -jar installer.jar --installClient <game_dir>`,
/// cwd = каталог игры, stdout/stderr собираются (спека §6.3).
/// Инсталлятор отказывается работать без `launcher_profiles.json` (проверка
/// «вы запускали ванильный лаунчер») — создаём минимальный профиль.
/// Возвращает id появившейся версии — только из версий, соответствующих
/// требуемой версии инсталлятора (`expected_version`).
/// D64: у выполнения стоит дедлайн INSTALLER_TIMEOUT — зависший инсталлятор
/// убивается по дереву процессов, установка падает с честной ошибкой.
pub fn run_installer(
    installer: &Path,
    java_exe: &Path,
    game_dir: &Path,
    loader: &str,
    expected_version: &str,
    jvm_extra: &[String],
) -> Result<String> {
    let profiles = game_dir.join("launcher_profiles.json");
    if !profiles.exists() {
        std::fs::create_dir_all(crate::util::fs::long_path(game_dir))?;
        crate::util::fs::atomic_write(
            &profiles,
            br#"{"profiles": {}, "settings": {}, "version": 3}"#,
        )?;
    }
    let before = list_versions(game_dir);
    let mut cmd = Command::new(java_exe);
    for arg in jvm_extra {
        cmd.arg(arg);
    }
    cmd.arg("-jar")
        .arg(crate::util::fs::long_path(installer))
        .arg("--installClient")
        .arg(crate::util::fs::long_path(game_dir))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // ENV-6: рабочий каталог получает Win32 (SetCurrentDirectoryW), `\\?\`-пути
        // он не принимает (см. spawn_dir в run.rs) — префикс снимаем. Аргументы
        // выше идут файловыми API и остаются расширенными.
        .current_dir(crate::util::fs::strip_long_prefix(game_dir));
    hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| LauncherError::Internal(format!("запуск инсталлятора: {e}")))?;

    // Пайпы читаем в потоках: инсталлятор болтлив, без дренирования пайп
    // (~64 КБ) переполнился бы и ребёнок завис на записи до нашего дедлайна.
    use std::io::Read as _;
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout_pipe.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr_pipe.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });

    // D64: дедлайн с поллимогом try_wait (как test_java в java/detect.rs) —
    // по истечении убиваем дерево процессов и падаем с честной ошибкой.
    let deadline = std::time::Instant::now() + crate::loaders::INSTALLER_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    crate::loaders::kill_tree(child.id());
                    let _ = child.wait();
                    return Err(LauncherError::Timeout(format!(
                        "установка {loader}: инсталлятор завис и остановлен \
                         (нет завершения за {:?})",
                        crate::loaders::INSTALLER_TIMEOUT
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                crate::loaders::kill_tree(child.id());
                let _ = child.wait();
                return Err(LauncherError::Internal(format!("ожидание инсталлятора: {e}")));
            }
        }
    };
    let out_stdout = stdout_reader.join().unwrap_or_default();
    let out_stderr = stderr_reader.join().unwrap_or_default();
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&out_stdout),
        String::from_utf8_lossy(&out_stderr)
    );
    if !status.success() {
        let tail: String = log.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(LauncherError::Internal(format!(
            "инсталлятор {loader} завершился с {}: \n{tail}",
            status
        )));
    }
    let after = list_versions(game_dir);
    pick_installed_version(&before, &after, expected_version).ok_or_else(|| {
        LauncherError::Internal(format!(
            "инсталлятор {loader} не создал версию для {expected_version}"
        ))
    })
}

fn list_versions(game_dir: &Path) -> Vec<String> {
    let dir = game_dir.join("versions");
    let Ok(entries) = std::fs::read_dir(crate::util::fs::long_path(&dir)) else {
        return Vec::new();
    };
    let mut v: Vec<(std::time::SystemTime, String)> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((mtime, name))
        })
        .collect();
    v.sort();
    v.into_iter().map(|(_, n)| n).collect()
}

/// Полная установка NeoForge в инстанс: скачать инсталлятор → headless →
/// скопировать появившийся version JSON в versions/ инстанса.
pub async fn install(
    client: &HttpClient,
    cache_dir: &Path,
    java_exe: &Path,
    game_dir: &Path,
    instance_versions_dir: &Path,
    version: &str,
) -> Result<String> {
    // D64: прокси из настроек → -D-флаги JVM инсталлятора.
    let jvm_extra = crate::loaders::installer_jvm_args(cache_dir);
    let installer = download_installer(client, cache_dir, version).await?;
    let id = crate::loaders::run_installer_guarded(
        java_exe.to_path_buf(),
        installer,
        game_dir.to_path_buf(),
        "neoforge",
        version.to_string(),
        jvm_extra,
    )
    .await?;

    // JSON версии инсталлятор пишет в game_dir/versions/<id>/ — копируем
    // в versions/ инстанса для единой схемы резолвинга.
    let json = game_dir.join("versions").join(&id).join(format!("{id}.json"));
    if json.exists() {
        let data = std::fs::read(crate::util::fs::long_path(&json))?;
        crate::util::fs::atomic_write(
            &instance_versions_dir.join(format!("{id}.json")),
            &data,
        )?;
    } else {
        return Err(LauncherError::Internal(format!(
            "инсталлятор не оставил JSON для {id}"
        )));
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Реальная сеть — #[ignore], гонять на приёмке.
    #[tokio::test]
    #[ignore]
    async fn lists_versions() {
        let client = HttpClient::new(None).unwrap();
        let v = versions(&client).await.unwrap();
        assert!(v.iter().any(|s| s.starts_with("1.21")));
    }

    /// Аудит 2026-10-06: точное семейство — «1.21.1» даёт 21.1., а «1.21»
    /// (без патча) даёт 21.0., не матчит 21.1.x.
    #[test]
    fn family_prefix_is_exact_family() {
        assert_eq!(family_prefix("1.21.1"), "21.1.");
        assert_eq!(family_prefix("1.21"), "21.0.");
        assert_eq!(family_prefix("1.20.1"), "20.1.");
        assert_eq!(family_prefix("1.20"), "20.0.");
        // MC без ведущего «1.» (схема 26.3.x → MC 26.3) — как есть.
        assert_eq!(family_prefix("26.3"), "26.3.");
    }

    /// Аудит 2026-10-06: max по числовому кортежу, не по строке —
    /// лексикографика ставила бы «21.1.9» выше «21.1.251».
    #[test]
    fn latest_in_picks_numeric_max() {
        let v: Vec<String> = ["21.1.9", "21.1.251", "21.1.100"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(latest_in("21.1.", &v).as_deref(), Some("21.1.251"));
    }

    /// Бета-фильтр сохранён: суффиксные версии в выборе не участвуют.
    #[test]
    fn latest_in_skips_beta_and_keeps_stable() {
        let v: Vec<String> = ["21.1.9-beta", "21.1.8", "21.1.10-beta"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(latest_in("21.1.", &v).as_deref(), Some("21.1.8"));
    }

    /// Чужое семейство и мусор вне формата m.m.p отсеиваются; пустой выбор —
    /// None (latest_for_mc превратит в VersionNotFound).
    #[test]
    fn latest_in_filters_foreign_family_and_garbage() {
        let v: Vec<String> = ["21.0.55", "21.1.9", "не-версия", "21.1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(latest_in("21.1.", &v).as_deref(), Some("21.1.9"));
        assert_eq!(latest_in("99.9.", &v), None);
        // «21.1» без третьей компоненты — не наш формат (skip), не паника.
        assert_eq!(parse_version("21.1"), None);
        assert_eq!(parse_version("21.1.2.3"), None);
        assert_eq!(parse_version("21.1.251"), Some((21, 1, 251)));
    }

    /// D64: выбор id после инсталлятора фильтруется по требуемой версии —
    /// прежний fallback «последняя по mtime вообще» мог отдать чужую версию.
    /// `after` отсортирован по mtime по возрастанию (как list_versions).
    #[test]
    fn pick_installed_filters_by_expected_version() {
        let before: Vec<String> = ["1.20.1".to_string(), "neoforge-21.0.40".to_string()].to_vec();
        // Инсталлятор создал новую версию — она и возвращается.
        let after = [
            "1.20.1".to_string(),
            "neoforge-21.0.40".to_string(),
            "neoforge-21.1.251".to_string(),
        ]
        .to_vec();
        assert_eq!(
            pick_installed_version(&before, &after, "21.1.251").as_deref(),
            Some("neoforge-21.1.251")
        );
        // Повторная установка (новых нет) — последняя по mtime, но только
        // среди версий требуемой версии.
        let after = ["1.20.1".to_string(), "neoforge-21.1.251".to_string()].to_vec();
        assert_eq!(
            pick_installed_version(&before, &after, "21.1.251").as_deref(),
            Some("neoforge-21.1.251")
        );
        // Требуемой версии нет вовсе — None (честная ошибка), а не чужая
        // свежая версия (прежний fallback отдавал бы «1.20.1»/«21.0.40»).
        let after = ["1.20.1".to_string(), "neoforge-21.0.40".to_string()].to_vec();
        assert_eq!(pick_installed_version(&before, &after, "21.1.251"), None);
        // Формат Forge («1.20.1-forge-47.4.10» содержит «47.4.10») тоже матчится.
        let after = ["1.20.1-forge-47.4.10".to_string()].to_vec();
        assert_eq!(
            pick_installed_version(&[], &after, "47.4.10").as_deref(),
            Some("1.20.1-forge-47.4.10")
        );
    }
}
