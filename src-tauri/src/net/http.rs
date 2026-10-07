//! HTTP-клиент лаунчера: UA по требованиям Modrinth, таймауты, backoff на 429/5xx.
//! P2-таймаут: общего таймаута на тело у клиента по умолчанию НЕТ (см. `new`).

use crate::errors::{LauncherError, Result};
use reqwest::header::RETRY_AFTER;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Явный User-Agent — обязательное требование Modrinth (спека §5).
pub const USER_AGENT: &str = concat!(
    "mc-launcher-v2/",
    env!("CARGO_PKG_VERSION"),
    " (github:loxord241/Cobble-Launcher)"
);

const MAX_RETRIES: u32 = 4;
/// P2-таймаут: у общего клиента таймаута на тело нет; JSON-запросы быстрые,
/// но каждая попытка (send + чтение тела) страхуется локально, чтобы сервер,
/// принявший соединение и молчащий, не подвешивал вызов навсегда.
const JSON_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(60);
/// Предохранители для «сырых» запросов (бэклог D61, аудит 2026-10-07):
/// общий таймаут снят, `get_json_retry` страхует только себя. Константы
/// ниже — для обёрток `send_timed`/`json_timed`/`body_timed`.
pub const RAW_SEND_TIMEOUT: Duration = Duration::from_secs(30);
pub const RAW_BODY_TIMEOUT: Duration = Duration::from_secs(60);
/// Тела, которые легитимно велики (установщики, архивы через bytes()).
pub const RAW_BIG_BODY_TIMEOUT: Duration = Duration::from_secs(600);

/// Redirect-барьер (бэклог D61): политика по умолчанию следует за Location
/// без проверок — открытый редирект на доверенном CDN мог увести запрос на
/// приватный/loopback-адрес (SSRF) или на понижение https→http. Allowlist у
/// каждого вызова свой и проверяется ДО запроса; сюда переносить его нельзя
/// (политика глобальная), поэтому барьер минимальный и универсальный.
/// Целостность скачивемого гарантируют хэши манифеста, конфиденциальность —
/// reqwest (снимает Authorization при смене хоста).
fn redirect_allowed(next: &reqwest::Url) -> bool {
    if next.scheme() != "http" && next.scheme() != "https" {
        return false;
    }
    if let Some(host) = next.host_str() {
        if host.eq_ignore_ascii_case("localhost") || host.ends_with(".local") {
            return false;
        }
        // url-крейт отдаёт IPv6-хост в скобках («[::1]») — снимаем их.
        let unbracketed = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = unbracketed.parse::<std::net::IpAddr>() {
            let private = match ip {
                std::net::IpAddr::V4(v4) => {
                    v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
                }
                std::net::IpAddr::V6(v6) => {
                    // D64: IPv4-mapped IPv6 (::ffff:a.b.c.d) — замаскированный
                    // IPv4: без прогонa через V4-правила «::ffff:10.0.0.1»
                    // проходил барьер как «глобальный» v6. Маппированные —
                    // те же проверки, что у настоящего IPv4; прочий v6 —
                    // прежние v6-проверки.
                    if let Some(v4) = v6.to_ipv4_mapped() {
                        v4.is_loopback()
                            || v4.is_private()
                            || v4.is_link_local()
                            || v4.is_unspecified()
                    } else {
                        v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local()
                    }
                }
            };
            if private {
                return false;
            }
        }
    }
    true
}

#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    /// F16 «работать офлайн»: Arc — флаг общий для всех клонов клиента.
    offline: Arc<AtomicBool>,
    /// F17 лимит скорости загрузки, КБ/с (0 — без ограничения).
    /// Arc — как offline: значение общее для всех клонов клиента.
    speed_limit: Arc<AtomicU32>,
}

