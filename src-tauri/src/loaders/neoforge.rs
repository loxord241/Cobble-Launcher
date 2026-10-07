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

/// Скачать installer.jar с обязательной сверкой `.sha1` (спека §3, §6.3).
pub async fn download_installer(
    client: &HttpClient,
    cache_dir: &Path,
    version: &str,
) -> Result<PathBuf> {
    let dest = installer_path(cache_dir, version);
    let dest_long = crate::util::fs::long_path(&dest);
    if dest_long.exists() {
        // Уже скачан — хэш проверен при первой загрузке.
        return Ok(dest);
    }
    let jar_url = format!(
        "{MAVEN_BASE}/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"
    );
    let sha_url = format!("{jar_url}.sha1");

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
    let sha_resp = client.send_timed(client.raw().get(&sha_url)).await?;
    if !sha_resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "нет .sha1 для installer {version} — скачивание запрещено (спека §3)"
        )));
    }
    let expected = String::from_utf8_lossy(&client.body_timed(sha_resp).await?)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    let actual = crate::util::fs::sha1_bytes(&bytes);
    if expected.is_empty() || actual != expected {
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

/// Headless-установка: `java -jar installer.jar --installClient <game_dir>`,
/// cwd = каталог игры, stdout/stderr собираются (спека §6.3).
/// Инсталлятор отказывается работать без `launcher_profiles.json` (проверка
/// «вы запускали ванильный лаунчер») — создаём минимальный профиль.
/// Возвращает id появившейся версии (новейший JSON в `versions/`).
pub fn run_installer(
    installer: &Path,
    java_exe: &Path,
    game_dir: &Path,
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
    cmd.arg("-jar")
        .arg(crate::util::fs::long_path(installer))
        .arg("--installClient")
        .arg(crate::util::fs::long_path(game_dir))
        // ENV-6: рабочий каталог получает Win32 (SetCurrentDirectoryW), `\\?\`-пути
        // он не принимает (см. spawn_dir в run.rs) — префикс снимаем. Аргументы
        // выше идут файловыми API и остаются расширенными.
        .current_dir(crate::util::fs::strip_long_prefix(game_dir));
    hide_console(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| LauncherError::Internal(format!("запуск инсталлятора: {e}")))?;
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        let tail: String = log.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(LauncherError::Internal(format!(
            "инсталлятор NeoForge завершился с {}: \n{tail}",
            out.status
        )));
    }
    let after = list_versions(game_dir);
    let added = after.iter().find(|v| !before.contains(v));
    let id = added
        .or(after.last())
        .cloned()
        .ok_or_else(|| LauncherError::Internal("инсталлятор не создал версию".into()))?;
    Ok(id)
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
    let installer = download_installer(client, cache_dir, version).await?;
    let id = tokio::task::spawn_blocking({
        let installer = installer.clone();
        let java_exe = java_exe.to_path_buf();
        let game_dir = game_dir.to_path_buf();
        move || run_installer(&installer, &java_exe, &game_dir)
    })
    .await
    .map_err(|e| LauncherError::Internal(format!("join инсталлятора: {e}")))??;

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
}
