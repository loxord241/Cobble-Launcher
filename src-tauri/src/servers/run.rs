//! Запуск/остановка/надзор выделенных Minecraft-серверов (stdin-управление).
//!
//! Отличия от игры (instances/run.rs): процесс долгоживущий, команда `stop`
//! в stdin — штатное сохранение мира (проверено живьём: exit 0, мир цел),
//! жёсткий taskkill — запасной выход по таймауту. Сервер пишет свой
//! `logs/latest.log` сам, поэтому супервизор файл лога НЕ ведёт — только
//! события `server_event` в UI.
//!
//! События идут через собственный канал `subscribe_server_events()`:
//! events.rs вне полосы правок этого модуля, контракт сигнатуры
//! `server_start(.., bus: &EventBus, ..)` зафиксирован ТЗ.

use crate::errors::{LauncherError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

// ---------- контракт вызова ----------

/// Что и как запускать (собирает вызывающий из реестра).
#[derive(Debug, Clone)]
pub struct ServerRunSpec {
    pub id: String,
    pub dir: std::path::PathBuf,
    pub ram_mb: u32,
    /// None или "vanilla" — ванильный `server.jar`; "fabric" | "forge" | "neoforge".
    pub loader: Option<String>,
    pub loader_version: Option<String>,
    pub mc_version: String,
    /// Требуемый мажор Java из version JSON (0 — эвристика fallback_major).
    pub java_major: u32,
}

/// Каталог конкретного сервера: `<servers_root>/<id>`, где корень —
/// `servers_root` из servers/mod.rs (единый с реестром servers.json).
fn server_dir(paths: &crate::paths::Paths, id: &str) -> PathBuf {
    crate::servers::servers_root(paths).join(id)
}

// ---------- сериализация стартов/удаления ----------

/// Гвард сериализации стартов серверов; server_delete берёт его же,
/// чтобы старт не просочился между проверкой is_running и удалением каталога.
static START_GUARD: Mutex<()> = Mutex::new(());

/// Замок на гвард: отравленный Mutex после паники нити — не повод терять
/// старт/удаление (паттерн кодовой базы: into_inner, как lock_children).
pub(crate) fn start_guard() -> MutexGuard<'static, ()> {
    START_GUARD.lock().unwrap_or_else(|e| e.into_inner())
}

/// Запустить сервер. Проверки ДО спавна: каталог есть; eula.txt существует
/// и eula=true; сервер ещё не запущен; порт свободен (пробный bind на
/// 0.0.0.0:port). Спавнит java с piped stdin/stdout/stderr и CREATE_NO_WINDOW,
/// cwd=dir, пишет pid+метку старта в `<servers_root>/<id>/.server.lock`.
/// Возврат: Ok(pid).
pub fn server_start(
    paths: &crate::paths::Paths,
    bus: &crate::events::EventBus,
    spec: ServerRunSpec,
    port: u16,
) -> Result<u32> {
    // События серверов идут через subscribe_server_events() (см. шапку модуля).
    let _ = bus;
    crate::instances::valid_id(&spec.id)?;
    // P2-ревизии: двойной старт одного id (обе проверки is_running проходят
    // до записи лока) — сериализуем секцию проверка→спавн→лок глобально:
    // старты редки, а per-id карта усложнила бы уборку.
    let _start_guard = start_guard();

    // 1. Каталог сервера существует. Совпадение с каноническим
    //    `<servers_root>/<id>` — нестрогое (warn): cwd берём из spec.
    if !spec.dir.is_dir() {
        return Err(LauncherError::InvalidInput(format!(
            "каталог сервера не найден: {}",
            spec.dir.display()
        )));
    }
    if spec.dir != server_dir(paths, &spec.id) {
        tracing::warn!(
            "сервер {}: каталог {} не совпадает с каноническим {}",
            spec.id,
            spec.dir.display(),
            server_dir(paths, &spec.id).display()
        );
    }

    // 2. EULA: сервер без eula=true на первом старте сам завершится с ошибкой —
    //    честнее отказать до спавна.
    if !eula_accepted(&spec.dir) {
        return Err(LauncherError::InvalidInput(
            "EULA не принята — сначала принять EULA (строка eula=true в eula.txt каталога сервера)"
                .into(),
        ));
    }

    // 3. Уже запущен (до проверки порта: у работающего сервера порт и так
    //    занят — сообщение «уже запущен» честнее).
    if is_running(paths, &spec.id) {
        return Err(LauncherError::InvalidInput(format!(
            "сервер {} уже запущен",
            spec.id
        )));
    }

    // 4. Порт свободен (пробный bind; порт 0 не проверить — bind всегда удаётся).
    if port_in_use(port) {
        return Err(LauncherError::InvalidInput(format!(
            "порт {port} занят — освободите его или смените порт сервера"
        )));
    }

    // 5. Java (v1: только найденная на машине) и командная строка по лоадеру.
    // Приёмка 0.2.10: для снапшотов 26.x эвристика fallback_major занижала
    // (21 вместо 25) — супервизор запускал старую Java и сервер падал с
    // UnsupportedClassVersionError. Мажор берём из записи (version JSON).
    let need_major = if spec.java_major > 0 {
        spec.java_major
    } else {
        crate::instances::run::fallback_major(&spec.mc_version)
    };
    let java = resolve_java(paths, need_major)?;
    let argv = build_command(&java, &spec)?;

    // 6. Спавн: stdin piped — через него уходит «stop».
    let mut command = std::process::Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(crate::util::fs::strip_long_prefix(&spec.dir))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    crate::util::win::hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| LauncherError::InvalidInput(format!("не удалось запустить сервер: {e}")))?;
    let pid = child.id();
    let started_at_ms = unix_ms();

    // 7. Лок: pid + sysinfo-старт (строгая живость, D62-семантика) + unix-ms
    //    метка старта. Пишем атомарно; сбой не откатывает спавн — только warn.
    let start_time = crate::instances::process_start_time(pid).unwrap_or(0);
    let lock = ServerLock {
        pid,
        start_time,
        started_at_ms,
    };
    if let Err(e) = crate::util::fs::atomic_write(
        &crate::util::fs::long_path(&lock_path(paths, &spec.id)),
        lock.to_line().as_bytes(),
    ) {
        tracing::warn!("сервер {}: .server.lock не записан: {e}", spec.id);
    }

    // 8. stdin остаётся в реестре для server_stop; Child уходит супервизору.
    let stdin = child.stdin.take();
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    insert_child(&spec.id, ChildEntry { stdin, generation });

    emit_server_event(ServerEvent::ServerEvent {
        server_id: spec.id.clone(),
        kind: ServerEventKind::Started,
        line: None,
        code: None,
    });

    // 9. Супервизор: живёт до смерти процесса; паника задачи приложение не
    //    роняет, уборку (лок + запись реестра) делает Drop-guard. Вместе с ним
    //    уходит корень данных ЭТОГО запуска: playtime пишется по Paths
    //    (CLI --data-dir уважается), а не в дефолтный каталог.
    let id = spec.id.clone();
    let lp = lock_path(paths, &spec.id);
    let data_root = paths.root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        supervise(lp, id, child, lock, generation, data_root)
    });
    tracing::info!("сервер {}: запущен (PID {pid}, порт {port})", spec.id);
    Ok(pid)
}