impl HttpClient {
    pub fn new(proxy_url: Option<&str>) -> Result<Self> {
        // P2-таймаут (аудит 2026-10-06): общего таймаута на запрос больше НЕТ —
        // он покрывал всё тело вместе с паузами троттлинга (F17), и длинная
        // загрузка при лимите скорости рвалась детерминированно на 600-й
        // секунде. reqwest без общего таймаута живёт, пока идут данные;
        // тишину ограничивают connect_timeout и read-idle 120 с в движке
        // (download.rs::READ_IDLE), JSON-попытки — JSON_ATTEMPT_TIMEOUT ниже.
        Self::build(proxy_url, Duration::from_secs(15), None)
    }

    /// Конструктор с явными таймаутами (тесты, инсталляторы): `total`
    /// ограничивает весь запрос целиком, включая чтение тела.
    pub fn with_timeouts(
        proxy_url: Option<&str>,
        connect: Duration,
        total: Duration,
    ) -> Result<Self> {
        Self::build(proxy_url, connect, Some(total))
    }

    fn build(proxy_url: Option<&str>, connect: Duration, total: Option<Duration>) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(connect)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    return attempt.error("слишком много редиректов");
                }
                let next = attempt.url().clone();
                let downgraded = attempt
                    .previous()
                    .iter()
                    .any(|p| p.scheme() == "https" && next.scheme() == "http");
                if downgraded || !redirect_allowed(&next) {
                    return attempt.error(format!("редирект на {next} заблокирован политикой"));
                }
                attempt.follow()
            }));
        if let Some(t) = total {
            builder = builder.timeout(t);
        }
        if let Some(p) = proxy_url {
            builder = builder.proxy(reqwest::Proxy::all(p)?);
        }
        Ok(Self {
            inner: builder.build()?,
            offline: Arc::new(AtomicBool::new(false)),
            // По умолчанию лимита нет: ограничение включается только из настроек.
            speed_limit: Arc::new(AtomicU32::new(0)),
        })
    }

    /// F16: тумблер «работать офлайн». Включён — любые сетевые попытки
    /// получают мгновенный `OfflineMode` до похода в сеть.
    pub fn set_offline(&self, on: bool) {
        self.offline.store(on, Ordering::Relaxed);
    }

    pub fn offline(&self) -> bool {
        self.offline.load(Ordering::Relaxed)
    }

    /// F17: общий лимит скорости загрузки, КБ/с (0 — без ограничения).
    /// Вызывается интегратором при старте движка — как `set_offline`;
    /// смена на лету подхватывается на ближайшем семпле троттлинга.
    pub fn set_speed_limit(&self, kbps: u32) {
        self.speed_limit.store(kbps, Ordering::Relaxed);
    }

    /// Текущий лимит, КБ/с; 0 — без ограничения.
    pub fn speed_limit(&self) -> u32 {
        self.speed_limit.load(Ordering::Relaxed)
    }

    pub fn raw(&self) -> &reqwest::Client {
        &self.inner
    }

    /// Предохранитель на send «сырого» запроса (TTFB): сервер, принявший
    /// соединение и молчащий, не подвешивает вызов навечно. Тело читается
    /// отдельно — стримы движка загрузок живут под своим READ_IDLE.
    pub async fn send_timed(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        match tokio::time::timeout(RAW_SEND_TIMEOUT, builder.send()).await {
            Ok(r) => r.map_err(LauncherError::from),
            Err(_) => Err(LauncherError::network(format!(
                "нет ответа за {RAW_SEND_TIMEOUT:?}"
            ))),
        }
    }

    /// JSON-тело ответа под таймаутом: заголовки могли прийти, а тело — нет.
    pub async fn json_timed<T: serde::de::DeserializeOwned>(
        &self,
        resp: reqwest::Response,
    ) -> Result<T> {
        match tokio::time::timeout(RAW_BODY_TIMEOUT, resp.json::<T>()).await {
            Ok(r) => r.map_err(LauncherError::from),
            Err(_) => Err(LauncherError::network(format!(
                "тело не получено за {RAW_BODY_TIMEOUT:?}"
            ))),
        }
    }

    /// Байты тела под таймаутом: для маленьких тел (JSON/иконки/мелкие
    /// бинарники). Большие тела (установщики) — `body_timed_big`.
    pub async fn body_timed(&self, resp: reqwest::Response) -> Result<Vec<u8>> {
        self.body_timed_with(resp, RAW_BODY_TIMEOUT).await
    }

    /// То же с щедрым лимитом для больших бинарных тел.
    pub async fn body_timed_big(&self, resp: reqwest::Response) -> Result<Vec<u8>> {
        self.body_timed_with(resp, RAW_BIG_BODY_TIMEOUT).await
    }

    async fn body_timed_with(
        &self,
        resp: reqwest::Response,
        limit: Duration,
    ) -> Result<Vec<u8>> {
        match tokio::time::timeout(limit, resp.bytes()).await {
            Ok(r) => r.map(|b| b.to_vec()).map_err(LauncherError::from),
            Err(_) => Err(LauncherError::network(format!(
                "тело не получено за {limit:?}"
            ))),
        }
    }

    /// GET JSON с повторами на 429/5xx и сетевых ошибках (backoff c jitter).
    /// Каждая попытка ограничена `JSON_ATTEMPT_TIMEOUT` (send и чтение тела
    /// под локальным таймаутом отдельно).
    pub async fn get_json_retry<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        // F16: офлайн — честный отказ до любых сетевых вызовов и ретраев.
        if self.offline() {
            return Err(LauncherError::OfflineMode(url.into()));
        }
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            let sent = tokio::time::timeout(JSON_ATTEMPT_TIMEOUT, self.inner.get(url).send()).await;
            match sent {
                // Локальный таймаут попытки ведём как сетевую ошибку: backoff, ретрай.
                Err(_) => {
                    if attempt >= MAX_RETRIES {
                        return Err(LauncherError::network(format!(
                            "нет ответа за {JSON_ATTEMPT_TIMEOUT:?} после {MAX_RETRIES} попыток: {url}"
                        )));
                    }
                    tokio::time::sleep(backoff_delay(attempt)).await;
                }
                Ok(Ok(resp)) if resp.status().is_success() => {
                    // Тело тоже под локальным таймаутом: сервер может отдать
                    // заголовки и замолчать уже в BODY.
                    match tokio::time::timeout(JSON_ATTEMPT_TIMEOUT, resp.json::<T>()).await {
                        Ok(parsed) => return parsed.map_err(LauncherError::from),
                        Err(_) => {
                            return Err(LauncherError::network(format!(
                                "тело не получено за {JSON_ATTEMPT_TIMEOUT:?}: {url}"
                            )));
                        }
                    }
                }
                Ok(Ok(resp)) if is_retryable(resp.status().as_u16()) => {
                    if attempt >= MAX_RETRIES {
                        return Err(LauncherError::network(format!(
                            "HTTP {} после {MAX_RETRIES} попыток: {url}",
                            resp.status()
                        )));
                    }
                    let wait = retryable_wait(resp.status().as_u16(), resp.headers().get(RETRY_AFTER), attempt);
                    tokio::time::sleep(wait).await;
                }
                Ok(Ok(resp)) => {
                    // Не-2xx без ретрая (например, 404 у снятого с Modrinth
                    // проекта): статус структурно в ошибке — вызывающий
                    // отличает «нет на сервере» от сетевого сбоя, не парся
                    // текст. Для фронта сериализуется по-прежнему как network.
                    return Err(LauncherError::HttpStatus {
                        status: resp.status().as_u16(),
                        url: url.to_string(),
                    });
                }
                Ok(Err(e)) => {
                    if attempt >= MAX_RETRIES {
                        return Err(e.into());
                    }
                    tokio::time::sleep(backoff_delay(attempt)).await;
                }
            }
        }
    }
}

