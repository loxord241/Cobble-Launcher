//! Microsoft-аутентификация: device-code flow → XBL → XSTS → Minecraft
//! (спека §6.8). Refresh-токен живёт в keyring; в логи не попадает
//! (util::redact).

use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

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
    // D62: «сырой» запрос под предохранителями (30 с TTFB, 60 с тело) —
    // молчащий сервер не подвешивает старт входа навечно.
    let resp = client
        .send_timed(
            client
                .raw()
                .post(format!("{AUTH_BASE}/devicecode"))
                .form(&[("client_id", azure_client_id), ("scope", SCOPE)]),
        )
        .await?
        .error_for_status()?;
    client.json_timed::<DeviceCodeStart>(resp).await
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
    // D62: сам poll-send тоже под предохранителем — внешняя обёртка ниже
    // покрывала только ветку успеха (xbox_chain), а молчащий /token подвешивал
    // опрос навечно.
    let resp = client
        .send_timed(client.raw().post(format!("{AUTH_BASE}/token")).form(&[
            ("client_id", azure_client_id),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
        ]))
        .await?;
    if resp.status().is_success() {
        // D62: тело ответа читаем под таймаутом (заголовки могли прийти без тела).
        let token: TokenResponse = client.json_timed(resp).await?;
        let refresh = token
            .refresh_token
            .ok_or_else(|| LauncherError::Internal("MSA не вернул refresh_token".into()))?;
        // D62: внешняя обёртка 120 с снята — xbox_chain теперь страхует каждый
        // свой send/json сам; здесь она покрывала лишь один из путей вызова,
        // а refresh и web-login идут в цепочку без неё. Дубли не нужны.
        let session = xbox_chain(client, &token.access_token, refresh).await?;
        crate::util::redact::register_secret(&session.access_token);
        crate::util::redact::register_secret(&session.refresh_token);
        // Вход завершён — состояние slow_down больше не нужно.
        slow_down_clear(device_code);
        return Ok(Some(session));
    }
    // D62: тело ошибки тоже под таймаутом; сбой чтения — прежний запасной
    // «unknown» (семантика unwrap_or сохранена).
    let err: TokenError = client.json_timed(resp).await.unwrap_or(TokenError {
        error: "unknown".into(),
    });
    match err.error.as_str() {
        "authorization_pending" => {
            // D64 (RFC 8628 §3.5): после slow_down увеличенный интервал
            // сохраняется и на последующих опросах.
            if let Some(extra) = slow_down_extra(device_code) {
                tokio::time::sleep(std::time::Duration::from_secs(extra)).await;
            }
            Ok(None)
        }
        "slow_down" => {
            // D64 (RFC 8628 §3.5): при slow_down интервал растёт (+5 с за
            // каждый, кап 180 с). Цикл опроса живёт у вызывающих (GUI-фронт,
            // CLI), поэтому пауза выдерживается здесь — в общем примитиве
            // poll, оба вызывающих её наследуют.
            let extra = slow_down_bump(device_code);
            tokio::time::sleep(std::time::Duration::from_secs(extra)).await;
            Ok(None)
        }
        other => {
            // Конечная ошибка — состояние slow_down больше не нужно.
            slow_down_clear(device_code);
            Err(LauncherError::InvalidInput(format!(
                "MSA device code: {other}"
            )))
        }
    }
}

const SLOW_DOWN_STEP: u64 = 5;
const SLOW_DOWN_CAP: u64 = 180;

/// Накопленная пауза slow_down per device_code (сек). Состояние переживает
/// отдельные вызовы poll: записи снимаются при успехе/конечной ошибке;
/// брошенный посреди вход оставляет запись (~десятки байт) до конца
/// процесса — приемлемая цена.
fn slow_down_state() -> &'static Mutex<HashMap<String, u64>> {
    static EXTRA: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    EXTRA.get_or_init(|| Mutex::new(HashMap::new()))
}

fn slow_down_extra(device_code: &str) -> Option<u64> {
    slow_down_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(device_code)
        .copied()
}