/// Корректная остановка: «stop\n» в stdin, ждать выхода до 60 с; не дождались —
/// taskkill /F /T (запасной). Возвращает true, если вышли сами. Если stdin
/// недоступен (лаунчер перезапускали, а сервер жив) — сразу жёсткий kill c
/// честным false. Не запущен — false (останавливать нечего).
pub fn server_stop(paths: &crate::paths::Paths, id: &str) -> Result<bool> {
    crate::instances::valid_id(id)?;

    // 1. Лаунчер держит stdin — штатная остановка.
    let stdin = lock_children().get_mut(id).and_then(|e| e.stdin.take());
    if let Some(mut stdin) = stdin {
        tracing::info!("сервер {id}: отправляю «stop» в stdin");
        if let Err(e) = stdin.write_all(b"stop\n").and_then(|()| stdin.flush()) {
            tracing::warn!("сервер {id}: stdin не принят ({e})");
        }
        drop(stdin); // сервер после обработки `stop` увидит EOF — не мешаем
        let deadline = std::time::Instant::now() + STOP_TIMEOUT;
        while std::time::Instant::now() < deadline {
            if !is_running(paths, id) {
                tracing::info!("сервер {id}: вышел сам после «stop»");
                return Ok(true);
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        // Запасной выход. Супервизор жив — сам увидит выход, пришлёт crashed
        // и снимет лок: здесь лок не трогаем.
        tracing::warn!("сервер {id}: не вышел за 60 с — taskkill /F /T");
        if let Some(pid) = running_pid(paths, id) {
            force_kill_tree(pid);
        }
        return Ok(false);
    }

    // 2. Процесса в реестре нет, но лок живой: лаунчер перезапускали, stdin
    //    потерян. Штатно сохранить мир через `stop` уже нельзя — честный
    //    жёсткий kill и уборка лока (супервизора нет, кроме нас никто).
    if let Some(pid) = running_pid(paths, id) {
        tracing::warn!(
            "сервер {id}: stdin недоступен (лаунчер перезапускался) — taskkill /F /T PID {pid}"
        );
        force_kill_tree(pid);
        // P3-ревизии: между чтением pid и удалением мог стартовать новый
        // запуск — снимаем лок только если он всё ещё описывает убитый процесс.
        if read_lock(paths, id).is_some_and(|l| l.pid == pid) {
            crate::util::fs::remove_file_ignore(
                crate::util::fs::long_path(&lock_path(paths, id)).as_path(),
            );
        }
        return Ok(false);
    }

    // 3. Не запущен.
    tracing::info!("сервер {id}: не запущен — останавливать нечего");
    Ok(false)
}

/// Запущен ли сервер: `.server.lock` + живость процесса по паре
/// (pid, start_time) — как D62 у инстансов.
pub fn is_running(paths: &crate::paths::Paths, id: &str) -> bool {
    running_pid(paths, id).is_some()
}

/// PID запущенного сервера из `.server.lock` (None = не запущен / протух).
pub fn running_pid(paths: &crate::paths::Paths, id: &str) -> Option<u32> {
    if crate::instances::valid_id(id).is_err() {
        return None;
    }
    let text =
        std::fs::read_to_string(crate::util::fs::long_path(&lock_path(paths, id))).ok()?;
    let lock = ServerLock::parse(&text)?;
    // Строгая пара: PID жив И start_time совпадает (Windows переиспользует
    // PID); startTime:0 (не успели прочитать) протухает мгновенно.
    (lock.start_time != 0
        && crate::instances::process_start_time(lock.pid) == Some(lock.start_time))
        .then_some(lock.pid)
}

// ---------- события UI ----------

/// Род события сервера: строки лога пакетами, жизненный цикл — по одному.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerEventKind {
    Log,
    Started,
    Exited,
    Crashed,
}

/// `{"event":"server_event","serverId":"..","kind":"log"|"started"|"exited"|"crashed","line":"..","code":0}`
/// — line только у log (пакет строк через `\n`), code только у exited/crashed.
/// Один вариант с тегом — тот же паттерн сериализации, что у LauncherEvent.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ServerEvent {
    ServerEvent {
        server_id: String,
        kind: ServerEventKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        line: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
    },
}

/// Канал событий серверов: ёмкость как у EventBus (подробный лог сервера).
static SERVER_BUS: OnceLock<tokio::sync::broadcast::Sender<ServerEvent>> = OnceLock::new();

fn server_bus() -> &'static tokio::sync::broadcast::Sender<ServerEvent> {
    SERVER_BUS.get_or_init(|| tokio::sync::broadcast::channel(8192).0)
}

/// Подписка UI на события серверов (мост `app.emit("server_event", ev)` —
/// точка интеграции в commands/, здесь AppHandle недоступен).
pub fn subscribe_server_events() -> tokio::sync::broadcast::Receiver<ServerEvent> {
    server_bus().subscribe()
}

fn emit_server_event(ev: ServerEvent) {
    // Нет подписчиков — не ошибка (как EventBus: CLI без UI).
    let _ = server_bus().send(ev);
}

fn emit_log(id: &str, blob: &str) {
    emit_server_event(ServerEvent::ServerEvent {
        server_id: id.to_string(),
        kind: ServerEventKind::Log,
        line: Some(blob.to_string()),
        code: None,
    });
}

// ---------- реестр спавнов (stdin для stop) ----------

/// stdin живого сервера + номер поколения спавна (уборка только своей записи).
struct ChildEntry {
    stdin: Option<std::process::ChildStdin>,
    generation: u64,
}

