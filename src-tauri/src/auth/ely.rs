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
    let resp: AuthResp = client
        .raw()
        .post(format!("{AUTHSERVER}/api/auth/authenticate"))
        .json(&serde_json::json!({
            "agent": { "name": "Minecraft", "version": 1 },
            "username": username,
            "password": password,
            "clientToken": CLIENT_TOKEN,
            "requestUser": false
        }))
        .send()
        .await?
        .error_for_status()
        .map_err(|e| LauncherError::InvalidInput(format!("ely.by: неверный логин/пароль ({e})")))?
        .json()
        .await?;
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
    let resp: RefreshResp = client
        .raw()
        .post(format!("{AUTHSERVER}/api/auth/refresh"))
        .json(&serde_json::json!({
            "accessToken": refresh_token,
            "clientToken": CLIENT_TOKEN
        }))
        .send()
        .await?
        .error_for_status()
        .map_err(|e| LauncherError::network_err("ely.by refresh", &e))?
        .json()
        .await?;
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
    let resp = client.raw().get(&asset.url).send().await?;
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "HTTP {} для {}",
            resp.status(),
            asset.url
        )));
    }
    let bytes = resp.bytes().await?;
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
