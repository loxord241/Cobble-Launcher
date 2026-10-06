//! Движок загрузок (спека §4.4): очередь с приоритетами, параллелизм,
//! retry/backoff, хэши, атомарные записи, отмена по группе/глобально,
//! троттлинг прогресса ≤ 10 соб./с, общий лимит скорости (F17).

use crate::errors::{LauncherError, Result};
use crate::events::{DlProgress, DlQueueState, EventBus, LauncherEvent};
use crate::net::http::{backoff_delay, retryable_wait, HttpClient};
use crate::util::fs::{long_path, part_path_unique, sha1_file, sha512_file};
use futures::StreamExt;
use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// Задача загрузки. Приоритет: больше = раньше; ассеты — с наименьшим (спека §4.4).
/// D26: персистентность очереди не существует — serde-деривы задачам не нужны.
#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    /// SHA512 из доверенного манифеста: сверяется, когда sha1 не задан
    /// (в index.json модпаков бывает только sha512).
    pub sha512: Option<String>,
    pub size: Option<u64>,
    pub group: String,
    pub priority: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Downloading,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub task: DownloadTask,
    pub state: TaskState,
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
/// P2-таймаут: тишина при чтении (TTFB или пауза между чанками) дольше двух
/// минут = «сервер не отвечает», ошибка ретраится штатно. Заменяет убранный
/// общий 600-с таймаут клиента: reqwest без него ждёт вечно, а паузы
/// троттлинга (F17) в этот таймаут не попадают — оборачивается только чтение.
const READ_IDLE: Duration = Duration::from_secs(120);

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

/// PERF#6: ключ дедупликации (dest + хэши) — эквивалент прежнего any().
fn file_key(t: &DownloadTask) -> String {
    format!("{:?}|{:?}|{:?}", t.dest, t.sha1, t.sha512)
}

/// D18: инстанс-владелец из группы задач: `instance:{id}` и `mrpack:{id}`
/// дают Some(id), прочие группы (`jre:1`, …) — None. У инстанс-групп бывают
/// суффиксы (`instance:{id}:content`/:update/:repair) — в id берётся только
/// сегмент до следующего ':'.
fn instance_id_from_group(group: &str) -> Option<String> {
    let rest = group
        .strip_prefix("instance:")
        .or_else(|| group.strip_prefix("mrpack:"))?;
    let id = rest.split(':').next().unwrap_or_default();
    (!id.is_empty()).then(|| id.to_string())
}

/// A51: маркер part-файла движка между именем цели и хэшем группы — формат
/// задаёт `util::fs::part_path_unique` (`<имя>.part.<12 hex>`), менять его
/// нельзя (ENV-2 завязан на уникальность по группе).
pub const PART_FILE_MARK: &str = ".part.";

/// A51: суффикс-маркер для уборщиков part-файлов (см. `part_files_in`).
pub fn part_file_suffix() -> &'static str {
    PART_FILE_MARK
}

/// A51: это имя — part-файл движка? После маркера ровно 12 hex-символов.
/// Обычный `.part` одиночных атомарных записей (`atomic_write`) не матчится —
/// уборщик его не трогает.
pub fn is_engine_part_name(file_name: &str) -> bool {
    match file_name.rsplit_once(PART_FILE_MARK) {
        Some((_, tail)) => tail.len() == 12 && tail.bytes().all(|b| b.is_ascii_hexdigit()),
        None => false,
    }
}

/// A51: все part-файлы движка в каталоге — хвосты после краха процесса
/// (нормальный путь: `.part` переименовывается или сносится в download_one).
pub fn part_files_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            if is_engine_part_name(name) {
                out.push(entry.path());
            }
        }
    }
    out
}

/// Результат потоковой записи: дайджесты, посчитанные на лету.
struct StreamedFile {
    sha1: String,
    sha512: String,
}

/// P3: классификация сбоя потоковой записи — локальный IO (диск полон, права,
/// файл держит Defender) не ретраится полной перекачкой, транспорт — ретраится.
#[derive(Debug)]
enum StreamFail {
    Cancelled,
    /// Чтение стрима / тишина сервера — транспортная, ретраится до MAX_ATTEMPTS.
    Network(LauncherError),
    /// Создание/запись локального файла — фатально, одна попытка.
    Local(LauncherError),
}

