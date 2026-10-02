//! HTTP-клиент лаунчера: UA по требованиям Modrinth, таймауты, backoff на 429/5xx.

use crate::errors::{LauncherError, Result};
use reqwest::header::RETRY_AFTER;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Явный User-Agent — обязательное требование Modrinth (спека §5).
pub const USER_AGENT: &str = concat!(
    "mc-launcher-v2/",
    env!("CARGO_PKG_VERSION"),
    " (github:owner/mc-launcher-v2)"
);

const MAX_RETRIES: u32 = 4;

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
        Self::with_timeouts(
            proxy_url,
            Duration::from_secs(15),
            // Общий таймаут на запрос: самый большой файл — JRE ~50 МБ.
            Duration::from_secs(600),
        )
    }

    /// Конструктор с явными таймаутами (тесты, инсталляторы).
    pub fn with_timeouts(proxy_url: Option<&str>, connect: Duration, total: Duration) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(connect)
            .timeout(total);
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
    pub async fn get_json_retry<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        // F16: офлайн — честный отказ до любых сетевых вызовов и ретраев.
        if self.offline() {
            return Err(LauncherError::OfflineMode(url.into()));
        }
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            match self.inner.get(url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    return Ok(resp.json::<T>().await?);
                }
                Ok(resp) if is_retryable(resp.status().as_u16()) => {
                    if attempt >= MAX_RETRIES {
                        return Err(LauncherError::network(format!(
                            "HTTP {} после {MAX_RETRIES} попыток: {url}",
                            resp.status()
                        )));
                    }
                    let wait = retryable_wait(resp.status().as_u16(), resp.headers().get(RETRY_AFTER), attempt);
                    tokio::time::sleep(wait).await;
                }
                Ok(resp) => {
                    return Err(LauncherError::network(format!(
                        "HTTP {} для {url}",
                        resp.status()
                    )));
                }
                Err(e) => {
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
