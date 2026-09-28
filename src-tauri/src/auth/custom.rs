//! Свой authlib-injector сервер (F13): общий случай ely.rs — тот же протокол
//! (authserver + алиас /api/authlib-injector), но базовый URL приходит от
//! пользователя, а не зашит. ely.by остаётся отдельным видом аккаунта
//! (auth/ely.rs не трогаем); jar инжектора общий — качается один раз в тот же
//! кэш-файл (ely::authlib_injector_jar).

use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use serde::{Deserialize, Serialize};

const CLIENT_TOKEN: &str = "mc-launcher-v2";
/// Алиас метаданных authlib-injector: как у ely (INJECTOR_SERVER = {root}/api/authlib-injector).
pub const INJECTOR_ALIAS_SUFFIX: &str = "/api/authlib-injector";

/// Публичные метаданные сервера для UI (camelCase — контракт types.ts AuthlibServerInfo).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub server_url: String,
    pub server_name: Option<String>,
}

/// Валидация и нормализация URL сервера (IPC-граница): только https://;
/// http:// — исключительно localhost/127.0.0.1/[::1] (LAN-серверы сообществ).
/// Путь не допускается (алиас /api/authlib-injector лаунчер достраивает сам),
/// userinfo, query и fragment запрещены; хвостовой / убирается, host — в
/// нижний регистр, домен ≤ 253 символов.
pub fn normalize_server_url(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(LauncherError::InvalidInput("URL сервера пуст".into()));
    }
    let url = reqwest::Url::parse(trimmed)
        .map_err(|_| LauncherError::InvalidInput(format!("некорректный URL: {trimmed}")))?;
    let host = url.host_str().unwrap_or_default();
    let is_local = matches!(
        host.trim_start_matches('[').trim_end_matches(']'),
        "localhost" | "127.0.0.1" | "::1"
    );
    match url.scheme() {
        "https" => {}
        "http" if is_local => {}
        "http" => {
            return Err(LauncherError::InvalidInput(
                "http допустим только для localhost/127.0.0.1 — для остальных серверов https://"
                    .into(),
            ))
        }
        other => {
            return Err(LauncherError::InvalidInput(format!(
                "схема «{other}:» не поддерживается — используйте https://"
            )))
        }
    }
    if host.is_empty() {
        return Err(LauncherError::InvalidInput("в URL нет домена".into()));
    }
    // Ограничение RFC для домена — 253 символа (IPv4/IPv6 короче по определению).
    if host.len() > 253 {
        return Err(LauncherError::InvalidInput(
            "домен длиннее 253 символов".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(LauncherError::InvalidInput(
            "логин/пароль в URL не допускаются".into(),
        ));
    }
    if url.path() != "/" {
        return Err(LauncherError::InvalidInput(
            "укажите корень сервера без пути — алиас /api/authlib-injector лаунчер достроит сам"
                .into(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(LauncherError::InvalidInput(
            "URL не должен содержать ?query или #fragment".into(),
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// Алиас сервера для -javaagent: {base}/api/authlib-injector (как ely INJECTOR_SERVER).
pub fn injector_alias(base: &str) -> String {
    format!("{base}{INJECTOR_ALIAS_SUFFIX}")
}

/// JVM-аргумент `-javaagent:путь=алиас` для запуска с аккаунтом своего
/// authlib-сервера (по образцу ely::javaagent_arg).
pub fn build_agent_arg(jar: &std::path::Path, server_url: &str) -> String {
    format!("-javaagent:{}={}", jar.display(), injector_alias(server_url))
}

/// Имя сервера из metadata (serverName по спеке authlib-injector опционален).
fn parse_server_name(value: &serde_json::Value) -> Option<String> {
    value
        .get("meta")?
        .get("serverName")?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Метаданные authlib-injector сервера (GET): сначала канонический алиас
/// {url}/api/authlib-injector (как у ely), резервно — корень {url} (часть
/// серверов отдаёт metadata там). serverName опционален — его отсутствие не
/// ошибка. URL нормализуется: UI получает и показывает ровно то, что сохранится.
pub async fn server_info(client: &HttpClient, server_url: &str) -> Result<ServerInfo> {
    let base = normalize_server_url(server_url)?;
    // F16: офлайн-режим — проверка сервера требует сеть, честный отказ сразу.
    if client.offline() {
        return Err(LauncherError::OfflineMode(format!("метаданные {base}")));
    }
    // Один GET на кандидата без ретраев: кнопка «Проверить» в UI ждёт ответа,
    // backoff против опечатанного адреса — минуты тишины (как POST-логин у ely).
    let mut last_err = LauncherError::NotFound(format!("сервер {base} не отвечает"));
    for candidate in [injector_alias(&base), base.clone()] {
        let resp = match client.raw().get(&candidate).send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = LauncherError::network(format!("{candidate}: {e}"));
                continue;
            }
        };
        let status = resp.status();
        if !status.is_success() {
            last_err = LauncherError::network(format!("HTTP {status} для {candidate}"));
            continue;
        }
        let value: serde_json::Value = resp.json().await?;
        return Ok(ServerInfo {
            server_url: base,
            server_name: parse_server_name(&value),
        });
    }
    Err(last_err)
}

/// Логин по логину/паролю: POST {base}/api/auth/authenticate — то же тело,
/// что у ely.rs (agent/username/password/clientToken). Сессия — общий тип
/// ely::Session (у authlib-серверов refreshToken обычно равен accessToken).
pub async fn login(
    client: &HttpClient,
    server_url: &str,
    username: &str,
    password: &str,
) -> Result<crate::auth::ely::Session> {
    let base = normalize_server_url(server_url)?;
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
        return Err(LauncherError::OfflineMode(format!("вход {base}")));
    }
    let resp: AuthResp = client
        .raw()
        .post(format!("{base}/api/auth/authenticate"))
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
        .map_err(|e| {
            LauncherError::InvalidInput(format!("{base}: неверный логин/пароль ({e})"))
        })?
        .json()
        .await?;
    crate::util::redact::register_secret(&resp.access_token);
    Ok(crate::auth::ely::Session {
        // Как у ely: refreshToken отдельным полем не приходит — при refresh
        // используем accessToken.
        access_token: resp.access_token.clone(),
        refresh_token: resp.access_token,
        player_name: resp.profile.name,
        uuid: resp.profile.id,
    })
}

/// Обновить сессию: POST {base}/api/auth/refresh (как у ely.rs).
pub async fn refresh_session(
    client: &HttpClient,
    server_url: &str,
    refresh_token: &str,
) -> Result<crate::auth::ely::Session> {
    let base = normalize_server_url(server_url)?;
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
    // F16: офлайн-режим — refresh требует сеть, честный отказ сразу.
    if client.offline() {
        return Err(LauncherError::OfflineMode(format!("refresh {base}")));
    }
    let resp: RefreshResp = client
        .raw()
        .post(format!("{base}/api/auth/refresh"))
        .json(&serde_json::json!({
            "accessToken": refresh_token,
            "clientToken": CLIENT_TOKEN
        }))
        .send()
        .await?
        .error_for_status()
        .map_err(|e| LauncherError::network(format!("{base} refresh: {e}")))?
        .json()
        .await?;
    crate::util::redact::register_secret(&resp.access_token);
    Ok(crate::auth::ely::Session {
        access_token: resp.access_token.clone(),
        refresh_token: resp.access_token,
        player_name: resp.profile.name,
        uuid: resp.profile.id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_keeps_https_root() {
        assert_eq!(
            normalize_server_url("https://auth.example.org").unwrap(),
            "https://auth.example.org"
        );
        // Хвостовой слэш и регистр домена нормализуются.
        assert_eq!(
            normalize_server_url("https://Auth.Example.org/").unwrap(),
            "https://auth.example.org"
        );
        // Пробелы по краям обрезаются.
        assert_eq!(
            normalize_server_url("  https://skin.example.net  ").unwrap(),
            "https://skin.example.net"
        );
        // Порт сохраняется (не дефолтный).
        assert_eq!(
            normalize_server_url("https://auth.example.org:8443").unwrap(),
            "https://auth.example.org:8443"
        );
    }

    #[test]
    fn normalize_allows_http_only_for_localhost() {
        assert_eq!(
            normalize_server_url("http://127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080"
        );
        assert_eq!(
            normalize_server_url("http://localhost").unwrap(),
            "http://localhost"
        );
        assert_eq!(
            normalize_server_url("http://[::1]:8080").unwrap(),
            "http://[::1]:8080"
        );
        // http для внешнего домена — отказ.
        assert!(normalize_server_url("http://auth.example.org").is_err());
    }

    #[test]
    fn normalize_rejects_bad_input() {
        for bad in [
            "",
            "   ",
            "ftp://auth.example.org",
            "https://",
            "auth.example.org",
            "javascript:alert(1)",
            // Путь выше корня — алиас лаунчер достраивает сам.
            "https://auth.example.org/api",
            "https://auth.example.org/authlib-injector",
            "https://auth.example.org/?x=1",
            "https://auth.example.org/#frag",
            // user:pass@host
            "https://user:pass@auth.example.org",
        ] {
            assert!(
                normalize_server_url(bad).is_err(),
                "должен быть отклонён: {bad:?}"
            );
        }
    }

    #[test]
    fn normalize_enforces_domain_length() {
        let ok = format!("https://{}.org", "a".repeat(249)); // 249 + 4 = 253
        assert!(normalize_server_url(&ok).is_ok());
        let too_long = format!("https://{}.org", "a".repeat(250)); // 254
        assert!(normalize_server_url(&too_long).is_err());
    }

    #[test]
    fn server_name_from_metadata_json() {
        let with_name: serde_json::Value =
            serde_json::from_str(r#"{"meta":{"serverName":"My Server"}}"#).unwrap();
        assert_eq!(
            parse_server_name(&with_name),
            Some("My Server".to_string())
        );
        // serverName опционален; пустая строка — как отсутствие.
        let without: serde_json::Value = serde_json::from_str(r#"{"meta":{}}"#).unwrap();
        assert_eq!(parse_server_name(&without), None);
        let empty: serde_json::Value =
            serde_json::from_str(r#"{"meta":{"serverName":"  "}}"#).unwrap();
        assert_eq!(parse_server_name(&empty), None);
        let junk: serde_json::Value = serde_json::from_str(r#"{"error":"nope"}"#).unwrap();
        assert_eq!(parse_server_name(&junk), None);
    }

    #[test]
    fn agent_arg_uses_injector_alias() {
        assert_eq!(
            injector_alias("https://auth.example.org"),
            "https://auth.example.org/api/authlib-injector"
        );
        let arg = build_agent_arg(std::path::Path::new("C:\\cache\\authlib-injector.jar"), "https://auth.example.org");
        assert_eq!(
            arg,
            "-javaagent:C:\\cache\\authlib-injector.jar=https://auth.example.org/api/authlib-injector"
        );
    }

    /// F16: офлайн-гейт срабатывает до сети — metadata не проверяется, login
    /// не уходит (сеть в юнит-тестах недоступна по определению).
    #[tokio::test]
    async fn server_info_offline_fails_before_io() {
        let client = HttpClient::new(None).unwrap();
        client.set_offline(true);
        let err = server_info(&client, "https://auth.example.org")
            .await
            .expect_err("офлайн должен дать ошибку");
        assert!(matches!(err, LauncherError::OfflineMode(_)));
    }

    #[tokio::test]
    async fn login_offline_fails_before_io() {
        let client = HttpClient::new(None).unwrap();
        client.set_offline(true);
        let err = login(&client, "https://auth.example.org", "user", "pass")
            .await
            .expect_err("офлайн должен дать ошибку");
        assert!(matches!(err, LauncherError::OfflineMode(_)));
    }

    /// Некорректный URL отклоняется до любых сетевых вызовов — даже без
    /// офлайн-режима (иначе 127.0.0.1:1 походил бы в сеть из теста).
    #[tokio::test]
    async fn server_info_rejects_bad_url_before_io() {
        let client = HttpClient::new(None).unwrap();
        let err = server_info(&client, "ftp://auth.example.org")
            .await
            .expect_err("ftp должен дать ошибку");
        assert!(matches!(err, LauncherError::InvalidInput(_)));
    }
}