struct Runtime {
    tasks: Mutex<Vec<TaskEntry>>,
    /// PERF#6: уникальные группы — задачи из вектора не удаляются, поэтому
    /// множество растёт только в add_tasks; тикеру не нужно клонировать
    /// тысячи строк под локом каждые 100 мс.
    groups: Mutex<std::collections::HashSet<String>>,
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
                groups: Mutex::new(std::collections::HashSet::new()),
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

    /// Добавить задачи. Дедуп: тот же dest+sha или тот же URL+dest — пропуск
    /// (спека §4.4: одинаковые URL не качаются дважды). P3: в ключ URL-дедупа
    /// входит dest — тот же URL в другой dest это ДРУГАЯ задача и молча
    /// не пропадает.
    pub fn add_tasks(&self, tasks: Vec<DownloadTask>) -> usize {
        let mut guard = self.rt.tasks.lock().unwrap();
        // PERF#6: дедуп за O(1) на задачу — раньше any() по всему вектору
        // давал O(N^2) на сборках с тысячами ассетов.
        let mut seen_file: std::collections::HashSet<String> =
            guard.iter().map(|e| file_key(&e.task)).collect();
        let mut seen_url: std::collections::HashSet<(String, PathBuf)> = guard
            .iter()
            .map(|e| (e.task.url.clone(), e.task.dest.clone()))
            .collect();
        let mut added = 0;
        for t in tasks {
            let dup = seen_file.contains(&file_key(&t))
                || seen_url.contains(&(t.url.clone(), t.dest.clone()));
            if dup {
                tracing::debug!("дедуп задачи: {}", t.url);
                continue;
            }
            seen_file.insert(file_key(&t));
            seen_url.insert((t.url.clone(), t.dest.clone()));
            self.rt.groups.lock().unwrap().insert(t.group.clone());
            guard.push(TaskEntry {
                task: t,
                state: TaskState::Pending,
                error: None,
            });
            added += 1;
        }
        added
    }

    /// P3: полный сброс флагов прерывания в начале `run()` — новый прогон
    /// стартует с чистыми global_cancel/paused и всеми group-флагами (после
    /// отмены движок не «кирпич»).
    fn reset_run_flags(&self) {
        self.rt.global_cancel.store(false, Ordering::SeqCst);
        self.rt.paused.store(false, Ordering::SeqCst);
        let map = self.rt.group_cancels.lock().unwrap();
        for flag in map.values() {
            flag.store(false, Ordering::SeqCst);
        }
    }

