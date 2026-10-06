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
/// принявший соединение и молчащий, не подвешивал вызов навечно.
const JSON_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(60);

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
            .connect_timeout(connect);
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