static CHILDREN: OnceLock<Mutex<HashMap<String, ChildEntry>>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn children() -> &'static Mutex<HashMap<String, ChildEntry>> {
    CHILDREN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Замок на реестр: отравленный Mutex после паники нити — не повод терять
/// остановку сервера (паттерн кодовой базы: into_inner).
fn lock_children() -> MutexGuard<'static, HashMap<String, ChildEntry>> {
    children().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn insert_child(id: &str, entry: ChildEntry) {
    lock_children().insert(id.to_string(), entry);
}

// ---------- .server.lock ----------

const STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Содержимое `.server.lock` — JSON одной строкой, camelCase:
/// `{"pid":123,"startTime":456789,"startedAtMs":1759900000000}`.
/// `startTime` — sysinfo-старт процесса (сек) для строгой пары с PID
/// (Windows переиспользует PID — D62); `startedAtMs` — unix-ms метка старта.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerLock {
    pid: u32,
    start_time: u64,
    #[serde(default)]
    started_at_ms: u64,
}

impl ServerLock {
    fn to_line(&self) -> String {
        // Два-три примитива — сериализации нечем упасть.
        serde_json::to_string(self).unwrap_or_default()
    }

    fn parse(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        serde_json::from_str(trimmed).ok()
    }
}

fn lock_path(paths: &crate::paths::Paths, id: &str) -> PathBuf {
    server_dir(paths, id).join(".server.lock")
}

/// Прочитать .server.lock (None — нет/битый).
fn read_lock(paths: &crate::paths::Paths, id: &str) -> Option<ServerLock> {
    let text =
        std::fs::read_to_string(crate::util::fs::long_path(&lock_path(paths, id))).ok()?;
    ServerLock::parse(&text)
}

/// Unix-время в миллисекундах — метка старта сервера.
fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------- проверки перед спавном ----------

/// EULA принята? Единый парсер с UI (worlds::read_eula_accepted): толерантный
/// к регистру/пробелам, побеждает последняя строка `eula=` — раньше здесь был
/// свой парсер, и рукописанный «eula = TRUE» расходился с карточкой (P2-ревизии).
fn eula_accepted(dir: &Path) -> bool {
    crate::servers::worlds::read_eula_accepted(dir)
}

/// Свободен ли порт: пробный bind на 0.0.0.0:port (как будет делать сервер).
fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("0.0.0.0", port)).is_err()
}

// ---------- java (v1: только системная/рантаймы лаунчера) ----------

/// Java для сервера: PATH, Program Files, ~/.jdks, рантаймы лаунчера и
/// пользовательские пути из настроек; major-минимум — из версии MC.
/// Автозагрузку JRE не тащим: подходящей нет — InvalidInput с честным советом.
fn resolve_java(paths: &crate::paths::Paths, need_major: u32) -> Result<PathBuf> {
    let settings = crate::settings::Settings::load(&paths.settings_file()).unwrap_or_default();
    let mut candidates: Vec<(PathBuf, u32)> = Vec::new();
    for (exe, origin) in
        crate::java::detect::find_java_exes(&paths.runtime_dir(), &settings.extra_java_paths)
    {
        // Проба `java -version` — сервер стартует редко, цена приемлема.
        let Ok(info) = crate::java::detect::test_java(&exe.to_string_lossy()) else {
            tracing::debug!("java {exe:?} ({origin}) не отвечает — пропускаю");
            continue;
        };
        let major = u32::try_from(info.major).unwrap_or(0);
        if major >= need_major {
            candidates.push((exe, major));
        }
    }
    // Точная мажорная версия → ближайшая старшая → наибольшая доступная
    // (последняя — осознанный рингер: для снапшотов эвристика может
    // завышать/занижать, а запуск на новейшей Java лучше отказа).
    let best = candidates
        .iter()
        .find(|(_, m)| *m == need_major)
        .or_else(|| candidates.iter().filter(|(_, m)| *m > need_major).min_by_key(|(_, m)| *m))
        .or_else(|| candidates.iter().max_by_key(|(_, m)| *m));
    best.map(|(exe, _)| exe.clone()).ok_or_else(|| {
        LauncherError::InvalidInput(format!(
            "подходящая Java ({need_major}+) не найдена — сначала запустите игру один раз или установите Java"
        ))
    })
}

// ---------- командная строка ----------

/// `-Xmx` с клампом ram_mb к валидному диапазону (зеркало instances::run::xmx_flag:
/// реестр серверов мог быть отредактирован руками мимо IPC-валидатора).
fn xmx_flag(ram_mb: u32) -> String {
    format!(
        "-Xmx{}M",
        ram_mb.clamp(crate::servers::MIN_RAM_MB, crate::servers::MAX_RAM_MB)
    )
}

/// Чистое построение командной строки (тестируется офлайн): argv[0] — java,
/// дальше JVM-флаги и режим запуска по лоадеру. Отсутствие server.jar /
/// fabric-server-launch.jar / win_args.txt — InvalidInput «серверное ПО не
/// установлено» ещё до спавна.
pub fn build_command(java_exe: &Path, spec: &ServerRunSpec) -> Result<Vec<String>> {
    let mut args = vec![xmx_flag(spec.ram_mb)];
    match spec.loader.as_deref() {
        None | Some("vanilla") => {
            require_file(&spec.dir.join("server.jar"), "server.jar")?;
            args.extend(["-jar".into(), "server.jar".into(), "nogui".into()]);
        }
        Some("fabric") => {
            require_file(
                &spec.dir.join("fabric-server-launch.jar"),
                "fabric-server-launch.jar",
            )?;
            args.extend([
                "-jar".into(),
                "fabric-server-launch.jar".into(),
                "nogui".into(),
            ]);
        }
        Some(loader @ ("forge" | "neoforge")) => {
            let maven = if loader == "forge" {
                "net/minecraftforge/forge"
            } else {
                "net/neoforged/neoforge"
            };
            let Some(lv) = spec.loader_version.as_deref() else {
                return Err(LauncherError::InvalidInput(format!(
                    "не указана версия загрузчика {loader} сервера"
                )));
            };
            // Forge-сборка — «{mc}-{loader_version}», NeoForge — просто версия.
            let exact = if loader == "forge" {
                format!("{}-{}", spec.mc_version, lv)
            } else {
                lv.to_string()
            };
            let maven_dir = spec.dir.join("libraries").join(maven);
            let win_args = find_win_args(&maven_dir, &exact).ok_or_else(|| {
                LauncherError::InvalidInput(format!(
                    "серверное ПО не установлено: win_args.txt для {loader} не найден в {} (сначала установите серверное ПО)",
                    maven_dir.display()
                ))
            })?;
            args.push(format!("@{}", win_args.to_string_lossy()));
            args.push("nogui".into());
        }
        Some(other) => {
            return Err(LauncherError::InvalidInput(format!(
                "неизвестный загрузчик сервера {other:?} (vanilla|fabric|forge|neoforge)"
            )));
        }
    }
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(java_exe.to_string_lossy().into_owned());
    argv.extend(args);
    Ok(argv)
}