/// Увеличить паузу на SLOW_DOWN_STEP (кап SLOW_DOWN_CAP), вернуть новую.
fn slow_down_bump(device_code: &str) -> u64 {
    let mut guard = slow_down_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let next = (guard.get(device_code).copied().unwrap_or(0) + SLOW_DOWN_STEP).min(SLOW_DOWN_CAP);
    guard.insert(device_code.to_string(), next);
    next
}

fn slow_down_clear(device_code: &str) {
    slow_down_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(device_code);
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
    // D62: refresh — «сырой» запрос под предохранителями (30 с TTFB / 60 с тело).
    let resp = client
        .send_timed(client.raw().post(format!("{AUTH_BASE}/token")).form(&[
            ("client_id", azure_client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", SCOPE),
        ]))
        .await?;
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "MSA refresh: HTTP {} — требуется повторный вход",
            resp.status()
        )));
    }
    // D62: тело под таймаутом — сервер мог отдать заголовки и замолчать.
    let token: TokenResponse = client.json_timed(resp).await?;
    let refresh = token
        .refresh_token
        .unwrap_or_else(|| refresh_token.to_string());
    crate::util::redact::register_secret(&token.access_token);
    crate::util::redact::register_secret(&refresh);
    xbox_chain(client, &token.access_token, refresh).await
}

async fn xbox_chain(client: &HttpClient, ms_token: &str, refresh_token: String) -> Result<Session> {
    // D62: предохранители стоят на КАЖДОМ send/json цепочки, а не снаружи у
    // одного вызывающего: refresh (запуск игры) и web-login идут сюда без
    // внешних обёрток, дублировать таймауты не нужно. Худший случай —
    // 4×(30 с TTFB + 60 с тело).
    // 1. XBL
    #[derive(Deserialize)]
    struct XblResp {
        #[serde(rename = "Token")]
        token: String,
        #[serde(rename = "DisplayClaims")]
        claims: serde_json::Value,
    }
    let resp = client
        .send_timed(
            client
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
                })),
        )
        .await?
        .error_for_status()?;
    let xbl: XblResp = client.json_timed(resp).await?;
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
        .send_timed(
            client
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
                })),
        )
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        // D62: тело ошибки читаем под таймаутом.
        if let Ok(e) = client.json_timed::<XstsError>(resp).await {
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
    let xsts: XblResp = client.json_timed(resp).await?;
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
        .send_timed(
            client
                .raw()
                .post(MC_LOGIN_URL)
                .header("Accept", "application/json")
                .json(&serde_json::json!({
                    "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
                })),
        )
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        // 403 = провайдер отклонил САМО приложение (аудит 2026-10-06):
        // без обещаний про формы/одобрения — от пользователя это не зависит.
        if status.as_u16() == 403 {
            return Err(LauncherError::network(
                "Xbox Live отклонил приложение (HTTP 403). Повтори вход позже; если повторится — сообщи разработчику",
            ));
        }
        // D62: тело ошибки под таймаутом; сбой чтения — прежняя пустая строка.
        let body = client
            .body_timed(resp)
            .await
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default();
        let hint = if status.as_u16() == 401 {
            " (частая причина — у аккаунта нет Minecraft: Java Edition)"
        } else {
            ""
        };
        return Err(LauncherError::network(format!(
            "login_with_xbox: HTTP {status}, тело ответа: {body}{hint}"
        )));
    }
    let mc: McLogin = client.json_timed(resp).await?;
    crate::util::redact::register_secret(&mc.access_token);

    // 4. Профиль (uuid, ник; 404 = лицензии нет — честная ошибка).
    #[derive(Deserialize)]
    struct Profile {
        id: String,
        name: String,
    }
    let resp = client
        .send_timed(
            client
                .raw()
                .get(MC_PROFILE_URL)
                .bearer_auth(&mc.access_token)
                .header("Accept", "application/json"),
        )
        .await?;
    // D64: 429 — rate limit, а не «нет Minecraft»: раньше попадал в маппинг
    // ниже и обманывал («лицензия не куплена»).
    if resp.status().as_u16() == 429 {
        return Err(LauncherError::network(
            "слишком много запросов к серверу — подожди немного и повтори",
        ));
    }
    let resp = resp.error_for_status().map_err(|_| {
        LauncherError::InvalidInput(
            "у этого Microsoft-аккаунта нет профиля Minecraft (лицензия не куплена)".into(),
        )
    })?;
    let profile: Profile = client.json_timed(resp).await?;

    Ok(Session {
        access_token: mc.access_token,
        refresh_token,
        player_name: profile.name,
        uuid: profile.id,
    })
}

