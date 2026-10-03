//! Единый тип ошибки ядра. Сериализуется в UI как `{code, message, hint?}`
//! (спека §4.1, §4.3). Никаких unwrap/expect в продуктовых путях.

use serde::Serialize;

/// Плоский payload ошибки для UI/IPC.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LauncherError {
    #[error("сеть: {0}")]
    Network(String),

    #[error("хэш не совпал для {path}: ожидался {expected}, получен {actual}")]
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("не найдено: {0}")]
    NotFound(String),

    #[error("некорректный ввод: {0}")]
    InvalidInput(String),

    #[error("путь вне целевого каталога (zip-slip): {0}")]
    ZipSlip(String),

    #[error("Java не найдена: {0}")]
    JavaNotFound(String),

    #[error("версия не найдена: {0}")]
    VersionNotFound(String),

    #[error("операция отменена")]
    Cancelled,

    #[error("таймаут: {0}")]
    Timeout(String),

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP: {0}")]
    Http(#[from] reqwest::Error),

    #[error("zip: {0}")]
    Zip(String),

    #[error("инстанс запущен: {0}")]
    InstanceRunning(String),

    #[error("режим „работать офлайн“: сеть отключена ({0})")]
    OfflineMode(String),

    #[error("внутренняя ошибка: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, LauncherError>;

/// ENV-15: подсказка для сертификатных ошибок TLS (сбитая дата/время,
/// captive-портал): без неё они выглядят как безликая «ошибка сети».
const CERT_HINT: &str =
    "Не удалось проверить сервер. Если на компьютере сбита дата и время — исправьте их и повторите.";

/// ENV-15: ищет «certificate» по всей цепочке source(). Display у reqwest
/// показывает только верхний уровень, а слово про сертификат лежит глубже
/// (rustls/schannel), поэтому текст проверяем по цепочке источников.
fn mentions_certificate(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(err);
    while let Some(e) = current {
        if e.to_string().to_lowercase().contains("certificate") {
            return true;
        }
        current = e.source();
    }
    false
}

/// Сериализуется как payload — так ошибки автоматически проходят через
/// #[tauri::command] в UI (спека §4.1: `{code, message, hint}`).
impl serde::Serialize for LauncherError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.payload().serialize(serializer)
    }
}

