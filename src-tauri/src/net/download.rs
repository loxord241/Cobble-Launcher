//! Движок загрузок (спека §4.4): очередь с приоритетами, параллелизм,
//! retry/backoff, хэши, атомарные записи, отмена по группе/глобально,
//! персистентность очереди, троттлинг прогресса ≤ 10 соб./с.

use crate::errors::{LauncherError, Result};
use crate::events::{DlProgress, DlQueueState, EventBus, LauncherEvent};
use crate::net::http::{backoff_delay, retryable_wait, HttpClient};
use crate::util::fs::{long_path, part_path, sha1_file};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// Задача загрузки. Приоритет: больше = раньше; ассеты — с наименьшим (спека §4.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub dest: PathBuf,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    pub group: String,
    #[serde(default)]
    pub priority: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Downloading,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEntry {
    pub task: DownloadTask,
    pub state: TaskState,
    pub attempts: u32,
    #[serde(default)]
    pub error: Option<String>,
}

/// 1 попытка + 3 retry (спека §4.4).
pub const MAX_ATTEMPTS: u32 = 4;
const PROGRESS_TICK: Duration = Duration::from_millis(100);

struct Runtime {
    tasks: Mutex<Vec<TaskEntry>>,
    global_cancel: AtomicBool,
    group_cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    paused: AtomicBool,
    done_files: AtomicU64,
    failed_files: AtomicU64,
    /// Байты, реально полученные по сети (учитываются и повторные загрузки).
    received_bytes: AtomicU64,
}

pub struct DownloadEngine {
    client: Arc<HttpClient>,
    parallelism: usize,
    bus: EventBus,
    rt: Runtime,
}

impl DownloadEngine {
    pub fn new(client: Arc<HttpClient>, parallelism: usize, bus: EventBus) -> Arc<Self> {
        Arc::new(Self {
            client,
            parallelism: parallelism.clamp(1, 64),
            bus,
            rt: Runtime {
                tasks: Mutex::new(Vec::new()),
                global_cancel: AtomicBool::new(false),
                group_cancels: Mutex::new(HashMap::new()),
                paused: AtomicBool::new(false),
                done_files: AtomicU64::new(0),
                failed_files: AtomicU64::new(0),
                received_bytes: AtomicU64::new(0),
            },
        })
    }

    /// Добавить задачи. Дедуп: тот же dest+sha или тот же URL — пропуск
    /// (спека §4.4: одинаковые URL не качаются дважды).
    pub fn add_tasks(&self, tasks: Vec<DownloadTask>) -> usize {
        let mut guard = self.rt.tasks.lock().unwrap();
        let mut added = 0;
        for t in tasks {
            let dup = guard.iter().any(|e| {
                (e.task.dest == t.dest && e.task.sha1 == t.sha1) || e.task.url == t.url
            });
            if dup {
                tracing::debug!("дедуп задачи: {}", t.url);
                continue;
            }
            guard.push(TaskEntry {
                task: t,
                state: TaskState::Pending,
                attempts: 0,
                error: None,
            });
            added += 1;
        }
        added
    }

    /// Выполнить очередь до завершения (или паузы/отмены).
    /// Упавшие задачи остаются Failed — см. `queue_state` / `retry_failed`.
    pub async fn run(self: &Arc<Self>) -> Result<()> {
        self.rt.global_cancel.store(false, Ordering::SeqCst);
        self.rt.paused.store(false, Ordering::SeqCst);

        let ticker = {
            let this = Arc::clone(self);
            tokio::spawn(async move { this.progress_ticker().await })
        };

        let sem = Arc::new(Semaphore::new(self.parallelism));
        let mut workers = Vec::with_capacity(self.parallelism);
        for _ in 0..self.parallelism {
            let this = Arc::clone(self);
            let sem = Arc::clone(&sem);
            workers.push(tokio::spawn(async move { this.worker(sem).await }));
        }
        for w in workers {
            let _ = w.await;
        }
        ticker.abort();

        let st = self.queue_state();
        if st.failed > 0 {
            tracing::warn!("загрузка завершена с ошибками: {} задач", st.failed);
        }
        Ok(())
    }

