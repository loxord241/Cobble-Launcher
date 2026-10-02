//! Microsoft-аутентификация: device-code flow → XBL → XSTS → Minecraft
//! (спека §6.8). Refresh-токен живёт в keyring; в логи не попадает
//! (util::redact).

use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use serde::{Deserialize, Serialize};

const AUTH_BASE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
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
    // F16/A61: офлайн-режим — вход требует сеть, честный отказ сразу
    // (как в ely.rs/custom.rs), а не висяк до сетевого таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode("вход Microsoft".into()));
    }
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
    // F16/A61: опрос токена — тоже сеть; офлайн отталкиваем до IO.
    if client.offline() {
        return Err(LauncherError::OfflineMode("device-code MSA".into()));
    }
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
pub async fn refresh_session(
    client: &HttpClient,
    refresh_token: &str,
    azure_client_id: &str,
) -> Result<Session> {
    // F16/A61: авто-refresh зовётся при старте и запуске игры — в офлайне
    // обязан отказать мгновенно, иначе запуск висит до connect-таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode("refresh MSA".into()));
    }
    // client_id передаёт вызывающий — из ФАКТИЧЕСКОГО корня данных,
    // а не default_root (ломалось при кастомном каталоге, D4).
    let resp = client
        .raw()
        .post(format!("{AUTH_BASE}/token"))
        .form(&[
            ("client_id", azure_client_id),
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
        // Вики (Microsoft authentication): эти эндпоинты «ругаются», если нет
        // Content-Type: application/json (даёт .json()) и Accept.
        .header("Accept", "application/json")
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
        .header("Accept", "application/json")
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

    // 3. login_with_xbox (актуальный эндпоинт /authentication/login_with_xbox —
    // старый /minecraft/loginWithXbox для кастомных Azure-приложений отвечает
    // 401 {"path":...}; канон — вики «Microsoft authentication» + все лаунчеры).
    #[derive(Deserialize)]
    struct McLogin {
        access_token: String,
    }
    let resp = client
        .raw()
        .post(MC_LOGIN_URL)
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        // 403 = Azure-приложению не выдано разрешение на Minecraft API
        // (новым приложениям нужна заявка Mojang — вики, раздел про Azure).
        let hint = if status.as_u16() == 403 {
            " (Azure-приложению не выдан доступ к Minecraft API — нужна заявка Mojang на использование api.minecraftservices.com)"
        } else if status.as_u16() == 401 {
            " (частая причина — у аккаунта нет Minecraft: Java Edition)"
        } else {
            ""
        };
        return Err(LauncherError::network(format!(
            "login_with_xbox: HTTP {status}, тело ответа: {body}{hint}"
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
        .header("Accept", "application/json")
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

/// Ответ браузеру после принятого redirect'а.
const OAUTH_DONE_PAGE: &str = "HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Connection: close

<html><body style='font-family:sans-serif'><h2>OK</h2><p>close this tab</p></body></html>";

/// Извлечь `?code=` из первой строки запроса redirect'а (percent-decode).
fn extract_code(req: &str) -> Option<String> {
    let line = req.lines().next().unwrap_or("");
    let raw = line
        .split_whitespace()
        .nth(1)
        .and_then(|path| path.split("code=").nth(1))
        .map(|rest| rest.split('&').next().unwrap_or(rest))?;
    let decoded = pct_decode(raw);
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

/// Принять redirect браузера на локальном порту и извлечь ?code=.
/// Асинхронный вариант для GUI (A5/D15): listener живёт прямо в future, поэтому
/// таймаут/отмена вызывающей стороны закрывают сокет вместе с ней — раньше
/// блокирующий `accept` в `spawn_blocking` переживал таймаут и держал порт.
pub async fn wait_auth_code_async(listener: tokio::net::TcpListener) -> Result<String> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|e| LauncherError::network(format!("accept: {e}")))?;
        let mut buf = vec![0u8; 8192];
        let n = match stream.read(&mut buf).await {
            Ok(0) => continue,
            Ok(n) => n,
            Err(e) => return Err(LauncherError::network(format!("read: {e}"))),
        };
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let code = extract_code(&req);
        let _ = stream.write_all(OAUTH_DONE_PAGE.as_bytes()).await;
        let _ = stream.shutdown().await;
        if let Some(code) = code {
            return Ok(code);
        }
    }
}

/// Принять redirect браузера на локальном порту и извлечь ?code=.
/// Блокирующий вариант для CLI (`mcl auth-msa-web`): там процесс живёт до
/// ответа браузера, закрывать сокет по таймауту не требуется.
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
            let code = extract_code(&req);
            let _ = std::io::Write::write_all(&mut stream, OAUTH_DONE_PAGE.as_bytes());
            let _ = stream.shutdown(std::net::Shutdown::Both);
            if let Some(code) = code {
                return Ok(code);
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
    // F16/A61: обмен кода на токены — сеть; офлайн-гейт до IO.
    if client.offline() {
        return Err(LauncherError::OfflineMode("вход Microsoft (web)".into()));
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A5: listener принимает реальный redirect браузера и разбирает `?code=`.
    #[tokio::test]
    async fn wait_auth_code_async_parses_redirect() {
        use tokio::io::AsyncWriteExt as _;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(wait_auth_code_async(listener));
        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        client
            .write_all(b"GET /?code=abc%20123&state=x HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        assert_eq!(task.await.unwrap().unwrap(), "abc 123", "percent-decode сохранён");
    }

    /// A5/D15: по таймауту future отменяется и сокет закрывается — порт
    /// свободен. Раньше блокирующий accept в spawn_blocking переживал таймаут
    /// и держал порт до конца процесса.
    #[tokio::test]
    async fn timeout_closes_oauth_listener() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let res = tokio::time::timeout(
            Duration::from_millis(100),
            wait_auth_code_async(listener),
        )
        .await;
        assert!(res.is_err(), "таймаут ожидания входа наступил");
        std::net::TcpListener::bind(("127.0.0.1", port))
            .expect("порт освобождён вместе с listener'ом");
    }

    /// Разбор строки запроса: мусор без `code=` — не код, а не пустая строка.
    #[test]
    fn extract_code_ignores_requests_without_code() {
        assert_eq!(extract_code("GET /favicon.ico HTTP/1.1"), None);
        assert_eq!(extract_code("GET /?code=&state=x HTTP/1.1"), None);
        assert_eq!(
            extract_code("GET /?code=xyz&state=x HTTP/1.1"),
            Some("xyz".into())
        );
    }

    /// A61/F16: все публичные входы MSA гейтятся офлайном до любых сетевых
    /// вызовов (сеть = 127.0.0.1:1; если бы запрос ушёл, ошибка была бы
    /// Http/Network, а не OfflineMode).
    #[tokio::test]
    async fn msa_entries_offline_fail_before_io() {
        let client = HttpClient::new(None).unwrap();
        client.set_offline(true);

        let err = device_code_start(&client, "client-id")
            .await
            .expect_err("device_code_start должен отказать офлайн");
        assert!(matches!(err, LauncherError::OfflineMode(_)));

        let err = device_code_poll(&client, "client-id", "dc")
            .await
            .expect_err("device_code_poll должен отказать офлайн");
        assert!(matches!(err, LauncherError::OfflineMode(_)));

        let err = refresh_session(&client, "refresh", "client-id")
            .await
            .expect_err("refresh_session должен отказать офлайн");
        assert!(matches!(err, LauncherError::OfflineMode(_)));

        let err = finish_web_login(&client, "client-id", "code", 12345)
            .await
            .expect_err("finish_web_login должен отказать офлайн");
        assert!(matches!(err, LauncherError::OfflineMode(_)));
    }
}