impl LauncherError {
    /// Код ошибки для i18n-маппинга в UI (спека §8: `i18n/errors.json`).
    pub fn code(&self) -> &'static str {
        match self {
            LauncherError::Network(_) => "network",
            LauncherError::HashMismatch { .. } => "hash_mismatch",
            LauncherError::NotFound(_) => "not_found",
            LauncherError::InvalidInput(_) => "invalid_input",
            LauncherError::ZipSlip(_) => "zip_slip",
            LauncherError::JavaNotFound(_) => "java_not_found",
            LauncherError::VersionNotFound(_) => "version_not_found",
            LauncherError::Cancelled => "cancelled",
            LauncherError::Timeout(_) => "timeout",
            LauncherError::Io(_) => "io",
            LauncherError::Json(_) => "json",
            LauncherError::Http(_) => "http",
            LauncherError::Zip(_) => "zip",
            LauncherError::InstanceRunning(_) => "instance_running",
            LauncherError::OfflineMode(_) => "offline_mode",
            LauncherError::Internal(_) => "internal",
        }
    }

    /// Подсказка человеку (показывается в UI рядом с сообщением).
    pub fn hint(&self) -> Option<String> {
        match self {
            LauncherError::HashMismatch { .. } => Some(
                "Файл скачан повторно и хэш снова не совпал — проверьте соединение или зеркало."
                    .into(),
            ),
            LauncherError::JavaNotFound(_) => Some(
                "Установите Java автоматически в настройках или укажите путь вручную.".into(),
            ),
            // Network без хинта: UI локализует «Проверьте подключение…» сам
            // (errors.json → errorText), раньше совет дублировался дважды.
            LauncherError::InstanceRunning(_) => Some(
                "Остановите игру этого инстанса — Windows не даёт менять занятые файлы.".into(),
            ),
            LauncherError::OfflineMode(_) => Some(
                "Выключите тумблер «Работать офлайн» в титлбаре, чтобы качать из сети.".into(),
            ),
            // ENV-15: исключение из «сеть без хинта» — сертификатная ошибка
            // (сбитое время, captive-портал) без адресного совета неразличима
            // с обычным обрывом сети.
            LauncherError::Http(e) => {
                if mentions_certificate(e) {
                    Some(CERT_HINT.into())
                } else {
                    None
                }
            }
            LauncherError::Network(msg) => {
                if msg.to_lowercase().contains("certificate") {
                    Some(CERT_HINT.into())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn payload(&self) -> ErrorPayload {
        // Network: Display добавляет префикс «сеть: », а UI кладёт свою
        // локализованную обёртку («Проблема с сетью…») — в payload идёт
        // голая причина, иначе в сообщении тройное дублирование.
        let message = match self {
            LauncherError::Network(msg) => msg.clone(),
            _ => self.to_string(),
        };
        ErrorPayload {
            code: self.code().into(),
            message,
            hint: self.hint(),
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        LauncherError::Internal(msg.into())
    }

    pub fn network(msg: impl Into<String>) -> Self {
        LauncherError::Network(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_has_code_message_hint() {
        let e = LauncherError::HashMismatch {
            path: "x.jar".into(),
            expected: "aaa".into(),
            actual: "bbb".into(),
        };
        let p = serde_json::to_value(e.payload()).unwrap();
        assert_eq!(p["code"], "hash_mismatch");
        assert!(p["message"].as_str().unwrap().contains("x.jar"));
        assert!(p["hint"].is_string());
    }

    /// Network: в payload голая причина без префикса «сеть: » (его добавляет
    /// Display для логов) и без хинта — UI локализует обёртку сам.
    #[test]
    fn network_payload_is_raw_reason() {
        let e = LauncherError::network("login_with_xbox: HTTP 403");
        let p = serde_json::to_value(e.payload()).unwrap();
        assert_eq!(p["code"], "network");
        assert_eq!(p["message"], "login_with_xbox: HTTP 403");
        assert!(p["hint"].is_null(), "хинт сети теперь на стороне i18n");
        // Display для логов сохраняет префикс.
        assert!(e.to_string().starts_with("сеть: "));
    }

    /// ENV-15: подменная цепочка ошибок — верхний уровень без слова
    /// «certificate», источник с ним (как rustls под hyper под reqwest:
    /// Display reqwest цепочку не разворачивает).
    #[derive(Debug)]
    struct CertSource;
    impl std::fmt::Display for CertSource {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "invalid peer certificate: UnknownIssuer")
        }
    }
    impl std::error::Error for CertSource {}

    #[derive(Debug)]
    struct PlainSource;
    impl std::fmt::Display for PlainSource {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "connection refused")
        }
    }
    impl std::error::Error for PlainSource {}

    #[derive(Debug)]
    struct TopLevel(Box<dyn std::error::Error + 'static>);
    impl std::fmt::Display for TopLevel {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "error sending request for url (https://example.com)")
        }
    }
    impl std::error::Error for TopLevel {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(self.0.as_ref())
        }
    }

    /// ENV-15: «certificate» находится в source-цепочке, чистая цепочка — нет.
    #[test]
    fn certificate_found_in_source_chain() {
        assert!(mentions_certificate(&TopLevel(Box::new(CertSource))));
        assert!(!mentions_certificate(&TopLevel(Box::new(PlainSource))));
    }

    /// ENV-15: Network с сертификатной причиной получает хинт про дату/время,
    /// остальные сетевые тексты — как раньше, без хинта (его даёт i18n).
    #[test]
    fn network_certificate_error_gets_time_hint() {
        let e = LauncherError::network("tls handshake: certificate verify failed");
        let p = serde_json::to_value(e.payload()).unwrap();
        assert_eq!(p["code"], "network");
        assert!(p["hint"].as_str().unwrap().contains("дата и время"));

        let plain = LauncherError::network("connection refused");
        assert!(plain.payload().hint.is_none());
    }
}
