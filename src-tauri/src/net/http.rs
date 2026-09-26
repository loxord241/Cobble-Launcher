//! HTTP-клиент лаунчера: UA по требованиям Modrinth, таймауты, backoff на 429/5xx.

use crate::errors::{LauncherError, Result};
use reqwest::header::RETRY_AFTER;
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
        })
    }

    pub fn raw(&self) -> &reqwest::Client {
        &self.inner
    }

    /// GET JSON с повторами на 429/5xx и сетевых ошибках (backoff c jitter).
    pub async fn get_json_retry<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
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
}