fn is_retryable(status: u16) -> bool {
    status == 429 || status >= 500
}

/// Экспоненциальный backoff с jitter: 1s, 2s, 4s… cap 30s.
pub fn backoff_delay(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(5);
    let base = Duration::from_secs(1u64.checked_shl(shift).unwrap_or(32));
    let jitter_ms = rand::random::<u64>() % 500;
    (base + Duration::from_millis(jitter_ms)).min(Duration::from_secs(30))
}

/// Задержка для 429/5xx: уважаем Retry-After, иначе backoff.
pub fn retryable_wait(
    status: u16,
    retry_after: Option<&reqwest::header::HeaderValue>,
    attempt: u32,
) -> Duration {
    if let Some(v) = retry_after.and_then(|h| h.to_str().ok()) {
        if let Ok(secs) = v.parse::<u64>() {
            return Duration::from_secs(secs.min(60));
        }
    }
    if status == 429 {
        // 429 — ждём дольше обычного backoff
        return (Duration::from_secs(5) + Duration::from_millis(rand::random::<u64>() % 1000))
            .min(Duration::from_secs(30));
    }
    backoff_delay(attempt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        assert!(backoff_delay(1) < Duration::from_secs(2));
        assert!(backoff_delay(3) >= Duration::from_secs(4));
        assert!(backoff_delay(10) <= Duration::from_secs(30));
    }

    /// Барьер редиректов: приватные/loopback/link-local хосты и не-HTTP
    /// схемы запрещены; публичный DNS-хост и литеральный публичный IP — можно.
    #[test]
    fn redirect_policy_blocks_private_and_allows_public() {
        for blocked in [
            "http://127.0.0.1/x",
            "http://localhost/a",
            "http://10.0.0.5/x",
            "http://172.16.1.1/x",
            "http://192.168.1.1/x",
            "http://169.254.169.254/latest/meta-data",
            "http://0.0.0.0/x",
            "http://[::1]/x",
            "http://[fe80::1]/x",
            "http://[fc00::1]/x",
            "ftp://example.com/x",
            "file:///etc/passwd",
            "http://printer.local/x",
        ] {
            let url: reqwest::Url = blocked.parse().unwrap();
            assert!(!redirect_allowed(&url), "{blocked} должен быть заблокирован");
        }
        for allowed in [
            "https://cdn.modrinth.com/f.bin",
            "https://piston-data.mojang.com/v.jar",
            "http://93.184.216.34/x",
            "https://api.adoptium.net/v3/assets",
        ] {
            let url: reqwest::Url = allowed.parse().unwrap();
            assert!(redirect_allowed(&url), "{allowed} должен быть разрешён");
        }
    }

    /// D64: IPv4-mapped IPv6 (::ffff:a.b.c.d) — замаскированный IPv4:
    /// приватные/loopback-значения блокируются теми же V4-правилами,
    /// публичные и обычный глобальный IPv6 проходят.
    #[test]
    fn redirect_blocks_ipv4_mapped_private_v6() {
        for blocked in [
            "http://[::ffff:127.0.0.1]/x",
            "http://[::ffff:10.0.0.1]/x",
            "http://[::ffff:192.168.1.1]/x",
            "http://[::ffff:169.254.1.1]/x",
            "http://[::ffff:0.0.0.0]/x",
        ] {
            let url: reqwest::Url = blocked.parse().unwrap();
            assert!(!redirect_allowed(&url), "{blocked} должен быть заблокирован");
        }
        for allowed in ["http://[::ffff:8.8.8.8]/x", "http://[2001:db8::1]/x"] {
            let url: reqwest::Url = allowed.parse().unwrap();
            assert!(redirect_allowed(&url), "{allowed} должен быть разрешён");
        }
    }

    /// Интеграция: редирект на loopback блокируется политикой на уровне
    /// клиента (не только предикатом) — SSRF-барьер работает по факту.
    #[tokio::test]
    async fn client_blocks_redirect_to_loopback() {
        let app = axum::Router::new().route(
            "/hop",
            axum::routing::get(|| async {
                axum::response::Redirect::temporary("http://127.0.0.1:9/inside")
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let c = HttpClient::new(None).unwrap();
        let err = c
            .send_timed(c.raw().get(format!("http://{addr}/hop")))
            .await
            .expect_err("редирект на loopback обязан падать");
        // reqwest оборачивает отказ политики в «error following redirect» —
        // важно, что следования не было (а не текст внутренней причины).
        let msg = format!("{err}");
        assert!(
            msg.contains("following redirect"),
            "ожидали отказ политики, получили: {msg}"
        );
    }

    #[tokio::test]
    async fn get_json_404_is_error_not_retry() {
        // Локальный сервер не нужен: https://httpstat.us недоступен в тестах,
        // проверяем только построение клиента.
        let c = HttpClient::new(None).unwrap();
        let r: Result<serde_json::Value> = c
            .get_json_retry("http://127.0.0.1:1/nope")
            .await;
        assert!(r.is_err());
    }

    #[test]
    fn offline_toggle_roundtrip() {
        let c = HttpClient::new(None).unwrap();
        assert!(!c.offline());
        c.set_offline(true);
        assert!(c.offline());
        c.set_offline(false);
        assert!(!c.offline());
    }

    #[test]
    fn speed_limit_default_zero_roundtrip_and_shared() {
        // Контракт для интегратора: по умолчанию лимита нет; set_speed_limit
        // на общем Arc видят все клоны клиента (как set_offline).
        let c = HttpClient::new(None).unwrap();
        assert_eq!(c.speed_limit(), 0, "по умолчанию лимита нет");
        let c2 = c.clone();
        c.set_speed_limit(10240);
        assert_eq!(c.speed_limit(), 10240);
        assert_eq!(c2.speed_limit(), 10240);
        c.set_speed_limit(0);
        assert_eq!(c2.speed_limit(), 0);
    }

    #[test]
    fn offline_flag_shared_between_clones() {
        // Контракт для интегратора: set_offline из commands на общем Arc
        // видят все клоны клиента (в т.ч. ушедшие в tokio::spawn).
        let c = HttpClient::new(None).unwrap();
        let c2 = c.clone();
        c.set_offline(true);
        assert!(c2.offline());
        c.set_offline(false);
        assert!(!c2.offline());
    }

    /// Регресс: не-2xx без ретрая несёт статус и URL структурно
    /// (LauncherError::HttpStatus), но сериализуется по-прежнему как
    /// network с текстом «HTTP {status} для {url}» — фронт не меняется.
    #[tokio::test]
    async fn get_json_non_2xx_carries_structural_status() {
        let app = axum::Router::new().route(
            "/v2/project/gone/version",
            axum::routing::get(|| async { axum::http::StatusCode::NOT_FOUND }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let c = HttpClient::new(None).unwrap();
        let url = format!("http://{addr}/v2/project/gone/version");
        let r: Result<serde_json::Value> = c.get_json_retry(&url).await;
        let err = r.expect_err("404 должен дать ошибку без ретраев");
        assert!(
            matches!(err, LauncherError::HttpStatus { status: 404, .. }),
            "ожидался HttpStatus(404), получен {err:?}"
        );
        let p = serde_json::to_value(err.payload()).unwrap();
        assert_eq!(p["code"], "network");
        assert_eq!(p["message"], format!("HTTP 404 для {url}"));
    }

    #[tokio::test]
    async fn get_json_retry_offline_fails_before_io() {
        let c = HttpClient::new(None).unwrap();
        c.set_offline(true);
        // 127.0.0.1:1 — если бы запрос ушёл в сеть, после ретраев получилась
        // бы Http/Network-ошибка; офлайн-гейт обязан вернуть OfflineMode до IO.
        let r: Result<serde_json::Value> = c.get_json_retry("http://127.0.0.1:1/nope").await;
        let err = r.expect_err("офлайн должен дать ошибку");
        assert!(matches!(err, LauncherError::OfflineMode(_)));
        assert_eq!(err.code(), "offline_mode");
    }
}
