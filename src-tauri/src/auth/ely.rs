//! ely.by (спека §6.8): authserver-логин, refresh, authlib-injector (ЕДИНСТВЕННОЕ
//! исключение из «ноль инжекций» — явный выбор пользователем ely-профиля).

use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use serde::Deserialize;

pub const AUTHSERVER: &str = "https://authserver.ely.by";
pub const INJECTOR_SERVER: &str = "https://authserver.ely.by/api/authlib-injector";
const CLIENT_TOKEN: &str = "mc-launcher-v2";

/// Сессия ely.by (accessToken — их authserver, не Mojang).
#[derive(Debug, Clone)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub player_name: String,
    pub uuid: String,
}

/// Классификация не-2xx от authserver при логине (аудит 2026-10-06): 4xx —
/// неверные креды (InvalidInput, как раньше), 5xx — проблема на стороне
/// сервера (Network). Чистая функция — маппинг тестируется без сети; общая
/// для ely.by и своих authlib-серверов (auth/custom.rs).
pub(crate) fn classify_login_status(server_label: &str, status: u16) -> LauncherError {
    if status >= 500 {
        LauncherError::network(format!(
            "сервер {server_label} недоступен (HTTP {status})"
        ))
    } else {
        LauncherError::InvalidInput(format!(
            "{server_label}: неверный логин/пароль (HTTP {status})"
        ))
    }
}

/// Логин по логину/паролю ely.by (POST /api/auth/authenticate).
pub async fn authenticate(client: &HttpClient, username: &str, password: &str) -> Result<Session> {
    #[derive(Deserialize)]
    struct Profile {
        id: String,
        name: String,
    }
    #[derive(Deserialize)]
    struct AuthResp {
        #[serde(rename = "accessToken")]
        access_token: String,
        #[serde(rename = "selectedProfile")]
        profile: Profile,
    }
    // F16: офлайн-режим — вход требует сеть, честный отказ сразу.
    if client.offline() {
        return Err(LauncherError::OfflineMode("вход ely.by".into()));
    }
    // D62: предохранители send_timed/json_timed — после снятия общего таймаута
    // молчащий authserver не подвешивает логин навечно.
    let resp = client
        .send_timed(
            client
                .raw()
                .post(format!("{AUTHSERVER}/api/auth/authenticate"))
                .json(&serde_json::json!({
                    "agent": { "name": "Minecraft", "version": 1 },
                    "username": username,
                    "password": password,
                    "clientToken": CLIENT_TOKEN,
                    "requestUser": false
                })),
        )
        .await?;
    if !resp.status().is_success() {
        return Err(classify_login_status("ely.by", resp.status().as_u16()));
    }
    let resp: AuthResp = client.json_timed(resp).await?;
    crate::util::redact::register_secret(&resp.access_token);
    // ely authserver не выдаёт refresh — refreshToken == accessToken при refresh.
    Ok(Session {
        access_token: resp.access_token.clone(),
        refresh_token: resp.access_token,
        player_name: resp.profile.name,
        uuid: resp.profile.id,
    })
}

/// Обновить сессию (POST /api/auth/refresh).
pub async fn refresh_session(client: &HttpClient, refresh_token: &str) -> Result<Session> {
    // F16: офлайн-режим — refresh требует сеть, честный отказ сразу.
    if client.offline() {
        return Err(LauncherError::OfflineMode("refresh ely.by".into()));
    }
    #[derive(Deserialize)]
    struct Profile {
        id: String,
        name: String,
    }
    #[derive(Deserialize)]
    struct RefreshResp {
        #[serde(rename = "accessToken")]
        access_token: String,
        #[serde(rename = "selectedProfile")]
        profile: Profile,
    }
    // D62: предохранители send_timed/json_timed — как в authenticate выше.
    let resp = client
        .send_timed(
            client
                .raw()
                .post(format!("{AUTHSERVER}/api/auth/refresh"))
                .json(&serde_json::json!({
                    "accessToken": refresh_token,
                    "clientToken": CLIENT_TOKEN
                })),
        )
        .await?;
    // D64: 401 — сервер отклонил креды сессии; раньше уходил в network-маппинг
    // ниже и показывался как «сервер недоступен». Честный текст.
    if resp.status().as_u16() == 401 {
        return Err(LauncherError::InvalidInput(
            "сервер ely.by отклонил учётные данные (401) — проверь логин или войди заново".into(),
        ));
    }
    let resp = resp
        .error_for_status()
        .map_err(|e| LauncherError::network_err("ely.by refresh", &e))?;
    let resp: RefreshResp = client.json_timed(resp).await?;
    crate::util::redact::register_secret(&resp.access_token);
    Ok(Session {
        access_token: resp.access_token.clone(),
        refresh_token: resp.access_token,
        player_name: resp.profile.name,
        uuid: resp.profile.id,
    })
}