/// URL браузерного входа: auth-code + localhost redirect (device-code не даёт
/// согласия XboxLive для саморегистрированных приложений — спека §6.8).
/// `state` (uuid v4) — CSRF-защита: листнер принимает только redirect,
/// вернувший тот же state.
pub fn authorize_url(client_id: &str, port: u16, state: &str) -> String {
    format!(
        "{AUTH_BASE}/authorize?client_id={client_id}&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A{port}&scope=XboxLive.signin%20offline_access&state={state}"
    )
}

/// Ответ браузеру после принятого redirect'а.
const OAUTH_DONE_PAGE: &str = "HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Connection: close

<html><body style='font-family:sans-serif'><h2>OK</h2><p>close this tab</p></body></html>";

/// Ответ браузеру при redirect'е с неверным/отсутствующим state (CSRF):
/// код из такого запроса не принимается, ожидание продолжается.
const OAUTH_STATE_REJECTED_PAGE: &str = "HTTP/1.1 400 Bad Request
Content-Type: text/html; charset=utf-8
Connection: close

<html><body style='font-family:sans-serif'><h2>Invalid OAuth state</h2><p>close this tab</p></body></html>";

/// Извлечь параметр `?key=` из первой строки запроса redirect'а
/// (percent-decode; пустое значение — то же, что отсутствие). Имя сверяется
/// ЦЕЛИКОМ по '&'-сегментам query (аудит 2026-10-06): подстрочный поиск
/// «code=» ловил бы «xcode=» и отдавал чужое значение.
fn extract_query_param(req: &str, key: &str) -> Option<String> {
    let line = req.lines().next().unwrap_or("");
    let path = line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1;
    for seg in query.split('&') {
        let Some((name, value)) = seg.split_once('=') else {
            continue;
        };
        if name == key {
            let decoded = pct_decode(value);
            if decoded.is_empty() {
                return None;
            }
            return Some(decoded);
        }
    }
    None
}

/// Принять redirect браузера на локальном порту и извлечь ?code=.
/// Асинхронный вариант для GUI (A5/D15): listener живёт прямо в future, поэтому
/// таймаут/отмена вызывающей стороны закрывают сокет вместе с ней — раньше
/// блокирующий `accept` в `spawn_blocking` переживал таймаут и держал порт.
/// CSRF: запрос с отсутствующим или не совпадающим с `expected_state` state
/// получает HTTP 400, и ожидание продолжается (код из него отбрасывается).
pub async fn wait_auth_code_async(
    listener: tokio::net::TcpListener,
    expected_state: &str,
) -> Result<String> {
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
        let code = extract_query_param(&req, "code");
        let state_ok =
            extract_query_param(&req, "state").as_deref() == Some(expected_state);
        // SEC-3: пользователь нажал «Отмену» — Microsoft присылает error=access_denied
        // без code. Раньше мы отвечали браузеру «готово» и продолжали ждать до
        // таймаута; теперь честно прерываем ожидание с ошибкой.
        if state_ok {
            if let Some(err) = extract_query_param(&req, "error") {
                let _ = stream.write_all(OAUTH_DONE_PAGE.as_bytes()).await;
                let _ = stream.shutdown().await;
                return Err(LauncherError::InvalidInput(format!("OAuth: {err}")));
            }
        }
        let page = if state_ok {
            OAUTH_DONE_PAGE
        } else {
            OAUTH_STATE_REJECTED_PAGE
        };
        let _ = stream.write_all(page.as_bytes()).await;
        let _ = stream.shutdown().await;
        if let (Some(code), true) = (code, state_ok) {
            return Ok(code);
        }
    }
}

