//! Движок загрузок (спека §4.4): очередь с приоритетами, параллелизм,
//! retry/backoff, хэши, атомарные записи, отмена по группе/глобально,
//! персистентность очереди, троттлинг прогресса ≤ 10 соб./с,
//! общий лимит скорости (F17).

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

/// F17: окно bucket'а — скользящая секунда общего счётчика байтов.
const THROTTLE_WINDOW: Duration = Duration::from_secs(1);
/// F17: минимальный семпл троттлинга — решение принимаем по кускам ≥ 64 КБ
/// (корректность важнее точности, мьютекс не гоняется на каждый мелкий чанк).
const THROTTLE_SAMPLE: usize = 64 * 1024;
/// Границы сна одной итерации ожидания бюджета: без горячего спина,
/// пауза/отмена подхватываются в пределах десятков миллисекунд.
const THROTTLE_SLEEP_MIN: Duration = Duration::from_millis(10);
const THROTTLE_SLEEP_MAX: Duration = Duration::from_millis(50);

/// F17: сколько ждать перед записью чанка (семпла) при лимите `limit_kbps`.
/// ZERO — лимит выключен или окно позволяет; >0 — бюджет переполнен, оценка —
/// время пополнения «лишних» байтов по скорости лимита. Движок спит эту
/// оценку итерациями по 10–50 мс с перепроверкой окна, поэтому корректен,
/// даже если оценка длиннее остатка окна.
fn throttle_delay(bytes_this_second: u64, limit_kbps: u32, chunk: usize) -> Duration {
    if limit_kbps == 0 {
        return Duration::ZERO;
    }
    let limit = u64::from(limit_kbps).saturating_mul(1024);
    let total = bytes_this_second.saturating_add(chunk as u64);
    if total <= limit {
        return Duration::ZERO;
    }
    let excess = total - limit;
    Duration::from_secs_f64(excess as f64 / limit as f64)
}

struct Runtime {
    tasks: Mutex<Vec<TaskEntry>>,
    global_cancel: AtomicBool,
    group_cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    paused: AtomicBool,
    done_files: AtomicU64,
    failed_files: AtomicU64,
    /// Байты, реально полученные по сети (учитываются и повторные загрузки).
    received_bytes: AtomicU64,
    /// F17: окно общего bucket'а — (начало окна, байты за окно). Одно на
    /// движок: лимит общий на все worker'ы, а не на файл/соединение.
    throttle_window: Mutex<Option<(Instant, u64)>>,
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
                throttle_window: Mutex::new(None),
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
        if task.dest.exists() {
            match &task.sha1 {
                Some(sha) => match sha1_file(&task.dest) {
                    Ok(actual) if &actual == sha => return Ok(()),
                    // A26: битый файл может быть придержан Defender/OneDrive —
                    // удаление с повторами вместо мгновенного отказа.
                    _ => {
                        let _ = crate::util::fs::remove_file_with_retry(&task.dest);
                    }
                },
                None => return Ok(()),
            }
        }
        if let Some(parent) = task.dest.parent() {
            std::fs::create_dir_all(long_path(parent)).map_err(|e| {
                LauncherError::Internal(format!(
                    "create_dir_all для {}: {e}",
                    task.dest.display()
                ))
            })?;
        }
        let part = long_path(&part_path(&task.dest));
        let _ = crate::util::fs::remove_file_with_retry(&part);