/// Jar-часть серверного ПО обязана существовать — иначе java честно скажет
/// «Error: Unable to access jarfile» уже после спавна; лучше отказ до него.
fn require_file(path: &Path, what: &str) -> Result<()> {
    if path.is_file() {
        return Ok(());
    }
    Err(LauncherError::InvalidInput(format!(
        "серверное ПО не установлено: {what} не найден в {}",
        path.parent().unwrap_or(path).display()
    )))
}

/// win_args.txt: точное имя сборки, иначе glob `*/win_args.txt` — у версий
/// Forge бывают суффиксы, надёжнее поиск по каталогу, чем точная строка.
fn find_win_args(maven_dir: &Path, exact: &str) -> Option<PathBuf> {
    let direct = maven_dir.join(exact).join("win_args.txt");
    if direct.is_file() {
        return Some(direct);
    }
    // P2-ревизии: лексикографический первый hit мог поднять старую сборку
    // после переустановок — берём самую свежую по mtime win_args.txt.
    std::fs::read_dir(crate::util::fs::long_path(maven_dir))
        .ok()?
        .flatten()
        .map(|e| e.path().join("win_args.txt"))
        .filter(|p| p.is_file())
        .max_by_key(|p| {
            std::fs::metadata(crate::util::fs::long_path(p))
                .and_then(|m| m.modified())
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0))
                .unwrap_or(0)
        })
}

// ---------- супервизия ----------

/// Уборка супервизора: снимает `.server.lock` и свою запись реестра. Вызывается
/// и при панике нити (tokio разматывает задачу) — приложение не теряет лок.
struct SupervisorGuard {
    lock_path: PathBuf,
    id: String,
    lock: ServerLock,
    generation: u64,
}

impl Drop for SupervisorGuard {
    fn drop(&mut self) {
        // Лок снимаем только СВОЙ: между смертью процесса и Drop пользователь
        // мог успеть перезапустить сервер — его свежий лок трогать нельзя.
        if let Ok(text) =
            std::fs::read_to_string(crate::util::fs::long_path(&self.lock_path))
        {
            if ServerLock::parse(&text).as_ref() == Some(&self.lock) {
                crate::util::fs::remove_file_ignore(
                    crate::util::fs::long_path(&self.lock_path).as_path(),
                );
            }
        }
        let mut map = lock_children();
        if map.get(&self.id).is_some_and(|e| e.generation == self.generation) {
            map.remove(&self.id);
        }
    }
}

/// Пачка строк лога: 50 строк ИЛИ 200 мс с первой непосланной — что раньше
/// (не спамить шину). Строки пакета склеиваются `\n` в поле `line` события.
/// В файл не пишет: сервер ведёт свой logs/latest.log сам.
struct LogBatch {
    lines: Vec<String>,
    since_first: Option<std::time::Instant>,
}

impl LogBatch {
    const MAX_LINES: usize = 50;
    const MAX_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

    fn new() -> Self {
        Self {
            lines: Vec::new(),
            since_first: None,
        }
    }

    /// Строка копится с redaction (секреты в логах сервера тоже бывают).
    fn push(&mut self, line: String) {
        self.lines.push(crate::util::redact::redact(&line));
        if self.since_first.is_none() {
            self.since_first = Some(std::time::Instant::now());
        }
    }

    fn matured(&self) -> bool {
        self.since_first
            .is_some_and(|t| t.elapsed() >= Self::MAX_DELAY)
    }

    /// Пора слать: 50 строк или 200 мс — что раньше.
    fn ready(&self) -> bool {
        self.lines.len() >= Self::MAX_LINES || self.matured()
    }

    /// Забрать пачку склеенной `\n` (None — пусто); таймер сбрасывается.
    fn take(&mut self) -> Option<String> {
        if self.lines.is_empty() {
            return None;
        }
        self.since_first = None;
        Some(std::mem::take(&mut self.lines).join("\n"))
    }
}

/// Декод строки потока: байты до `\n` с lossy-декодом (не-UTF-8 → U+FFFD,
/// поток не обрывается), хвостовой `\r` от CRLF отрезается. Копия private-
/// хелпера instances::run (тот вне полосы вызовов).
fn lossy_line(buf: &[u8]) -> String {
    let mut line = String::from_utf8_lossy(buf).into_owned();
    while line.ends_with('\r') || line.ends_with('\n') {
        line.pop();
    }
    line
}

/// Общий бюджет финализации супервизора (~700 мс): дренаж хвоста лога и
/// отсрочка отсоединения читателей делят один дедлайн (см. supervise/finalize_run).
const FINALIZE_BUDGET: std::time::Duration = std::time::Duration::from_millis(700);

/// Шаг опроса is_finished при join читателей в пределах бюджета.
const READER_POLL: std::time::Duration = std::time::Duration::from_millis(10);