/// Принять redirect браузера на локальном порту и извлечь ?code=.
/// Блокирующий вариант для CLI (`mcl auth-msa-web`).
/// Проверка state — как в `wait_auth_code_async`.
pub async fn wait_auth_code(
    listener: std::net::TcpListener,
    // String, а не &str: уезжает в spawn_blocking (нужен 'static).
    expected_state: String,
    // D62: дедлайн ожидания подключений — раньше accept крутился без таймаута
    // и молчаливый браузер/файрвол подвешивал CLI навечно.
    accept_timeout: std::time::Duration,
) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        // D62: accept неблокирующий — цикл сам спит 200 мс между попытками и
        // проверяет дедлайн. Чтение уже подключённого потока оставлено
        // блокирующим (клиент уже подключился), но с read_timeout 10 с, чтобы
        // полуоткрытое соединение не держало ожидание вечно.
        listener
            .set_nonblocking(true)
            .map_err(|e| LauncherError::network(format!("listener: {e}")))?;
        let deadline = std::time::Instant::now() + accept_timeout;
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Наследование неблокирующего режима от listener'а
                    // платформозависимо — чтение делаем блокирующим явно.
                    stream
                        .set_nonblocking(false)
                        .map_err(|e| LauncherError::network(format!("stream: {e}")))?;
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                        .map_err(|e| LauncherError::network(format!("stream: {e}")))?;
                    let mut buf = vec![0u8; 8192];
                    let n = match std::io::Read::read(&mut stream, &mut buf) {
                        Ok(0) => continue,
                        Ok(n) => n,
                        // Истёк read_timeout — соединение полуоткрытое: не
                        // роняем всё ожидание, бросаем поток и ждём дальше.
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            continue;
                        }
                        Err(e) => return Err(LauncherError::network(format!("read: {e}"))),
                    };
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let code = extract_query_param(&req, "code");
                    let state_ok = extract_query_param(&req, "state").as_deref()
                        == Some(expected_state.as_str());
                    // SEC-3: как в async-варианте — error от провайдера прерывает ожидание.
                    if state_ok {
                        if let Some(err) = extract_query_param(&req, "error") {
                            let _ =
                                std::io::Write::write_all(&mut stream, OAUTH_DONE_PAGE.as_bytes());
                            let _ = stream.shutdown(std::net::Shutdown::Both);
                            return Err(LauncherError::InvalidInput(format!("OAuth: {err}")));
                        }
                    }
                    let page = if state_ok {
                        OAUTH_DONE_PAGE
                    } else {
                        OAUTH_STATE_REJECTED_PAGE
                    };
                    let _ = std::io::Write::write_all(&mut stream, page.as_bytes());
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    if let (Some(code), true) = (code, state_ok) {
                        return Ok(code);
                    }
                }
                // Никто не подключился: короткая пауза, затем проверка дедлайна.
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    if std::time::Instant::now() >= deadline {
                        return Err(LauncherError::network("таймаут ожидания ответа браузера"));
                    }
                }
                Err(e) => return Err(LauncherError::network(format!("accept: {e}"))),
            }
        }
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
    // D62: обмен кода — «сырой» запрос под предохранителями (30 с TTFB / 60 с тело).
    let resp = client
        .send_timed(client.raw().post(format!("{AUTH_BASE}/token")).form(&[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect.as_str()),
            ("scope", SCOPE),
        ]))
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        // D62: тело ошибки под таймаутом; сбой чтения — прежняя пустая строка.
        let body = client
            .body_timed(resp)
            .await
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default();
        return Err(LauncherError::network(format!(
            "MSA code exchange: HTTP {status}, {body}"
        )));
    }
    // D62: тело под таймаутом.
    let token: TokenResponse = client.json_timed(resp).await?;
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

    /// D64 (RFC 8628): пауза slow_down растёт по +5 с и упирается в кап 180 с;
    /// сброс (успех/конечная ошибка) возвращает её к нулю.
    #[test]
    fn slow_down_bump_grows_with_cap_and_resets() {
        let key = "test-slow-down-key-d64";
        slow_down_clear(key);
        assert_eq!(slow_down_bump(key), 5, "первый slow_down: +5 с");
        assert_eq!(slow_down_bump(key), 10, "второй: ещё +5 с");
        assert_eq!(slow_down_extra(key), Some(10), "текущее состояние читается");
        for _ in 0..40 {
            slow_down_bump(key);
        }
        assert_eq!(slow_down_bump(key), 180, "кап 180 с");
        slow_down_clear(key);
        assert_eq!(slow_down_extra(key), None, "сброс удаляет запись");
        assert_eq!(slow_down_bump(key), 5, "после сброса отсчёт заново");
        slow_down_clear(key);
    }

    /// A5: listener принимает реальный redirect браузера и разбирает `?code=`
    /// при совпадении state.
    #[tokio::test]
    async fn wait_auth_code_async_parses_redirect() {
        use tokio::io::AsyncWriteExt as _;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(wait_auth_code_async(listener, "teststate"));
        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        client
            .write_all(b"GET /?code=abc%20123&state=teststate HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        assert_eq!(task.await.unwrap().unwrap(), "abc 123", "percent-decode сохранён");
    }

    /// CSRF: redirect с неверным state получает HTTP 400 («Invalid OAuth
    /// state») и НЕ завершает ожидание; код принимается только от запроса
    /// с ожидаемым state.
    #[tokio::test]
    async fn oauth_state_mismatch_is_rejected() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(wait_auth_code_async(listener, "teststate"));

        // 1. Чужой код с неверным state: сервер отвечает 400 и продолжает ждать.
        let mut evil = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        evil.write_all(b"GET /?code=evil&state=wrong HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut resp = Vec::new();
        evil.read_to_end(&mut resp).await.unwrap();
        let resp = String::from_utf8_lossy(&resp);
        assert!(resp.contains("400"), "первый ответ должен быть 400: {resp}");
        assert!(
            resp.contains("Invalid OAuth state"),
            "первый ответ без текста про state: {resp}"
        );
        drop(evil);

        // 2. Легитимный redirect с ожидаемым state завершает ожидание.
        let mut good = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        good.write_all(b"GET /?code=good&state=teststate HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        assert_eq!(
            task.await.unwrap().unwrap(),
            "good",
            "код с верным state принят"
        );
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
            wait_auth_code_async(listener, "any"),
        )
        .await;
        assert!(res.is_err(), "таймаут ожидания входа наступил");
        std::net::TcpListener::bind(("127.0.0.1", port))
            .expect("порт освобождён вместе с listener'ом");
    }

    /// Разбор строки запроса: мусор без `code=` — не код, а не пустая строка;
    /// state извлекается тем же способом (пустое = отсутствие).
    #[test]
    fn extract_code_ignores_requests_without_code() {
        assert_eq!(extract_query_param("GET /favicon.ico HTTP/1.1", "code"), None);
        assert_eq!(extract_query_param("GET /?code=&state=x HTTP/1.1", "code"), None);
        assert_eq!(
            extract_query_param("GET /?code=xyz&state=x HTTP/1.1", "code"),
            Some("xyz".into())
        );
        assert_eq!(extract_query_param("GET /?state=&code=x HTTP/1.1", "state"), None);
        assert_eq!(
            extract_query_param("GET /?code=x&state=abc HTTP/1.1", "state"),
            Some("abc".into())
        );
    }

    /// Аудит 2026-10-06: имя параметра сверяется ЦЕЛИКОМ по '&'-сегментам —
    /// «xcode=» не выдаётся за «code=», берётся правильный сегмент.
    #[test]
    fn extract_code_ignores_lookalike_param_names() {
        assert_eq!(
            extract_query_param("GET /?xcode=1&code=real&state=s HTTP/1.1", "code"),
            Some("real".into()),
            "точное имя параметра, а не подстрока"
        );
        assert_eq!(
            extract_query_param("GET /?decode=1&code=real HTTP/1.1", "code"),
            Some("real".into())
        );
        // То же для state: «xstate=» не путается с «state=».
        assert_eq!(
            extract_query_param("GET /?xstate=1&code=c&state=abc HTTP/1.1", "state"),
            Some("abc".into())
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
