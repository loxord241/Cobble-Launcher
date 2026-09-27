//! Типизированные события ядро → UI (спека §4.3). В CLI — печать в stdout,
//! в Tauri (M4) — мост в window.emit.

use serde::Serialize;

/// Фаза запуска игры (спека §6.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchPhase {
    Preparing,
    Downloading,
    Launching,
    Running,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
}

/// Агрегированный прогресс загрузки (троттлинг ≤ 10 событий/с — спека §4.4).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DlProgress {
    pub done_files: u64,
    pub total_files: u64,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_sec: f64,
    pub eta_secs: Option<u64>,
}

/// Состояние очереди: сколько задач в каких статусах + упавшие с причинами.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DlQueueState {
    pub pending: u64,
    pub downloading: u64,
    pub done: u64,
    pub failed: u64,
    pub cancelled: u64,
    /// (url, причина) — для списка «повторить» в UI.
    pub failed_items: Vec<(String, String)>,
}

/// Все события ядра. `tag = "event"` — единая точка диспетчеризации в UI.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum LauncherEvent {
    DlProgress(DlProgress),
    DlQueueState(DlQueueState),
    LaunchState {
        instance_id: String,
        phase: LaunchPhase,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },
    GameLogLine {
        instance_id: String,
        line: String,
        stream: LogStream,
    },
    AccountRefreshFailed {
        account_id: String,
        reason: String,
    },
}

/// Шина событий: broadcast — подписчиков может быть несколько (UI + логгер).
#[derive(Clone)]
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<LauncherEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity);
        Self { tx }
    }

    pub fn emit(&self, event: LauncherEvent) {
        // Нет подписчиков — не ошибка (CLI без UI).
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<LauncherEvent> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialize_to_contract_camel_case() {
        let ev = LauncherEvent::LaunchState {
            instance_id: "inst-1".into(),
            phase: LaunchPhase::Running,
            exit_code: Some(0),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"event\":\"launch_state\""));
        assert!(json.contains("\"instanceId\":\"inst-1\""));
        assert!(json.contains("\"phase\":\"running\""));
        assert!(json.contains("\"exitCode\":0"));

        let dl = LauncherEvent::DlProgress(DlProgress {
            done_files: 5,
            total_files: 10,
            done_bytes: 1024,
            total_bytes: 2048,
            bytes_per_sec: 500.0,
            eta_secs: Some(2),
        });
        let dl_json = serde_json::to_string(&dl).unwrap();
        assert!(dl_json.contains("\"event\":\"dl_progress\""));
        assert!(dl_json.contains("\"doneFiles\":5"));
        assert!(dl_json.contains("\"totalFiles\":10"));
        assert!(dl_json.contains("\"doneBytes\":1024"));
        assert!(dl_json.contains("\"totalBytes\":2048"));
        assert!(dl_json.contains("\"bytesPerSec\":500.0"));
        assert!(dl_json.contains("\"etaSecs\":2"));
    }
}