/// Надзор: строки stdout/stderr → события (пачками), по выходе — единая
/// финализация. Возвращает управление только после смерти процесса (или
/// отсоединения застрявших читателей — см. ниже).
///
/// Читающие потоки: пайпы stdout/stderr разобирают свои нити, строки идут в
/// канал под тем же батчером. Выход детектится двумя путями — тиком
/// try_wait (ветка Timeout) и закрытием обоих пайпов (ветка Disconnected);
/// оба сходятся в finalize_run. Дедлайн финализации общий, FINALIZE_BUDGET:
/// 1) дренаж хвоста лога — при выходе по тику читатели могли не дочитать,
///    остаток (хвост краш-стека) выкачивается из канала тем же батчером до
///    дедлайна, без вечного ожидания;
/// 2) читатели — join на остатке того же дедлайна. Если ребёнок унаследовал
///    stdout/stderr и пережил родителя, read в читателе не вернётся никогда:
///    по истечении бюджета поток отсоединяется (daemon-семантика) с warn —
///    супервизор обязан завершиться всегда, лок и реестр не зависают.
fn supervise(
    lock_path: PathBuf,
    id: String,
    mut child: std::process::Child,
    written_lock: ServerLock,
    generation: u64,
    data_root: PathBuf,
) {
    let started = std::time::Instant::now();
    let _guard = SupervisorGuard {
        lock_path,
        id: id.clone(),
        lock: written_lock,
        generation,
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let streams: Vec<Option<Box<dyn std::io::Read + Send>>> = vec![
        stdout.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        stderr.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ];
    let mut readers = Vec::new();
    for stream in streams.into_iter().flatten() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stream);
            for buf in reader.split(b'\n') {
                let Ok(buf) = buf else { break };
                if tx.send(lossy_line(&buf)).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);

    let mut batch = LogBatch::new();
    let code = loop {
        match rx.recv_timeout(LogBatch::MAX_DELAY) {
            Ok(line) => {
                batch.push(line);
                if batch.ready() {
                    if let Some(blob) = batch.take() {
                        emit_log(&id, &blob);
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => match child.try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) => {
                    if let Some(blob) = batch.take_if_matured() {
                        emit_log(&id, &blob);
                    }
                }
                Err(e) => tracing::warn!("сервер {id}: try_wait: {e}"),
            },
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // Оба пайпа дочитаны — процесс завершился (или закрывает потоки
                // перед выходом): дожидаемся код завершения.
                break child.wait().ok().and_then(|s| s.code());
            }
        }
    };
    finalize_run(rx, readers, &id, &mut batch, code, started, &data_root);
}