    /// Выполнить очередь до завершения (или паузы/отмены).
    /// Упавшие задачи остаются Failed — см. `queue_state` / `retry_failed`.
    pub async fn run(self: &Arc<Self>) -> Result<()> {
        self.reset_run_flags();

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
            // Паника worker'а — не штатный выход, оставляем след в логе.
            if let Err(e) = w.await {
                tracing::warn!("worker очереди загрузок упал: {e}");
            }
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
            // Валидный кэш: совпал sha1, при его отсутствии — sha512; оба не
            // заданы — старое поведение, файл считается готовым.
            // P3: ошибка ЧТЕНИЯ кэша (Defender/OneDrive придержали файл) — это
            // НЕ «хэш не совпал»: dest не трогаем, наружу уходит обычная
            // ретраибельная IO-ошибка (retry_failed/следующий прогон повторят).
            let existing_ok = if let Some(sha) = &task.sha1 {
                sha1_file(&task.dest)? == *sha
            } else if let Some(sha) = &task.sha512 {
                sha512_file(&task.dest)? == *sha
            } else {
                return Ok(());
            };
            if existing_ok {
                return Ok(());
            }
            // A26: битый файл может быть придержан Defender/OneDrive —
            // удаление с повторами вместо мгновенного отказа.
            let _ = crate::util::fs::remove_file_with_retry(&task.dest);
        }
        if let Some(parent) = task.dest.parent() {
            std::fs::create_dir_all(long_path(parent)).map_err(|e| {
                LauncherError::Internal(format!(
                    "create_dir_all для {}: {e}",
                    task.dest.display()
                ))
            })?;
        }
        // ENV-2: `.part` уникален для группы. Два движка (два инстанса одной
        // версии) качают одну библиотеку параллельно: общий `.part` приводил к
        // тому, что второй движок усекал чужой недокачанный файл, а финальный
        // rename ловил ERROR_SHARING_VIOLATION. Валидный dest выше уже отдался
        // из кэша, так что существующий файл гонка не портит.
        let part = long_path(&part_path_unique(&task.dest, &task.group));
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
            // P2-таймаут: TTFB-тишина тоже ограничена READ_IDLE — иначе без
            // общего таймаута клиента worker навечно зависнет с пермитом.
            let resp = match tokio::time::timeout(
                READ_IDLE,
                self.client.raw().get(&task.url).send(),
            )
            .await
            {
                Ok(inner) => inner,
                Err(_) => {
                    // «Сервер не отвечает» — транспортная ошибка, ретрай штатно.
                    if attempt == MAX_ATTEMPTS {
                        return Err(LauncherError::network(format!(
                            "сервер не отвечает более {} с: {}",
                            READ_IDLE.as_secs(),
                            task.url
                        )));
                    }
                    tokio::time::sleep(backoff_delay(attempt)).await;
                    continue;
                }
            };
            match resp {
                Ok(r) if r.status().is_success() => {
                    match self.stream_to_file(r, &part, cancel).await {
                        Ok(streamed) => {
                            // Хэш-проверка обязательна для каждого байта (спека §3):
                            // sha1, при его отсутствии — sha512 (index.json
                            // модпаков); оба не заданы — без проверки.
                            // PERF#3: дайджесты уже посчитаны на лету.
                            let mismatch: Option<(String, String)> = if let Some(sha) = &task.sha1 {
                                (streamed.sha1 != *sha).then_some((sha.clone(), streamed.sha1.clone()))
                            } else if let Some(sha) = &task.sha512 {
                                (streamed.sha512 != *sha)
                                    .then_some((sha.clone(), streamed.sha512.clone()))
                            } else {
                                None
                            };
                            if let Some((expected, actual)) = mismatch {
                                let _ = crate::util::fs::remove_file_with_retry(&part);
                                if attempt == MAX_ATTEMPTS {
                                    return Err(LauncherError::HashMismatch {
                                        path: task.dest.to_string_lossy().into(),
                                        expected,
                                        actual,
                                    });
                                }
                                tokio::time::sleep(backoff_delay(attempt)).await;
                                continue;
                            }
                            // A26: финальное переименование `.part` → цель
                            // ретраится — антивирус/OneDrive держат файл миг.
                            crate::util::fs::rename_with_retry(&part, &task.dest)?;
                            return Ok(());
                        }
                        Err(StreamFail::Cancelled) => {
                            let _ = crate::util::fs::remove_file_with_retry(&part);
                            return Err(LauncherError::Cancelled);
                        }
                        Err(StreamFail::Local(e)) => {
                            // P3: локальный IO (диск полон, права) — полная
                            // перекачка не поможет, фатально с первой попытки.
                            let _ = crate::util::fs::remove_file_with_retry(&part);
                            return Err(e);
                        }
                        Err(StreamFail::Network(e)) => {
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

    /// Потоково писать ответ в файл. PERF#3 (аудит 2026-10-03): SHA-1 и
    /// SHA-512 считаются на лету из тех же чанков — раньше файл читался с
    /// диска второй раз ради проверки. Part-файл пишется без sync_all:
    /// целостность гарантирует хэш-проверка перед финальным rename.
    async fn stream_to_file(
        &self,
        resp: reqwest::Response,
        part: &Path,
        cancel: &AtomicBool,
    ) -> std::result::Result<StreamedFile, StreamFail> {
        use sha1::Digest as _;
        // P3: создание файла — локальный IO, фатально (без ретраев).
        let mut file = std::fs::File::create(part).map_err(|e| StreamFail::Local(e.into()))?;
        let mut stream = resp.bytes_stream();
        let mut written: u64 = 0;
        let mut hasher1 = sha1::Sha1::new();
        let mut hasher512 = sha2::Sha512::new();
        // F17: накопитель семпла — лимит общий на движок, поэтому перед записью
        // ждём разрешения в общем окне (см. throttle_wait).
        let mut sample: usize = 0;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(StreamFail::Cancelled);
            }
            // P2-таймаут: каждый шаг чтения ограничен READ_IDLE — тишина
            // дольше двух минут = «сервер не отвечает» (ретраится штатно).
            // Паузы троттлинга ниже в этот таймаут не попадают.
            let next = match tokio::time::timeout(READ_IDLE, stream.next()).await {
                Ok(next) => next,
                Err(_) => {
                    return Err(StreamFail::Network(LauncherError::network(format!(
                        "сервер не отвечает более {} с ({})",
                        READ_IDLE.as_secs(),
                        part.display()
                    ))));
                }
            };
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.map_err(|e| StreamFail::Network(e.into()))?;
            sample = sample.saturating_add(chunk.len());
            if sample >= THROTTLE_SAMPLE {
                self.throttle_wait(sample).await;
                sample = 0;
            }
            hasher1.update(&chunk);
            hasher512.update(&chunk);
            // P3: запись в локальный файл (диск полон, права) — фатально,
            // полной перекачкой не ретраится.
            file.write_all(&chunk).map_err(|e| StreamFail::Local(e.into()))?;
            written += chunk.len() as u64;
        }
        // Хвост меньше семпла тоже учитываем, иначе недосчёт ускорит следующий файл.
        if sample > 0 {
            self.throttle_wait(sample).await;
        }
        self.rt.received_bytes.fetch_add(written, Ordering::Relaxed);
        Ok(StreamedFile {
            sha1: hex::encode(hasher1.finalize()),
            sha512: hex::encode(hasher512.finalize()),
        })
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

            // D18: событие прогресса несёт группу задач и извлечённый из неё
            // instance_id — UI привязывает прогресс к конкретному инстансу,
            // раньше при двух параллельных установках прогресс прилипал к
            // первому попавшемуся. Движок в приложении создаётся по группе
            // (engine_for), так что группа здесь одна; для гипотетической
            // смеси шлём по событию на каждую группу с агрегатами движка.
            // PERF#6: из множества, без клона строк всех задач под локом.
            let groups: Vec<String> = {
                let groups = self.rt.groups.lock().unwrap();
                let mut g: Vec<String> = groups.iter().cloned().collect();
                g.sort();
                g
            };
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
            for group in groups {
                self.bus.emit(LauncherEvent::DlProgress(DlProgress {
                    group: group.clone(),
                    instance_id: instance_id_from_group(&group),
                    done_files: self.rt.done_files.load(Ordering::Relaxed),
                    total_files: pending + downloading + done + failed + cancelled,
                    done_bytes,
                    total_bytes,
                    bytes_per_sec: speed,
                    eta_secs: eta,
                }));
            }
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
    /// (Ленивых group-флагов пауза не имеет: `paused` глобальный и проверяется
    /// worker'ом до взятия задачи — «no-op до первой задачи» здесь невозможен.)
    pub fn pause(&self) {
        self.rt.paused.store(true, Ordering::SeqCst);
        for flag in self.rt.group_cancels.lock().unwrap().values() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// Отмена группы: мгновенно, `.part` чистятся (в download_one), задачи → Cancelled.
    /// P2-отмена: флаг создаётся немедленно и с `true` — раньше отмена до
    /// первой задачи группы была no-op (ленивый group_flag рождал свежий
    /// флаг с false и отмена терялась).
    pub fn cancel_group(&self, group: &str) {
        let mut map = self.rt.group_cancels.lock().unwrap();
        map.entry(group.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(true)))
            .store(true, Ordering::SeqCst);
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
                e.error = None;
                n += 1;
            }
        }
        n
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    /// D18: извлечение инстанса-владельца из группы задач.
    #[test]
    fn instance_id_from_group_extracts_owner() {
        assert_eq!(instance_id_from_group("instance:abc"), Some("abc".into()));
        assert_eq!(instance_id_from_group("mrpack:x"), Some("x".into()));
        // Суффиксы подгрупп (`:content`/:update/:repair) не портят id.
        assert_eq!(
            instance_id_from_group("instance:abc:content"),
            Some("abc".into())
        );
        // Чужие группы и пустой id — владельца нет.
        assert_eq!(instance_id_from_group("jre:1"), None);
        assert_eq!(instance_id_from_group("instance:"), None);
    }

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
            sha512: None,
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

    /// P2-отмена: cancel_group создаёт флаг немедленно (с `true`) — раньше
    /// отмена до первой задачи группы была no-op.
    #[test]
    fn cancel_group_precreates_true_flag() {
        let engine = DownloadEngine::new(Arc::new(HttpClient::new(None).unwrap()), 1, EventBus::default());
        engine.cancel_group("ghost");
        let map = engine.rt.group_cancels.lock().unwrap();
        let flag = map.get("ghost").expect("флаг существует до первой задачи группы");
        assert!(flag.load(Ordering::SeqCst), "предсозданный флаг обязан быть true");
    }

    /// P3: reset_run_flags (начало run()) сбрасывает paused, global_cancel
    /// и ВСЕ group-флаги — после отмены движок не «кирпич».
    #[test]
    fn reset_run_flags_clears_pause_and_group_flags() {
        let engine = DownloadEngine::new(Arc::new(HttpClient::new(None).unwrap()), 1, EventBus::default());
        engine.cancel_group("g");
        engine.cancel_all();
        engine.pause();
        assert!(engine.rt.paused.load(Ordering::SeqCst));

        engine.reset_run_flags();

        assert!(!engine.rt.paused.load(Ordering::SeqCst));
        assert!(!engine.rt.global_cancel.load(Ordering::SeqCst));
        let map = engine.rt.group_cancels.lock().unwrap();
        let flag = map.get("g").expect("флаг группы жив после сброса");
        assert!(!flag.load(Ordering::SeqCst), "group-флаг сброшен новым прогоном");
    }

    /// P3: дедуп по (URL, dest): тот же URL в другой dest — РАЗНАЯ задача и
    /// не пропадает; тот же URL+dest по-прежнему дедупится.
    #[test]
    fn dedup_key_includes_dest() {
        let engine = DownloadEngine::new(Arc::new(HttpClient::new(None).unwrap()), 1, EventBus::default());
        let mk = |id: &str, dest: PathBuf| DownloadTask {
            id: id.into(),
            url: "http://127.0.0.1:1/x".into(),
            dest,
            sha1: None,
            sha512: None,
            size: None,
            group: "g".into(),
            priority: 0,
        };
        let d1 = PathBuf::from("C:/tmp/one.jar");
        let d2 = PathBuf::from("C:/tmp/two.jar");
        assert_eq!(engine.add_tasks(vec![mk("a", d1.clone())]), 1);
        // Тот же URL+dest — дедуп.
        assert_eq!(engine.add_tasks(vec![mk("b", d1)]), 0);
        // Тот же URL, другой dest — новая задача, не должна молча пропасть.
        assert_eq!(engine.add_tasks(vec![mk("c", d2)]), 1);
    }

    /// A51: предикат part-имён движка — ровно 12 hex после маркера; общий
    /// `.part` атомарных записей и мусор не матчатся.
    #[test]
    fn engine_part_name_predicate() {
        assert!(is_engine_part_name("out.bin.part.0123456789ab"));
        assert!(is_engine_part_name("a.jar.part.ABCDEF012345"));
        assert!(!is_engine_part_name("out.bin.part"), "общий .part — не движковый");
        assert!(!is_engine_part_name("out.bin"));
        assert!(!is_engine_part_name("out.bin.part.0123456789a"), "11 hex");
        assert!(!is_engine_part_name("out.bin.part.0123456789abc"), "13 hex");
        assert!(!is_engine_part_name("out.bin.part.zzzzzzzzzzzz"), "не hex");
    }

    /// A51: part_path_unique даёт имя, которое распознаёт предикат, а
    /// part_files_in находит именно движковые хвосты.
    #[test]
    fn part_files_in_finds_engine_parts_only() {
        let dir = std::env::temp_dir().join("mcl-a51-part-helpers");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("x.jar");
        let p = part_path_unique(&dest, "instance:abc");
        assert!(is_engine_part_name(p.file_name().unwrap().to_str().unwrap()));

        std::fs::write(&p, b"tail").unwrap();
        std::fs::write(dir.join("x.jar.part"), b"atomic_write").unwrap();
        std::fs::write(dir.join("x.jar"), b"done").unwrap();

        let found = part_files_in(&dir);
        assert_eq!(found.len(), 1, "только движковый .part.<hex>: {found:?}");
        assert_eq!(found[0], p);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
