//! Microsoft-аутентификация: device-code flow → XBL → XSTS → Minecraft
//! (спека §6.8). Refresh-токен живёт в keyring; в логи не попадает
//! (util::redact).

use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use serde::{Deserialize, Serialize};

const AUTH_BASE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/minecraft/loginWithXbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const SCOPE: &str = "XboxLive.signin offline_access";

/// Начало device-code flow: показать user_code, открыть verification_uri.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeStart {
    pub device_code: String,
    pub user_code: String,
    #[serde(rename = "verification_uri")]
    pub verification_url: String,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default = "default_expires")]
    pub expires_in: u64,
}

fn default_interval() -> u64 {
    5
}

fn default_expires() -> u64 {
    900
}

pub async fn device_code_start(client: &HttpClient, azure_client_id: &str) -> Result<DeviceCodeStart> {
    let resp: DeviceCodeStart = client
        .raw()
        .post(format!("{AUTH_BASE}/devicecode"))
        .form(&[("client_id", azure_client_id), ("scope", SCOPE)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp)
}

/// Готовая сессия после входа.
#[derive(Debug, Clone)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub player_name: String,
    pub uuid: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
}

/// Один опрос токена. `Ok(None)` — авторизация ещё pending.
pub async fn device_code_poll(
    client: &HttpClient,
    azure_client_id: &str,
    device_code: &str,
) -> Result<Option<Session>> {
    let resp = client
        .raw()
        .post(format!("{AUTH_BASE}/token"))
        .form(&[
            ("client_id", azure_client_id),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
        ])
        .send()
        .await?;
    if resp.status().is_success() {
        let token: TokenResponse = resp.json().await?;
        let refresh = token
            .refresh_token
            .ok_or_else(|| LauncherError::Internal("MSA не вернул refresh_token".into()))?;
        let session = xbox_chain(client, &token.access_token, refresh).await?;
        crate::util::redact::register_secret(&session.access_token);
        crate::util::redact::register_secret(&session.refresh_token);
        return Ok(Some(session));
    }
    let err: TokenError = resp.json().await.unwrap_or(TokenError {
        error: "unknown".into(),
    });
    match err.error.as_str() {
        "authorization_pending" => Ok(None),
        "slow_down" => Ok(None),
        other => Err(LauncherError::InvalidInput(format!(
            "MSA device code: {other}"
        ))),
    }
}

/// Обновить сессию по refresh-токену (авто-refresh при старте, спека §6.8).
pub async fn refresh_session(client: &HttpClient, refresh_token: &str) -> Result<Session> {
    // client_id нужен для refresh; берём из локальных настроек.
    let settings = crate::settings::Settings::load(
        &crate::paths::Paths::default_root().join("settings.json"),
    )?;
    let azure_client_id = settings.azure_client_id.ok_or_else(|| {
        LauncherError::InvalidInput(
            "нет azureClientId в настройках — укажите его в Настройках (MSA-вход)".into(),
        )
    })?;
    let resp = client
        .raw()
        .post(format!("{AUTH_BASE}/token"))
        .form(&[
            ("client_id", azure_client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", SCOPE),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "MSA refresh: HTTP {} — требуется повторный вход",
            resp.status()
        )));
    }
    let token: TokenResponse = resp.json().await?;
    let refresh = token
        .refresh_token
        .unwrap_or_else(|| refresh_token.to_string());
    crate::util::redact::register_secret(&token.access_token);
    crate::util::redact::register_secret(&refresh);
    xbox_chain(client, &token.access_token, refresh).await
}

async fn xbox_chain(client: &HttpClient, ms_token: &str, refresh_token: String) -> Result<Session> {
    // 1. XBL
    #[derive(Deserialize)]
    struct XblResp {
        #[serde(rename = "Token")]
        token: String,
        #[serde(rename = "DisplayClaims")]
        claims: serde_json::Value,
    }
    let xbl: XblResp = client
        .raw()
        .post(XBL_URL)
        .json(&serde_json::json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_token}")
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let uhs = xbl.claims["xui"][0]["uhs"]
        .as_str()
        .ok_or_else(|| LauncherError::Internal("XBL: нет uhs".into()))?
        .to_string();

    // 2. XSTS
    #[derive(Deserialize)]
    struct XstsError {
        #[serde(rename = "XErr")]
        xerr: u64,
    }
    let resp = client
        .raw()
        .post(XSTS_URL)
        .json(&serde_json::json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl.token]
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        if let Ok(e) = resp.json::<XstsError>().await {
            let human = match e.xerr {
                2148916233 => "нет аккаунта Xbox у этого Microsoft-аккаунта",
                2148916238 => "детский аккаунт — Minecraft недоступен",
                2148916235 => "регион не поддерживается Xbox",
                _ => "XSTS отказал во входе",
            };
            return Err(LauncherError::InvalidInput(format!(
                "XSTS {0}: {human}",
                e.xerr
            )));
        }
        return Err(LauncherError::network(format!("XSTS: HTTP {status}")));
    }
    let xsts: XblResp = resp.json().await?;
    let xsts_token = xsts.token;

    // 3. loginWithXbox
    #[derive(Deserialize)]
    struct McLogin {
        access_token: String,
    }
    let mc: McLogin = client
        .raw()
        .post(MC_LOGIN_URL)
        .json(&serde_json::json!({
            "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    crate::util::redact::register_secret(&mc.access_token);

    // 4. Профиль (uuid, ник; 404 = лицензии нет — честная ошибка).
    #[derive(Deserialize)]
    struct Profile {
        id: String,
        name: String,
    }
    let profile: Profile = client
        .raw()
        .get(MC_PROFILE_URL)
        .bearer_auth(&mc.access_token)
        .send()
        .await?
        .error_for_status()
        .map_err(|_| {
            LauncherError::InvalidInput(
                "у этого Microsoft-аккаунта нет профиля Minecraft (лицензия не куплена)".into(),
            )
        })?
        .json()
        .await?;

    Ok(Session {
        access_token: mc.access_token,
        refresh_token,
        player_name: profile.name,
        uuid: profile.id,
    })
}
