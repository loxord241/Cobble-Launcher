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
            LauncherError::Network(_) => Some(
                "Проверьте подключение к интернету; при необходимости настройте прокси в настройках."
                    .into(),
            ),
            LauncherError::InstanceRunning(_) => Some(
                "Остановите игру этого инстанса — Windows не даёт менять занятые файлы.".into(),
            ),
            LauncherError::OfflineMode(_) => Some(
                "Выключите тумблер «Работать офлайн» в титлбаре, чтобы качать из сети.".into(),
            ),
            _ => None,
        }
    }

    pub fn payload(&self) -> ErrorPayload {
        ErrorPayload {
            code: self.code().into(),
            message: self.to_string(),
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
}