/// Единая финализация обоих путей выхода супервизора (тик try_wait и
/// Disconnected): дренаж хвоста лога, освобождение читателей, playtime и
/// событие exited/crashed. Дедлайн общий (FINALIZE_BUDGET): сначала дренаж,
/// читателям — остаток; обычный выход (пайпы закрыты) проходит оба шага
/// мгновенно, застрявшие на наследнике — отсекаются по бюджету.
fn finalize_run(
    rx: std::sync::mpsc::Receiver<String>,
    readers: Vec<std::thread::JoinHandle<()>>,
    id: &str,
    batch: &mut LogBatch,
    code: Option<i32>,
    started: std::time::Instant,
    data_root: &Path,
) {
    let deadline = std::time::Instant::now() + FINALIZE_BUDGET;

    // 1. Дренаж хвоста: строка, поданная в канал после детекта выхода, ещё
    //    не потеряна — выкачиваем тем же батчером до дедлайна.
    while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
        match rx.recv_timeout(remaining) {
            Ok(line) => {
                batch.push(line);
                if batch.ready() {
                    if let Some(blob) = batch.take() {
                        emit_log(id, &blob);
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // 2. Читатели: join на остатке дедлайна; не закрылись (наследник держит
    //    пайп) — отсоединяем, супервизор не должен висеть на чужом read.
    for handle in readers {
        if !join_reader(handle, deadline) {
            tracing::warn!(
                "сервер {id}: читатель лога не закрылся (наследник держит пайп) — поток отсоединён"
            );
        }
    }

    // 3. Playtime сессии — в корень данных ЭТОГО запуска под id сервера; оба
    //    пути выхода пишут одинаково (одна точка вызова). Ошибка записи не
    //    роняет финализацию — статистика не критична.
    if let Err(e) = crate::playtime::record_in_dir(data_root, id, started.elapsed().as_secs()) {
        tracing::warn!("сервер {id}: playtime не записан: {e}");
    }

    finalize(id, batch, code);
}

/// Join читателя с дедлайном: поток, дошедший до EOF, закрывается мгновенно;
/// застрявший в read (наследник держит пайп) отсоединяется по истечении
/// бюджета (JoinHandle в drop — detach). false — не дождались, поток отсоединён.
fn join_reader(handle: std::thread::JoinHandle<()>, deadline: std::time::Instant) -> bool {
    while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
        if handle.is_finished() {
            return handle.join().is_ok();
        }
        std::thread::sleep(remaining.min(READER_POLL));
    }
    // Последняя проверка на границе бюджета: закрывшийся микросекундой раньше
    // читатель всё же присоединится, а не отсоединится.
    handle.is_finished() && handle.join().is_ok()
}

impl LogBatch {
    /// Пачка, «созревшая» по времени, — для тиков без строк.
    fn take_if_matured(&mut self) -> Option<String> {
        if self.matured() {
            self.take()
        } else {
            None
        }
    }
}

/// Финал: дочитать пачку, прислать exited (код 0) или crashed (иначе;
/// неизвестный код — тоже crashed, честнее для сигнальных завершений).
fn finalize(id: &str, batch: &mut LogBatch, code: Option<i32>) {
    if let Some(blob) = batch.take() {
        emit_log(id, &blob);
    }
    let kind = if code == Some(0) {
        ServerEventKind::Exited
    } else {
        ServerEventKind::Crashed
    };
    tracing::info!("сервер {id}: завершён (код {code:?})");
    emit_server_event(ServerEvent::ServerEvent {
        server_id: id.to_string(),
        kind,
        line: None,
        code,
    });
}

/// Жёсткий запасной выход: убить дерево процессов (штатный `stop` не прошёл).
fn force_kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let mut c = std::process::Command::new("taskkill");
        c.args(["/F", "/T", "/PID", &pid.to_string()]);
        crate::util::win::hide_console(&mut c);
        match c.output() {
            Ok(out) if out.status.success() => {}
            Ok(out) => tracing::warn!(
                "taskkill /PID {pid}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            Err(e) => tracing::warn!("taskkill /PID {pid} не запустился: {e}"),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths() -> (tempfile::TempDir, crate::paths::Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        (dir, paths)
    }

    fn spec(id: &str, dir: PathBuf) -> ServerRunSpec {
        ServerRunSpec {
            id: id.to_string(),
            dir,
            ram_mb: 2048,
            loader: None,
            loader_version: None,
            mc_version: "1.20.1".into(),
            java_major: 21,
        }
    }

    /// Формат .server.lock: JSON round-trip, camelCase-поля, пустой/битый файл
    /// не парсится.
    #[test]
    fn server_lock_roundtrip() {
        let lock = ServerLock {
            pid: 42,
            start_time: 1000,
            started_at_ms: 1_700_000_000_000,
        };
        let line = lock.to_line();
        assert!(line.contains("\"pid\":42"), "{line}");
        assert!(line.contains("\"startTime\":1000"), "{line}");
        assert!(line.contains("\"startedAtMs\":1700000000000"), "{line}");
        assert_eq!(ServerLock::parse(&line), Some(lock));
        assert_eq!(ServerLock::parse(""), None, "пустой файл — None");
        assert_eq!(ServerLock::parse("мусор"), None, "битый файл — None");
    }

    /// Живость по локу: нет лока — false; мёртвый PID — false; живой PID с
    /// чужим start_time (переиспользование PID) — false; свой живой PID с
    /// верной парой — true.
    #[test]
    fn running_pid_checks_pair_strictly() {
        let (_d, paths) = test_paths();
        let id = "lock-check";
        std::fs::create_dir_all(crate::util::fs::long_path(&server_dir(&paths, id))).unwrap();

        // Лока нет.
        assert!(!is_running(&paths, id));
        assert_eq!(running_pid(&paths, id), None);

        // Мёртвый PID.
        let dead = ServerLock {
            pid: u32::MAX,
            start_time: 123,
            started_at_ms: 0,
        };
        crate::util::fs::atomic_write(
            &crate::util::fs::long_path(&lock_path(&paths, id)),
            dead.to_line().as_bytes(),
        )
        .unwrap();
        assert!(!is_running(&paths, id), "несуществующий PID — не запущен");

        // Свой PID, но не тот start_time — «переиспользованный» PID.
        let my = std::process::id();
        let my_start = crate::instances::process_start_time(my).unwrap();
        let stale = ServerLock {
            pid: my,
            start_time: my_start + 1,
            started_at_ms: 0,
        };
        crate::util::fs::atomic_write(
            &crate::util::fs::long_path(&lock_path(&paths, id)),
            stale.to_line().as_bytes(),
        )
        .unwrap();
        assert!(!is_running(&paths, id), "чужой start_time — протух");

        // Верная пара — живой.
        let alive = ServerLock {
            pid: my,
            start_time: my_start,
            started_at_ms: unix_ms(),
        };
        crate::util::fs::atomic_write(
            &crate::util::fs::long_path(&lock_path(&paths, id)),
            alive.to_line().as_bytes(),
        )
        .unwrap();
        assert!(is_running(&paths, id));
        assert_eq!(running_pid(&paths, id), Some(my));

        // startTime:0 (не успели прочитать) протухает мгновенно.
        let zero = ServerLock {
            pid: my,
            start_time: 0,
            started_at_ms: 0,
        };
        crate::util::fs::atomic_write(
            &crate::util::fs::long_path(&lock_path(&paths, id)),
            zero.to_line().as_bytes(),
        )
        .unwrap();
        assert!(!is_running(&paths, id), "startTime:0 — протух");
    }

    /// Vanilla и Fabric: ровно один и тот же скелет аргументов, отличается jar.
    #[test]
    fn build_command_vanilla_and_fabric() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("server.jar"), b"jar").unwrap();
        let argv = build_command(Path::new("C:\\java\\java.exe"), &spec("s", dir.path().into()))
            .unwrap();
        assert_eq!(
            argv,
            vec![
                "C:\\java\\java.exe",
                "-Xmx2048M",
                "-jar",
                "server.jar",
                "nogui"
            ]
        );

        let fdir = tempfile::tempdir().unwrap();
        std::fs::write(fdir.path().join("fabric-server-launch.jar"), b"jar").unwrap();
        let mut fspec = spec("s", fdir.path().into());
        fspec.loader = Some("fabric".into());
        fspec.loader_version = Some("0.16.9".into());
        let argv = build_command(Path::new("java"), &fspec).unwrap();
        assert_eq!(
            argv,
            vec!["java", "-Xmx2048M", "-jar", "fabric-server-launch.jar", "nogui"]
        );
    }

    /// -Xmx клампится к диапазону инстансов: мусор из реестра не кладёт машину.
    #[test]
    fn xmx_clamps_ram() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("server.jar"), b"jar").unwrap();
        let mut s = spec("s", dir.path().into());
        s.ram_mb = 0;
        assert!(
            build_command(Path::new("java"), &s)
                .unwrap()
                .contains(&format!("-Xmx{}M", crate::servers::MIN_RAM_MB))
        );
        s.ram_mb = 99_999_999;
        assert!(
            build_command(Path::new("java"), &s)
                .unwrap()
                .contains(&format!("-Xmx{}M", crate::servers::MAX_RAM_MB))
        );
    }

    /// Forge: точное имя сборки «{mc}-{loader_version}»; суффиксованная версия
    /// находится glob-фолбэком; нет win_args.txt — «серверное ПО не установлено».
    #[test]
    fn build_command_forge_exact_glob_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let forge = dir.path().join("libraries/net/minecraftforge/forge");
        std::fs::create_dir_all(forge.join("1.20.1-47.2.0")).unwrap();
        std::fs::write(forge.join("1.20.1-47.2.0/win_args.txt"), b"@args").unwrap();

        let mut s = spec("s", dir.path().into());
        s.loader = Some("forge".into());
        s.loader_version = Some("47.2.0".into());
        let argv = build_command(Path::new("java"), &s).unwrap();
        assert_eq!(argv[0], "java");
        assert_eq!(argv[1], "-Xmx2048M");
        assert_eq!(argv[3], "nogui");
        // Сравнение как путей: read_dir нормализует разделители, / и \ равны.
        let arg = argv[2].strip_prefix('@').expect("@win_args");
        assert_eq!(
            Path::new(arg),
            forge.join("1.20.1-47.2.0").join("win_args.txt").as_path(),
            "точное имя сборки {}-{}",
            s.mc_version,
            s.loader_version.as_deref().unwrap_or_default()
        );

        // Glob: версия с суффиксом находится, даже если точное имя не совпало.
        let dir2 = tempfile::tempdir().unwrap();
        let forge2 = dir2.path().join("libraries/net/minecraftforge/forge");
        std::fs::create_dir_all(forge2.join("1.20.1-47.2.0-shattered")).unwrap();
        std::fs::write(forge2.join("1.20.1-47.2.0-shattered/win_args.txt"), b"@a").unwrap();
        let mut s2 = spec("s", dir2.path().into());
        s2.loader = Some("forge".into());
        s2.loader_version = Some("47.2.0".into());
        let argv = build_command(Path::new("java"), &s2).unwrap();
        assert!(
            argv[2].ends_with("win_args.txt") && argv[2].starts_with('@'),
            "glob нашёл win_args: {}",
            argv[2]
        );

        // Файла нет — честный отказ до спавна.
        let dir3 = tempfile::tempdir().unwrap();
        let mut s3 = spec("s", dir3.path().into());
        s3.loader = Some("forge".into());
        s3.loader_version = Some("47.2.0".into());
        let err = build_command(Path::new("java"), &s3).unwrap_err();
        assert!(err.to_string().contains("не установлено"), "{err}");
    }

    /// NeoForge: win_args по своей maven-ветке, имя каталога = версия лоадера.
    #[test]
    fn build_command_neoforge() {
        let dir = tempfile::tempdir().unwrap();
        let neo = dir.path().join("libraries/net/neoforged/neoforge/20.4.237");
        std::fs::create_dir_all(&neo).unwrap();
        std::fs::write(neo.join("win_args.txt"), b"@a").unwrap();
        let mut s = spec("s", dir.path().into());
        s.loader = Some("neoforge".into());
        s.loader_version = Some("20.4.237".into());
        let argv = build_command(Path::new("java"), &s).unwrap();
        let arg = argv[2].strip_prefix('@').expect("@win_args");
        assert_eq!(Path::new(arg), neo.join("win_args.txt").as_path());
        assert_eq!(argv[3], "nogui");
    }

    /// Неизвестный загрузчик и отсутствие jar'а — InvalidInput до спавна.
    #[test]
    fn build_command_rejects_unknown_loader_and_missing_jar() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = spec("s", dir.path().into());
        s.loader = Some("quilt".into());
        let err = build_command(Path::new("java"), &s).unwrap_err();
        assert!(err.to_string().contains("неизвестный загрузчик"), "{err}");

        // Ваниль без server.jar (каталог пуст).
        let err = build_command(Path::new("java"), &spec("s", dir.path().into())).unwrap_err();
        assert!(err.to_string().contains("не установлено"), "{err}");
    }

    /// EULA: нет файла / eula=false — отказ; «eula=true», «eula = TRUE»,
    /// комментарии вокруг — принята (Minecraft читает properties).
    #[test]
    fn eula_rules() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!eula_accepted(dir.path()), "нет eula.txt");

        std::fs::write(dir.path().join("eula.txt"), b"#Generated\neula=false\n").unwrap();
        assert!(!eula_accepted(dir.path()), "eula=false — отказ");

        std::fs::write(dir.path().join("eula.txt"), b"#By changing the setting below to TRUE you are indicating your agreement\neula=true\n").unwrap();
        assert!(eula_accepted(dir.path()));

        std::fs::write(dir.path().join("eula.txt"), b"eula = TRUE\n").unwrap();
        assert!(eula_accepted(dir.path()), "регистр и пробелы не важны");
    }

    /// Занятый порт: слушатель на 0.0.0.0:0 держится тестом, server_start
    /// обязан отказать «порт занят» ещё до java (java-спавна в тесте нет).
    #[test]
    fn start_rejects_busy_port() {
        let listener = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let (_d, paths) = test_paths();
        let dir = server_dir(&paths, "busy-port");
        std::fs::create_dir_all(crate::util::fs::long_path(&dir)).unwrap();
        std::fs::write(dir.join("eula.txt"), b"eula=true\n").unwrap();

        let bus = crate::events::EventBus::new(16);
        let err = server_start(&paths, &bus, spec("busy-port", dir), port).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("порт") && text.contains("занят"), "{text}");
    }

    /// Уже запущен: живой лок (свой PID) блокирует второй старт до java-спавна.
    #[test]
    fn start_rejects_already_running() {
        let (_d, paths) = test_paths();
        let id = "already";
        let dir = server_dir(&paths, id);
        std::fs::create_dir_all(crate::util::fs::long_path(&dir)).unwrap();
        std::fs::write(dir.join("eula.txt"), b"eula=true\n").unwrap();

        let my = std::process::id();
        let lock = ServerLock {
            pid: my,
            start_time: crate::instances::process_start_time(my).unwrap(),
            started_at_ms: unix_ms(),
        };
        crate::util::fs::atomic_write(
            &crate::util::fs::long_path(&lock_path(&paths, id)),
            lock.to_line().as_bytes(),
        )
        .unwrap();

        let bus = crate::events::EventBus::new(16);
        // Порт 0: bind-проба всегда удаётся, проверка порта мимо — отказ именно
        // из-за «уже запущен».
        let err = server_start(&paths, &bus, spec(id, dir.clone()), 0).unwrap_err();
        assert!(err.to_string().contains("уже запущен"), "{err}");
    }

    /// Нет каталога и нет EULA — отказ до java-спавна (тексты честные).
    #[test]
    fn start_rejects_missing_dir_and_eula() {
        let (_d, paths) = test_paths();
        let bus = crate::events::EventBus::new(16);
        let ghost = server_dir(&paths, "ghost");
        let err = server_start(&paths, &bus, spec("ghost", ghost), 0).unwrap_err();
        assert!(err.to_string().contains("каталог"), "{err}");

        let dir = server_dir(&paths, "no-eula");
        std::fs::create_dir_all(crate::util::fs::long_path(&dir)).unwrap();
        let err = server_start(&paths, &bus, spec("no-eula", dir), 0).unwrap_err();
        assert!(err.to_string().contains("EULA"), "{err}");
    }

    /// Контракт события: tag «server_event», camelCase-поля; line только у log,
    /// code только у exited/crashed.
    #[test]
    fn server_event_json_contract() {
        let log = ServerEvent::ServerEvent {
            server_id: "s1".into(),
            kind: ServerEventKind::Log,
            line: Some("Done (12.5s)!\nline2".into()),
            code: None,
        };
        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("\"event\":\"server_event\""), "{json}");
        assert!(json.contains("\"serverId\":\"s1\""), "{json}");
        assert!(json.contains("\"kind\":\"log\""), "{json}");
        assert!(json.contains("\"line\":\"Done (12.5s)!\\nline2\""), "{json}");
        assert!(!json.contains("code"), "{json}");

        let started = ServerEvent::ServerEvent {
            server_id: "s1".into(),
            kind: ServerEventKind::Started,
            line: None,
            code: None,
        };
        let json = serde_json::to_string(&started).unwrap();
        assert!(json.contains("\"kind\":\"started\""), "{json}");
        assert!(!json.contains("line"), "{json}");

        let exited = ServerEvent::ServerEvent {
            server_id: "s1".into(),
            kind: ServerEventKind::Exited,
            line: None,
            code: Some(0),
        };
        let json = serde_json::to_string(&exited).unwrap();
        assert!(json.contains("\"kind\":\"exited\""), "{json}");
        assert!(json.contains("\"code\":0"), "{json}");

        let crashed = ServerEvent::ServerEvent {
            server_id: "s1".into(),
            kind: ServerEventKind::Crashed,
            line: None,
            code: Some(-1),
        };
        let json = serde_json::to_string(&crashed).unwrap();
        assert!(json.contains("\"kind\":\"crashed\""), "{json}");
        assert!(json.contains("\"code\":-1"), "{json}");
    }

    /// Пачка: 50 строк — готова, take склеивает `\n` и сбрасывает; пустая
    /// пачка — None.
    #[test]
    fn log_batch_lines_and_join() {
        let mut b = LogBatch::new();
        assert_eq!(b.take(), None, "пустая пачка");
        for i in 0..LogBatch::MAX_LINES {
            b.push(format!("line-{i}"));
        }
        assert!(b.ready(), "MAX_LINES строк — готово");
        let blob = b.take().unwrap();
        assert_eq!(blob.lines().count(), LogBatch::MAX_LINES);
        assert!(blob.contains("line-0") && blob.contains("line-49"));
        assert!(!blob.contains("line-50"));
        assert_eq!(b.take(), None, "после take пачка пуста");
        assert!(!b.ready(), "таймер сброшен take'ом");
    }

    /// Пачка зреет по времени (200 мс) даже без 50 строк.
    #[test]
    fn log_batch_matures_by_time() {
        let mut b = LogBatch::new();
        b.push("only-one".into());
        assert!(!b.ready(), "меньше 50 строк и 200 мс не прошло");
        std::thread::sleep(LogBatch::MAX_DELAY + std::time::Duration::from_millis(50));
        assert!(b.matured(), "время вышло — пачка созрела");
        assert_eq!(b.take_if_matured().as_deref(), Some("only-one"));
    }

    /// A15-семантика: секрет из реестра не уходит в событие лога сервера.
    #[test]
    fn log_batch_redacts_secrets() {
        let secret = "SERVERLOG-SECRET-9f3";
        crate::util::redact::register_secret(secret);
        let mut b = LogBatch::new();
        b.push(format!("token {secret} end"));
        let blob = b.take().unwrap();
        assert!(!blob.contains(secret), "секрет утёк: {blob}");
        assert!(blob.contains("[REDACTED]"), "{blob}");
    }

    /// Декод строк потока: не-UTF-8 → U+FFFD (поток не обрывается),
    /// хвостовой \r от CRLF отрезается.
    #[test]
    fn lossy_line_decodes_and_trims() {
        assert_eq!(lossy_line(b"plain"), "plain");
        assert_eq!(lossy_line(b"ok\r"), "ok");
        assert_eq!(lossy_line(b""), "");
        let bad = vec![b'a', b'd', 0xFF, b'x'];
        let decoded = lossy_line(&bad);
        assert!(decoded.contains('\u{FFFD}'), "{decoded:?}");
    }

    /// Хвост лога при выходе по тику: строка, поданная в канал ПОСЛЕ детекта
    /// выхода, дожимается финализацией (дренаж до дедлайна), затем уходит
    /// финальное exited/crashed. Батчер с отложенной подачей — не теряется.
    #[test]
    fn finalize_drains_pending_log_tail() {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let mut sub = subscribe_server_events();

        // «Читатель не успел»: хвост краш-стека приходит в канал с задержкой.
        let late = tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = late.send("late crash frame".to_string());
        });
        drop(tx);

        let mut batch = LogBatch::new();
        // Корень в tempdir: даже если elapsed дотянет до целой секунды,
        // playtime-запись не загрязнит рабочую область теста.
        let root = tempfile::tempdir().unwrap();
        finalize_run(
            rx,
            Vec::new(),
            "drain-test",
            &mut batch,
            Some(1),
            std::time::Instant::now(), // elapsed ~0 сек — записи не будет
            root.path(),
        );

        // Дошли и хвост (log), и финал (crashed с кодом 1).
        let mut saw_tail = false;
        let mut saw_crashed = false;
        while let Ok(ev) = sub.try_recv() {
            let ServerEvent::ServerEvent {
                server_id,
                kind,
                line,
                code,
            } = ev;
            if server_id != "drain-test" {
                continue;
            }
            match kind {
                ServerEventKind::Log => {
                    saw_tail = true;
                    assert!(
                        line.unwrap_or_default().contains("late crash frame"),
                        "хвост лога потерян"
                    );
                }
                ServerEventKind::Crashed => {
                    saw_crashed = true;
                    assert_eq!(code, Some(1));
                }
                _ => {}
            }
        }
        assert!(saw_tail, "событие log с хвостом не дошло");
        assert!(saw_crashed, "финальное событие crashed не дошло");
    }

    /// Читатели: закрывшийся поток присоединяется мгновенно; застрявший
    /// (наследник держит пайп) отсоединяется по истечении дедлайна —
    /// супервизор не виснет.
    #[test]
    fn join_reader_finishes_and_times_out() {
        // Успешный путь: поток уже закончился — join без задержки.
        let done = std::thread::spawn(|| {});
        assert!(join_reader(
            done,
            std::time::Instant::now() + FINALIZE_BUDGET
        ));

        // Застрявший поток и дедлайн в прошлом — мгновенный отказ без join.
        let stuck = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(30));
        });
        let t0 = std::time::Instant::now();
        assert!(!join_reader(stuck, t0), "застрявший поток не присоединился");
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(500),
            "отказ по дедлайну затянулся: {:?}",
            t0.elapsed()
        );
        // Отсоединённый поток доспит сам; процесс теста его не ждёт.
    }

    /// server_stop: не запущен — честное false, ничего не падает.
    #[test]
    fn stop_not_running_is_false() {
        let (_d, paths) = test_paths();
        assert!(!server_stop(&paths, "ghost").unwrap(), "не запущен — false");
        // Некорректный id — ошибка валидации, а не паника.
        assert!(server_stop(&paths, "../evil").is_err());
    }
}