/// Скачать authlib-injector.jar с проверкой хэша (digest из GitHub API —
/// «хэш со страницы релиза», спека §5). Возвращает путь в кэше.
pub async fn authlib_injector_jar(
    client: &HttpClient,
    cache_dir: &std::path::Path,
) -> Result<std::path::PathBuf> {
    let dest = cache_dir.join("authlib-injector.jar");
    let dest_long = crate::util::fs::long_path(&dest);
    if dest_long.exists() {
        return Ok(dest); // уже скачан с проверкой
    }
    #[derive(Deserialize)]
    struct Release {
        #[serde(rename = "tag_name")]
        tag: String,
        assets: Vec<Asset>,
    }
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        #[serde(rename = "browser_download_url")]
        url: String,
        #[serde(default)]
        digest: Option<String>,
    }
    let release: Release = client
        .get_json_retry("https://api.github.com/repos/yushijinhun/authlib-injector/releases/latest")
        .await?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.starts_with("authlib-injector") && a.name.ends_with(".jar"))
        .ok_or_else(|| LauncherError::NotFound("authlib-injector jar в релизе".into()))?;
    let expected = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(|s| s.to_lowercase())
        .ok_or_else(|| {
            LauncherError::InvalidInput(
                "GitHub не отдал digest для authlib-injector — скачивание запрещено (спека §3)"
                    .into(),
            )
        })?;

    // F16: офлайн-режим — инжектор не скачать, честный отказ сразу.
    if client.offline() {
        return Err(LauncherError::OfflineMode(asset.url.clone()));
    }
    // D62: предохранители send_timed/body_timed_big — jar ~1–2 МБ, телу щедрый
    // лимит (600 с), send страхуется своими 30 с на TTFB.
    let resp = client.send_timed(client.raw().get(&asset.url)).await?;
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "HTTP {} для {}",
            resp.status(),
            asset.url
        )));
    }
    let bytes = client.body_timed_big(resp).await?;
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(&bytes);
    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        return Err(LauncherError::HashMismatch {
            path: asset.name.clone(),
            expected,
            actual,
        });
    }
    std::fs::create_dir_all(crate::util::fs::long_path(cache_dir))?;
    crate::util::fs::atomic_write(&dest, &bytes)?;
    tracing::info!("authlib-injector {} скачан (sha256 ок)", release.tag);
    Ok(dest)
}

/// JVM-аргумент `-javaagent:...=<server>` для запуска с ely-аккаунтом
/// (ЕДИНСТВЕННОЕ исключение из ноль-инжекций, спека §6.8).
pub fn javaagent_arg(jar: &std::path::Path) -> String {
    format!("-javaagent:{}={INJECTOR_SERVER}", jar.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Аудит 2026-10-06: не-2xx при логине классифицируется — 4xx считаем
    /// плохими кредами (InvalidInput), 5xx — недоступностью сервера (Network).
    #[test]
    fn login_status_classification() {
        // 4xx — InvalidInput с прежней подсказкой про креды.
        for status in [400u16, 401, 403, 429] {
            let err = classify_login_status("ely.by", status);
            assert!(
                matches!(err, LauncherError::InvalidInput(_)),
                "HTTP {status} должен быть InvalidInput, получено: {err:?}"
            );
            assert!(
                err.to_string().contains("неверный логин/пароль"),
                "HTTP {status}: подсказка про креды потерялась: {err}"
            );
        }
        // 5xx — Network «сервер недоступен».
        for status in [500u16, 502, 503, 521] {
            let err = classify_login_status("ely.by", status);
            assert!(
                matches!(err, LauncherError::Network(_)),
                "HTTP {status} должен быть Network, получено: {err:?}"
            );
            assert!(
                err.to_string()
                    .contains("сервер ely.by недоступен (HTTP "),
                "HTTP {status}: текст про недоступность потерялся: {err}"
            );
        }
        // Метка сервера подставляется (общая fn для custom-серверов).
        let err = classify_login_status("https://auth.example.org", 503);
        assert!(err.to_string().contains("https://auth.example.org"));
    }
}
