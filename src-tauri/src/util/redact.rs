//! Глобальный реестр секретов для redaction в логах (спека §11):
//! каждый полученный токен регистрируется; аппендер заменяет вхождения
//! на `[REDACTED]`.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// Реестр секретов отдельным типом, а не голым static — тесты работают с
/// ЛОКАЛЬНЫМ экземпляром и не зависят от порядка/параллельности других тестов
/// в процессе (A43: глобальный реестр — общий ресурс на весь бинарь тестов).
#[derive(Default)]
struct SecretRegistry(HashSet<String>);

impl SecretRegistry {
    /// Секреты короче 8 символов не регистрируем: слишком много ложных
    /// срабатываний на обычном тексте логов.
    fn register(&mut self, secret: &str) {
        if secret.len() >= 8 {
            self.0.insert(secret.to_string());
        }
    }

    fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        for secret in &self.0 {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), "[REDACTED]");
            }
        }
        out
    }
}

fn registry() -> &'static Mutex<SecretRegistry> {
    static REG: OnceLock<Mutex<SecretRegistry>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(SecretRegistry::default()))
}

/// Зарегистрировать секрет (токен) — его вхождения в логах будут затёрты.
pub fn register_secret(secret: &str) {
    registry().lock().unwrap().register(secret);
}

/// Заменить зарегистрированные секреты в тексте на [REDACTED].
pub fn redact(text: &str) -> String {
    registry().lock().unwrap().redact(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_registered_secret() {
        let mut reg = SecretRegistry::default();
        reg.register("SUPERSECRETVALUE123");
        assert_eq!(
            reg.redact("Authorization: Bearer SUPERSECRETVALUE123 ok"),
            "Authorization: Bearer [REDACTED] ok"
        );
    }

    #[test]
    fn short_values_ignored() {
        let reg = SecretRegistry::default();
        assert_eq!(reg.redact("token=abc"), "token=abc");
        let mut reg = SecretRegistry::default();
        reg.register("abc");
        assert_eq!(reg.redact("token=abc"), "token=abc", "короче 8 — не секрет");
    }

    #[test]
    fn unregistered_text_passes_through() {
        let mut reg = SecretRegistry::default();
        reg.register("KNOWN-SECRET-1234");
        assert_eq!(reg.redact("nothing to hide"), "nothing to hide");
    }

    /// Прод-API (глобальный реестр) обязан работать; значение уникально, чтобы
    /// не влиять на другие тесты процесса (A43).
    #[test]
    fn global_registry_redacts_registered() {
        register_secret("A43-GLOBAL-REGISTRY-SECRET-4471");
        assert_eq!(
            redact("token=A43-GLOBAL-REGISTRY-SECRET-4471"),
            "token=[REDACTED]"
        );
    }
}
