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
    let uhs = xsts.claims["xui"][0]["uhs"].as_str().map(String::from)
        .unwrap_or(uhs);
    // 2c. Кто вошёл: второй XSTS с RP http://xboxlive.com — в его claims есть gtg.
    match client
        .raw()
        .post(XSTS_URL)
        .json(&serde_json::json!({
            "Properties": {"SandboxId": "RETAIL", "UserTokens": [xbl.token]},
            "RelyingParty": "http://xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()
        .await
    {
        Ok(r) => match r.json::<serde_json::Value>().await {
            Ok(v) => println!(
                "Вошли как (Xbox): {} (xid {})",
                v["DisplayClaims"]["xui"][0]["gtg"],
                v["DisplayClaims"]["xui"][0]["xid"]
            ),
            Err(e) => println!("xboxlive-RP: parse fail: {e}"),
        },
        Err(e) => println!("xboxlive-RP: request fail: {e}"),
    }

    // 2b. Геймертег вошедшего аккаунта (best effort) — чтобы было видно, КТО вошёл.
    let gamertag: Option<String> = (|| async {
        let r = client
            .raw()
            .get("https://profile.xboxlive.com/users/me/profile/settings?settings=Gamertag")
            .bearer_auth(&xsts_token)
            .header("x-xbl-contract-version", "2")
            .header("Accept", "application/json")
            .send()
            .await
            .ok()?;
        r.json::<serde_json::Value>().await.ok().and_then(|v| {
            v["profileUsers"][0]["settings"]
                .as_array()?
                .iter()
                .find(|s| s["id"] == "Gamertag")
                .and_then(|s| s["value"].as_str())
                .map(String::from)
        })
    })()
    .await;
    if let Some(gt) = &gamertag {
        println!("Вошли как (Xbox): {gt}");
    }

    // 3. loginWithXbox
    #[derive(Deserialize)]
    struct McLogin {
        access_token: String,
    }
    let resp = client
        .raw()
        .post(MC_LOGIN_URL)
        .json(&serde_json::json!({
            "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(LauncherError::network(format!(
            "loginWithXbox: HTTP {status}, тело ответа: {body} (частая причина — у аккаунта нет Minecraft: Java Edition)"
        )));
    }
    let mc: McLogin = resp.json().await?;
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

/// URL браузерного входа: auth-code + localhost redirect (device-code не даёт
/// согласия XboxLive для саморегистрированных приложений — спека §6.8).
pub fn authorize_url(client_id: &str, port: u16) -> String {
    format!(
        "{AUTH_BASE}/authorize?client_id={client_id}&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A{port}&scope=XboxLive.signin%20offline_access"
    )
}

/// Принять redirect браузера на локальном порту и извлечь ?code=.
pub async fn wait_auth_code(
    listener: std::net::TcpListener,
) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(e) => return Err(LauncherError::network(format!("accept: {e}"))),
            };
            let mut buf = vec![0u8; 8192];
            let n = match std::io::Read::read(&mut stream, &mut buf) {
                Ok(0) => continue,
                Ok(n) => n,
                Err(e) => return Err(LauncherError::network(format!("read: {e}"))),
            };
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = req.lines().next().unwrap_or("");
            let code = line
                .split_whitespace()
                .nth(1)
                .and_then(|path| path.split("code=").nth(1))
                .map(|rest| rest.split('&').next().unwrap_or(rest).to_string());
            let page = b"HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Connection: close

<html><body style='font-family:sans-serif'><h2>OK</h2><p>close this tab</p></body></html>";
            let _ = std::io::Write::write_all(&mut stream, page);
            let _ = stream.shutdown(std::net::Shutdown::Both);
            if let Some(code) = code {
                let decoded = pct_decode(&code);
                println!("auth-code получен: len={} head={:?}", decoded.len(), decoded.get(..10));
                if !decoded.is_empty() {
                    return Ok(decoded);
                }
            }
        }
        Err(LauncherError::network("listener закрыт без code"))
    })
    .await
    .map_err(|e| LauncherError::network(format!("join: {e}")))?
}

/// Минимальный percent-decode для параметров запроса.
fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Обменять auth-code на токены и достроить сессию.
pub async fn finish_web_login(
    client: &HttpClient,
    client_id: &str,
    code: &str,
    port: u16,
) -> Result<Session> {
    let redirect = format!("http://localhost:{port}");
    let resp = client
        .raw()
        .post(format!("{AUTH_BASE}/token"))
        .form(&[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect.as_str()),
            ("scope", SCOPE),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(LauncherError::network(format!(
            "MSA code exchange: HTTP {status}, {body}"
        )));
    }
    let token: TokenResponse = resp.json().await?;
    let refresh = token.refresh_token.clone().ok_or_else(|| {
        LauncherError::InvalidInput("MSA не вернул refresh_token (нужен offline_access)".into())
    })?;
    crate::util::redact::register_secret(&token.access_token);
    crate::util::redact::register_secret(&refresh);
    xbox_chain(client, &token.access_token, refresh).await
}
