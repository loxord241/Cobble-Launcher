//! Глобальный реестр секретов для redaction в логах (спека §11):
//! каждый полученный токен регистрируется; аппендер заменяет вхождения
//! на `[REDACTED]`.

use std::collections::HashSet;
use std::path::Path;
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

/// D72: заменить в тексте все вхождения префикса пути `prefix` на `mask`.
/// Служит маскировке личных путей перед отправкой логов на публичные
/// paste-сервисы (mclo.gs, см. `mclogs::sanitize_log_for_public`).
///
/// Особенности Windows-путей учтены: регистр не важен (`(?i)`), ориентация
/// слэшей не важна (разделитель в паттерне — `[/\\]`), `\\?\`-префикс длинных
/// путей не мешает (совпадение ищется в любом месте строки). Граница
/// совпадения: за префиксом обязаны идти НЕ буква/цифра (разделитель, пробел,
/// кавычка) или конец текста — иначе `C:\Users\Vasya` замаскировал бы и чужой
/// `C:\Users\Vasya2`. Lookahead крейт `regex` не поддерживает, поэтому
/// граничный символ захватывается группой и возвращается на место через `$1`;
/// маска вставляется литерально (`$` в ней экранируется как `$$`).
pub fn mask_path_prefix(text: &str, prefix: &Path, mask: &str) -> String {
    let raw = crate::util::fs::normalize_for_compare(prefix)
        .to_string_lossy()
        .replace('/', "\\");
    // Сегменты пути; пустые (хвостов-/начальный разделитель) отбрасываем:
    // префиксы из paths/home_dir не заканчиваются разделителем, а «голое C:»
    // в паттерне замаскировало бы лишнее.
    let segments: Vec<String> = raw
        .split('\\')
        .filter(|s| !s.is_empty())
        .map(regex::escape)
        .collect();
    if segments.is_empty() {
        return text.to_string();
    }
    let body = segments.join(r"[/\\]");
    let pattern = format!(r"(?i){}([^\p{{L}}\p{{N}}]|$)", body);
    let Ok(re) = regex::Regex::new(&pattern) else {
        // Регекс из escape-сегментов не может не собраться; на всякий случай —
        // текст без маскировки, а не паника.
        return text.to_string();
    };
    let replacement = format!("{}$1", mask.replace('$', "$$"));
    re.replace_all(text, replacement.as_str()).into_owned()
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

    // ---------- D72: mask_path_prefix ----------

    /// Оба направления слэшей и оба регистра маскируются; разделитель после
    /// префикса сохраняется в исходном виде (граничный символ возвращается
    /// на место через $1).
    #[test]
    fn mask_prefix_matches_both_slash_directions_and_case() {
        let root = Path::new(r"C:\Data\mc-launcher-v2");
        let text = "game_dir: c:/data/MC-LAUNCHER-v2/cache/x.log, и c:\\data\\mc-launcher-v2\\instances";
        let masked = crate::util::redact::mask_path_prefix(text, root, "[DATA]");
        assert_eq!(
            masked,
            "game_dir: [DATA]/cache/x.log, и [DATA]\\instances",
            "регистр и слэши не мешают, граница сохранена"
        );
    }

    /// Кириллица в пути (реальный кейс: имя пользователя Windows) маскируется.
    #[test]
    fn mask_prefix_handles_cyrillic_path() {
        let home = Path::new(r"C:\Users\ИгнатийПетров");
        let text = "java: C:\\Users\\ИгнатийПетров\\.jdks\\temurin-21\\bin\\java.exe";
        let masked = crate::util::redact::mask_path_prefix(text, home, r"C:\Users\[USER]");
        assert_eq!(
            masked,
            r"java: C:\Users\[USER]\.jdks\temurin-21\bin\java.exe"
        );
    }

    /// Граница совпадения: соседний каталог с общим префиксом имени не
    /// задевается; вхождение в начале строки и перед пробелом — да.
    #[test]
    fn mask_prefix_respects_path_boundary() {
        let home = Path::new(r"C:\Users\Vasya");
        let masked = crate::util::redact::mask_path_prefix(
            r"C:\Users\Vasya2\чужой C:\Users\Vasya\log.txt",
            home,
            "[USER]",
        );
        assert!(
            masked.contains(r"C:\Users\Vasya2\чужой"),
            "чужой каталог не задет: {masked}"
        );
        assert!(masked.contains(r"[USER]\log.txt"), "{masked}");
    }

    /// `\\?\`-префикс длинного пути не мешает совпадению (совпадение ищется
    /// в любом месте строки, префикс остаётся перед маской).
    #[test]
    fn mask_prefix_inside_long_path_form() {
        let root = Path::new(r"C:\Data\mc-launcher-v2");
        let masked = crate::util::redact::mask_path_prefix(
            r"error: \\?\C:\Data\mc-launcher-v2\cache\big.bin too large",
            root,
            "[DATA]",
        );
        assert_eq!(
            masked,
            r"error: \\?\[DATA]\cache\big.bin too large"
        );
    }
}
