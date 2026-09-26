//! Глобальный реестр секретов для redaction в логах (спека §11):
//! каждый полученный токен регистрируется; аппендер заменяет вхождения
//! на `[REDACTED]`.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

fn registry() -> &'static Mutex<HashSet<String>> {
    static REG: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Зарегистрировать секрет (токен) — его вхождения в логах будут затёрты.
pub fn register_secret(secret: &str) {
    if secret.len() >= 8 {
        registry().lock().unwrap().insert(secret.to_string());
    }
}

/// Заменить зарегистрированные секреты в тексте на [REDACTED].
pub fn redact(text: &str) -> String {
    let reg = registry().lock().unwrap();
    let mut out = text.to_string();
    for secret in reg.iter() {
        if out.contains(secret.as_str()) {
            out = out.replace(secret.as_str(), "[REDACTED]");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_registered_secret() {
        register_secret("SUPERSECRETVALUE123");
        let out = redact("Authorization: Bearer SUPERSECRETVALUE123 ok");
        assert_eq!(out, "Authorization: Bearer [REDACTED] ok");
    }

    #[test]
    fn short_values_ignored() {
        let out = redact("token=abc");
        assert_eq!(out, "token=abc");
    }
}
