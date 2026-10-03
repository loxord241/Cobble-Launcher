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
fn family_prefix(mc: &str) -> String {
    if let Some(rest) = mc.strip_prefix("1.") {
        format!("{rest}.")
    } else {
        format!("{mc}.")
    }
}

/// Свежая стабильная версия NeoForge для указанного MC.
pub async fn latest_for_mc(client: &HttpClient, mc: &str) -> Result<String> {
    let all = versions(client).await?;
    let prefix = family_prefix(mc);
    all.iter()
        .filter(|v| v.starts_with(&prefix) && !v.ends_with("-beta"))
        .max()
        .cloned()
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
    let resp = client.raw().get(&jar_url).send().await?;
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "HTTP {} для {jar_url}",
            resp.status()
        )));
    }
    let bytes = resp.bytes().await?;

    let sha_resp = client.raw().get(&sha_url).send().await?;
    if !sha_resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "нет .sha1 для installer {version} — скачивание запрещено (спека §3)"
        )));
    }
    let expected = String::from_utf8_lossy(&sha_resp.bytes().await?)
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
}
