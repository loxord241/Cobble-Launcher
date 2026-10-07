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
    /// D18: группа задач (`instance:{id}` / `mrpack:{...}`) — UI привязывает
    /// прогресс к конкретному инстансу, а не к первому попавшемуся.
    pub group: String,
    /// D18: инстанс-владелец, извлечённый из группы (`instance:{id}` /
    /// `mrpack:{id}`); None в JSON не попадает — фронты, не читающие поле,
    /// не ломаются.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
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
    /// Группа задач дошла до конца (успех/ошибка/отмена) — D62: фронт раньше
    /// узнавал об этом по тишине (watchdog 90 с чистил «висящий» статус
    /// занятости инстанса). `failed` = в группе есть проваленные задачи.
    DlGroupDone {
        group: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        instance_id: Option<String>,
        failed: bool,
    },
    LaunchState {
        instance_id: String,
        phase: LaunchPhase,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        /// Unix-ms старта ЭТОГО запуска (только у событий живого супервизора):
        /// фронт отличает устаревший exited старого процесса от свежего
        /// preparing после быстрого перезапуска (D64).
        #[serde(skip_serializing_if = "Option::is_none")]
        launch_started_at: Option<u64>,
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
        // 8192, а не 1024: подробный лог игры идёт построчно через GameLogLine,
        // и при бурном выводе (краш-стеки, модпаки с дебаг-логами) канал
        // 1024 переполнялся — broadcast вытеснял непрочитанные строки и в UI
        // лог терялся. Слоты стоят копейки (указатель, не сообщение).
        Self::new(8192)
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
            launch_started_at: Some(1_700_000_000_000),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"event\":\"launch_state\""));
        assert!(json.contains("\"instanceId\":\"inst-1\""));
        assert!(json.contains("\"phase\":\"running\""));
        assert!(json.contains("\"exitCode\":0"));
        assert!(json.contains("\"launchStartedAt\":1700000000000"));

        let dl = LauncherEvent::DlProgress(DlProgress {
            group: "instance:inst-1".into(),
            instance_id: Some("inst-1".into()),
            done_files: 5,
            total_files: 10,
            done_bytes: 1024,
            total_bytes: 2048,
            bytes_per_sec: 500.0,
            eta_secs: Some(2),
        });
        let dl_json = serde_json::to_string(&dl).unwrap();
        assert!(dl_json.contains("\"event\":\"dl_progress\""));
        assert!(dl_json.contains("\"group\":\"instance:inst-1\""));
        assert!(dl_json.contains("\"doneFiles\":5"));
        assert!(dl_json.contains("\"totalFiles\":10"));
        assert!(dl_json.contains("\"doneBytes\":1024"));
        assert!(dl_json.contains("\"totalBytes\":2048"));
        assert!(dl_json.contains("\"bytesPerSec\":500.0"));
        assert!(dl_json.contains("\"etaSecs\":2"));
        assert!(dl_json.contains("\"instanceId\":\"inst-1\""));
    }

    /// D18: без владельца (чужая группа) поле `instanceId` в JSON не пишется —
    /// старые фронты, не знающие о нём, не ломаются.
    #[test]
    fn dl_progress_without_instance_id_omits_field() {
        let dl = LauncherEvent::DlProgress(DlProgress {
            group: "jre:1".into(),
            instance_id: None,
            done_files: 1,
            total_files: 2,
            done_bytes: 3,
            total_bytes: 4,
            bytes_per_sec: 0.0,
            eta_secs: None,
        });
        let json = serde_json::to_string(&dl).unwrap();
        assert!(json.contains("\"event\":\"dl_progress\""));
        assert!(!json.contains("instanceId"));
    }

    /// D62: контракт события завершения группы — фронт снимает виртуальный
    /// статус занятости по нему, а не watchdog-тишиной.
    #[test]
    fn dl_group_done_serializes() {
        let ev = LauncherEvent::DlGroupDone {
            group: "instance:abc:content".into(),
            instance_id: Some("abc".into()),
            failed: false,
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"event\":\"dl_group_done\""));
        assert!(json.contains("\"group\":\"instance:abc:content\""));
        assert!(json.contains("\"instanceId\":\"abc\""));
        assert!(json.contains("\"failed\":false"));

        let ev2 = LauncherEvent::DlGroupDone {
            group: "jre:2".into(),
            instance_id: None,
            failed: true,
        };
        let json2 = serde_json::to_string(&ev2).unwrap();
        assert!(json2.contains("\"event\":\"dl_group_done\""));
        assert!(!json2.contains("instanceId"));
    }
}