        for attempt in 1..=MAX_ATTEMPTS {
            if cancel.load(Ordering::SeqCst) {
                return Err(LauncherError::Cancelled);
            }
            // F16: офлайн — мгновенный фатальный отказ (как Cancelled: без
            // ретраев и backoff). Уже скачанные файлы выше отдались из кэша.
            if self.client.offline() {
                return Err(LauncherError::OfflineMode(task.url.clone()));
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
                                    let _ = crate::util::fs::remove_file_with_retry(&part);
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
                            // A26: финальное переименование `.part` → цель
                            // ретраится — антивирус/OneDrive держат файл миг.
                            crate::util::fs::rename_with_retry(&part, &task.dest)?;
                            return Ok(());
                        }
                        Err(LauncherError::Cancelled) => {
                            let _ = crate::util::fs::remove_file_with_retry(&part);
                            return Err(LauncherError::Cancelled);
                        }
                        Err(e) => {
                            let _ = crate::util::fs::remove_file_with_retry(&part);
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
        // F17: накопитель семпла — лимит общий на движок, поэтому перед записью
        // ждём разрешения в общем окне (см. throttle_wait).
        let mut sample: usize = 0;
        while let Some(chunk) = stream.next().await {
            if cancel.load(Ordering::SeqCst) {
                return Err(LauncherError::Cancelled);
            }
            let chunk = chunk?;
            sample = sample.saturating_add(chunk.len());
            if sample >= THROTTLE_SAMPLE {
                self.throttle_wait(sample).await;
                sample = 0;
            }
            file.write_all(&chunk)?;
            written += chunk.len() as u64;
        }
        // Хвост меньше семпла тоже учитываем, иначе недосчёт ускорит следующий файл.
        if sample > 0 {
            self.throttle_wait(sample).await;
        }
        file.sync_all()?;
        self.rt.received_bytes.fetch_add(written, Ordering::Relaxed);
        Ok(written)
    }

    /// F17: дождаться бюджета семпла в общем окне. Лимит берём с клиента
    /// (0 — без ограничения); смена лимита на лету подхватывается здесь же.
    async fn throttle_wait(&self, sample: usize) {
        let limit = self.client.speed_limit();
        if limit == 0 {
            return;
        }
        loop {
            let delay = self.reserve_sample(limit, sample);
            if delay.is_zero() {
                return;
            }
            // Спим итерациями 10–50 мс: без горячего спина. Окно перекатывается
            // само, поэтому каждый заход резервирует заново — это корректнее,
            // чем один долгий сон по теоретической оценке.
            tokio::time::sleep(delay.clamp(THROTTLE_SLEEP_MIN, THROTTLE_SLEEP_MAX))
                .await;
        }
    }

    /// Попытка зачислить семпл из `sample` байтов в текущее окно:
    /// Duration::ZERO — можно (байты уже учтены), >0 — окно переполнено,
    /// ждать (счётчик не трогаем).
    fn reserve_sample(&self, limit_kbps: u32, sample: usize) -> Duration {
        let mut guard = self.rt.throttle_window.lock().unwrap();
        let (start, mut bytes) = match *guard {
            Some((start, bytes)) if start.elapsed() < THROTTLE_WINDOW => (start, bytes),
            // Окна нет или оно перекатилось — новое окно с нулевым счётчиком.
            _ => (Instant::now(), 0),
        };
        let delay = throttle_delay(bytes, limit_kbps, sample);
        if delay.is_zero() {
            bytes = bytes.saturating_add(sample as u64);
        }
        // При отказе якорь окна не двигается — иначе ожидание стало бы вечным.
        *guard = Some((start, bytes));
        delay
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

#[cfg(test)]
mod tests {
    use super::*;

    /// F16: офлайн-задача падает мгновенно, фатально — ни сети, ни ретраев.
    /// С ретраями было бы MAX_ATTEMPTS × backoff ≈ 7+ с; здесь — миллисекунды.
    #[tokio::test]
    async fn offline_engine_fails_fast_without_retries() {
        let client = Arc::new(HttpClient::new(None).unwrap());
        client.set_offline(true);
        let engine = DownloadEngine::new(client, 2, EventBus::default());

        let tmp = std::env::temp_dir().join("mcl-f16-offline-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let dest = tmp.join("file.bin");

        let added = engine.add_tasks(vec![DownloadTask {
            id: "t1".into(),
            // 127.0.0.1:1 — соединение отклонилось бы мгновенно, но мы
            // обязаны вернуть OfflineMode ещё до попытки соединения.
            url: "http://127.0.0.1:1/file.bin".into(),
            dest: dest.clone(),
            sha1: None,
            size: None,
            group: "g".into(),
            priority: 0,
        }]);
        assert_eq!(added, 1);

        let started = Instant::now();
        engine.run().await.unwrap();
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_secs(5), "офлайн-отказ занял {elapsed:?}");

        let st = engine.queue_state();
        assert_eq!(st.failed, 1);
        assert!(st
            .failed_items
            .iter()
            .any(|(url, reason)| url.ends_with("/file.bin") && reason.contains("офлайн")));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// F17: лимит выключен — троттлинга нет ни при каком накоплении.
    #[test]
    fn throttle_delay_zero_when_limit_off() {
        assert_eq!(throttle_delay(0, 0, THROTTLE_SAMPLE), Duration::ZERO);
        assert_eq!(throttle_delay(u64::MAX, 0, THROTTLE_SAMPLE), Duration::ZERO);
    }

    /// F17: окно не переполнено (в т.ч. ровно в пределе) — ждать не нужно.
    #[test]
    fn throttle_delay_zero_within_window() {
        let limit_kbps = 1024u32; // 1 МБ/с = 1 МиБ за окно
        assert_eq!(throttle_delay(0, limit_kbps, THROTTLE_SAMPLE), Duration::ZERO);
        let almost_full = u64::from(limit_kbps) * 1024 - THROTTLE_SAMPLE as u64;
        assert_eq!(throttle_delay(almost_full, limit_kbps, THROTTLE_SAMPLE), Duration::ZERO);
        // Семпл, укладывающийся ровно в остаток окна, — ещё не переполнение.
        let fits_exactly = u64::from(limit_kbps) * 1024 - THROTTLE_SAMPLE as u64;
        assert_eq!(throttle_delay(fits_exactly, limit_kbps, 0), Duration::ZERO);
        // Переполнение — строго total > limit (см. positive-тест ниже).
    }

    /// F17: переполнение — ждать >0, дольше при большем излишке.
    #[test]
    fn throttle_delay_positive_when_overfilled() {
        let limit_kbps = 1024u32;
        // 1 МиБ уже ушло + ещё 64 КБ → лишних 64 КБ → 62.5 мс пополнения.
        let d = throttle_delay(1024 * 1024, limit_kbps, THROTTLE_SAMPLE);
        assert!(d > Duration::ZERO);
        assert_eq!(d, Duration::from_secs_f64(64.0 * 1024.0 / (1024.0 * 1024.0)));
        // Излишек в 4 раза больше — ожидание тоже.
        let d2 = throttle_delay(4 * 1024 * 1024, limit_kbps, THROTTLE_SAMPLE);
        assert!(d2 > d);
    }

    /// F17: окно bucket'а одно на движок — второй worker видит байты первого,
    /// а при отказе счётчик не меняется (якорь окна не продлевается).
    #[test]
    fn throttle_window_shared_across_workers() {
        let client = Arc::new(HttpClient::new(None).unwrap());
        client.set_speed_limit(1024); // 1 МиБ за окно
        let engine = DownloadEngine::new(client, 2, EventBus::default());

        assert_eq!(engine.reserve_sample(1024, 512 * 1024), Duration::ZERO);
        // Второй «worker»: 512 КБ + 576 КБ = 1 МиБ + 64 КБ → переполнение.
        assert!(engine.reserve_sample(1024, 512 * 1024 + 64 * 1024) > Duration::ZERO);
        // Отказ не израсходовал бюджет и не сдвинул окно.
        let guard = engine.rt.throttle_window.lock().unwrap();
        let (_, bytes) = guard.expect("окно должно существовать после первого резерва");
        assert_eq!(bytes, 512 * 1024);
    }
}