    async fn worker(self: Arc<Self>, sem: Arc<Semaphore>) {
        loop {
            if self.rt.paused.load(Ordering::SeqCst) || self.rt.global_cancel.load(Ordering::SeqCst)
            {
                return;
            }
            let Some(entry) = self.take_next_pending() else {
                return;
            };
            let permit = match sem.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => return,
            };
            let group_flag = self.group_flag(&entry.task.group);
            let result = self.download_one(&entry.task, &group_flag).await;
            drop(permit);

            let mut guard = self.rt.tasks.lock().unwrap();
            if let Some(e) = guard.iter_mut().find(|e| e.task.id == entry.task.id) {
                match &result {
                    Ok(()) => {
                        e.state = TaskState::Done;
                        e.error = None;
                        self.rt.done_files.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(LauncherError::Cancelled) => {
                        // Пауза → снова Pending (переживёт рестарт); отмена → Cancelled.
                        if self.rt.paused.load(Ordering::SeqCst) {
                            e.state = TaskState::Pending;
                        } else {
                            e.state = TaskState::Cancelled;
                        }
                        e.error = Some("отменено".into());
                    }
                    Err(err) => {
                        e.state = TaskState::Failed;
                        e.attempts = MAX_ATTEMPTS;
                        e.error = Some(err.to_string());
                        self.rt.failed_files.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        }
    }

    /// Взять следующую Pending-задачу с наивысшим приоритетом.
    fn take_next_pending(&self) -> Option<TaskEntry> {
        let mut guard = self.rt.tasks.lock().unwrap();
        let idx = guard
            .iter()
            .enumerate()
            .filter(|(_, e)| e.state == TaskState::Pending)
            .max_by_key(|(_, e)| e.task.priority)
            .map(|(i, _)| i)?;
        guard[idx].state = TaskState::Downloading;
        Some(guard[idx].clone())
    }

    fn group_flag(&self, group: &str) -> Arc<AtomicBool> {
        let mut map = self.rt.group_cancels.lock().unwrap();
        map.entry(group.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    /// Скачать один файл: `.part` → хэш → атомарный rename.
    /// Транспортные повторы (сеть/429/битый хэш) — до MAX_ATTEMPTS внутри.
    async fn download_one(&self, task: &DownloadTask, cancel: &AtomicBool) -> Result<()> {
        let dest_l = long_path(&task.dest);
        if task.dest.exists() {
            match &task.sha1 {
                Some(sha) => match sha1_file(&task.dest) {
                    Ok(actual) if &actual == sha => return Ok(()),
                    _ => crate::util::fs::remove_file_ignore(&task.dest),
                },
                None => return Ok(()),
            }
        }
        if let Some(parent) = task.dest.parent() {
            std::fs::create_dir_all(long_path(parent))?;
        }
        let part = long_path(&part_path(&task.dest));
        crate::util::fs::remove_file_ignore(&part);

        for attempt in 1..=MAX_ATTEMPTS {
            if cancel.load(Ordering::SeqCst) {
                return Err(LauncherError::Cancelled);
            }
            let resp = self.client.raw().get(&task.url).send().await;
            match resp {
                Ok(r) if r.status().is_success() => {
                    match self.stream_to_file(r, &part, cancel).await {
                        Ok(_) => {
                            // Хэш-проверка обязательна для каждого байта (спека §3).
                            if let Some(sha) = &task.sha1 {
                                let actual = sha1_file(&part)?;
                                if &actual != sha {
                                    crate::util::fs::remove_file_ignore(&part);
                                    if attempt == MAX_ATTEMPTS {
                                        return Err(LauncherError::HashMismatch {
                                            path: task.dest.to_string_lossy().into(),
                                            expected: sha.clone(),
                                            actual,
                                        });
                                    }
                                    tokio::time::sleep(backoff_delay(attempt)).await;
                                    continue;
                                }
                            }
                            std::fs::rename(&part, &dest_l)?;
                            return Ok(());
                        }
                        Err(LauncherError::Cancelled) => {
                            crate::util::fs::remove_file_ignore(&part);
                            return Err(LauncherError::Cancelled);
                        }
                        Err(e) => {
                            crate::util::fs::remove_file_ignore(&part);
                            if attempt == MAX_ATTEMPTS {
                                return Err(e);
                            }
                            tokio::time::sleep(backoff_delay(attempt)).await;
                        }
                    }
                }
                Ok(r) if r.status().as_u16() == 429 || r.status().is_server_error() => {
                    let status = r.status().as_u16();
                    let retry_after = r.headers().get(reqwest::header::RETRY_AFTER).cloned();
                    if attempt == MAX_ATTEMPTS {
                        return Err(LauncherError::network(format!(
                            "HTTP {status} для {}: исчерпаны попытки",
                            task.url
                        )));
                    }
                    let wait = retryable_wait(status, retry_after.as_ref(), attempt);
                    tokio::time::sleep(wait).await;
                }
                Ok(r) => {
                    return Err(LauncherError::network(format!(
                        "HTTP {} для {}",
                        r.status(),
                        task.url
                    )));
                }
                Err(e) => {
                    if attempt == MAX_ATTEMPTS {
                        return Err(e.into());
                    }
                    tokio::time::sleep(backoff_delay(attempt)).await;
                }
            }
        }
        unreachable!("цикл попыток всегда завершается return");
    }

    /// Потоково писать ответ в файл; возвращает число записанных байтов.
    async fn stream_to_file(
        &self,
        resp: reqwest::Response,
        part: &Path,
        cancel: &AtomicBool,
    ) -> Result<u64> {
        let mut file = std::fs::File::create(part)?;
        let mut stream = resp.bytes_stream();
        let mut written: u64 = 0;
        while let Some(chunk) = stream.next().await {
            if cancel.load(Ordering::SeqCst) {
                return Err(LauncherError::Cancelled);
            }
            let chunk = chunk?;
            file.write_all(&chunk)?;
            written += chunk.len() as u64;
        }
        file.sync_all()?;
        self.rt.received_bytes.fetch_add(written, Ordering::Relaxed);
        Ok(written)
    }

    /// Троттлинг прогресса: не чаще 10 событий/с (спека §4.4).
    async fn progress_ticker(self: Arc<Self>) {
        let mut last = Instant::now();
        let mut last_bytes = 0u64;
        loop {
            tokio::time::sleep(PROGRESS_TICK).await;
            let now_bytes = self.rt.received_bytes.load(Ordering::Relaxed);
            let elapsed = last.elapsed().as_secs_f64().max(0.001);
            let speed = (now_bytes.saturating_sub(last_bytes)) as f64 / elapsed;
            last = Instant::now();
            last_bytes = now_bytes;

            let (pending, downloading, done, failed, cancelled, total_bytes) = {
                let guard = self.rt.tasks.lock().unwrap();
                let mut total_bytes = 0u64;
                let mut c = [0u64; 5];
                for e in guard.iter() {
                    match e.state {
                        TaskState::Pending => c[0] += 1,
                        TaskState::Downloading => c[1] += 1,
                        TaskState::Done => c[2] += 1,
                        TaskState::Failed => c[3] += 1,
                        TaskState::Cancelled => c[4] += 1,
                    }
                    total_bytes += e.task.size.unwrap_or(0);
                }
                (c[0], c[1], c[2], c[3], c[4], total_bytes)
            };
            let done_bytes = self.rt.received_bytes.load(Ordering::Relaxed);
            let eta = if speed > 1.0 && total_bytes > done_bytes {
                Some(((total_bytes - done_bytes) as f64 / speed) as u64)
            } else {
                None
            };
            self.bus.emit(LauncherEvent::DlProgress(DlProgress {
                done_files: self.rt.done_files.load(Ordering::Relaxed),
                total_files: pending + downloading + done + failed + cancelled,
                done_bytes,
                total_bytes,
                bytes_per_sec: speed,
                eta_secs: eta,
            }));
        }
    }

    /// Сводка очереди для UI (в т.ч. список упавших с причинами).
    pub fn queue_state(&self) -> DlQueueState {
        let guard = self.rt.tasks.lock().unwrap();
        let mut st = DlQueueState {
            pending: 0,
            downloading: 0,
            done: 0,
            failed: 0,
            cancelled: 0,
            failed_items: Vec::new(),
        };
        for e in guard.iter() {
            match e.state {
                TaskState::Pending => st.pending += 1,
                TaskState::Downloading => st.downloading += 1,
                TaskState::Done => st.done += 1,
                TaskState::Failed => {
                    st.failed += 1;
                    st.failed_items.push((
                        e.task.url.clone(),
                        e.error.clone().unwrap_or_default(),
                    ));
                }
                TaskState::Cancelled => st.cancelled += 1,
            }
        }
        st
    }

    /// Пауза: остановить скачивание, недокачанное остаётся Pending.
    pub fn pause(&self) {
        self.rt.paused.store(true, Ordering::SeqCst);
        for flag in self.rt.group_cancels.lock().unwrap().values() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// Отмена группы: мгновенно, `.part` чистятся (в download_one), задачи → Cancelled.
    pub fn cancel_group(&self, group: &str) {
        if let Some(flag) = self.rt.group_cancels.lock().unwrap().get(group) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    pub fn cancel_all(&self) {
        self.rt.global_cancel.store(true, Ordering::SeqCst);
        for flag in self.rt.group_cancels.lock().unwrap().values() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// Повторить упавшие: Failed → Pending, попытки сброшены (кнопка «повторить»).
    pub fn retry_failed(&self) -> usize {
        let mut guard = self.rt.tasks.lock().unwrap();
        let mut n = 0;
        for e in guard.iter_mut() {
            if e.state == TaskState::Failed {
                e.state = TaskState::Pending;
                e.attempts = 0;
                e.error = None;
                n += 1;
            }
        }
        n
    }

    /// Сохранить недокачанную очередь атомарно в `store/queue.json` (спека §4.4).
    pub fn persist_queue(&self, path: &Path) -> Result<()> {
        let guard = self.rt.tasks.lock().unwrap();
        let unfinished: Vec<TaskEntry> = guard
            .iter()
            .filter(|e| !matches!(e.state, TaskState::Done))
            .cloned()
            .collect();
        drop(guard);
        let data = serde_json::to_vec_pretty(&unfinished)?;
        crate::util::fs::atomic_write(path, &data)
    }

    /// Загрузить очередь после рестарта: валидные на диске файлы → Done.
    pub fn load_queue(&self, path: &Path) -> Result<usize> {
        let long = long_path(path);
        if !long.exists() {
            return Ok(0);
        }
        let data = std::fs::read(&long)?;
        let entries: Vec<TaskEntry> = serde_json::from_slice(&data)?;
        let mut guard = self.rt.tasks.lock().unwrap();
        let mut added = 0;
        for mut e in entries {
            if e.state == TaskState::Done {
                continue;
            }
            if e.task.dest.exists() {
                let valid = match &e.task.sha1 {
                    Some(sha) => sha1_file(&e.task.dest).map(|a| &a == sha).unwrap_or(false),
                    None => true,
                };
                e.state = if valid {
                    TaskState::Done
                } else {
                    TaskState::Pending
                };
            } else {
                e.state = TaskState::Pending;
            }
            e.attempts = 0;
            if !guard.iter().any(|x| x.task.dest == e.task.dest) {
                guard.push(e);
                added += 1;
            }
        }
        Ok(added)
    }
}
