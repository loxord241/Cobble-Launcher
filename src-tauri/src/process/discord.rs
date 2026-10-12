//! Discord Rich Presence без инъекций (F26, D37-C).
//!
//! Протокол (упрощённый официальный discord-rpc): кадр = 4 байта LE opcode +
//! 4 байта LE длина тела + JSON-тело.
//!   opcode 0 — HANDSHAKE: `{"v":1,"client_id":"..."}`, первый кадр после
//!     открытия pipe;
//!   opcode 1 — FRAME: запросы/события; нам нужен только `SET_ACTIVITY`
//!     (ответы Discord — READY и т.п. — не читаем, для отправки не требуются).
//!
//! Транспорт: именованный pipe Windows `\\.\pipe\discord-ipc-0..9`, открываем
//! вручную через `std::os::windows::fs::OpenOptions` (новых крейтов нет).
//!
//! Кроссплатформенность: на не-Windows [`RpcConnection::connect`] всегда
//! возвращает `Ok(None)` — RPC считается недоступным, это не ошибка.
//!
//! [`DISCORD_CLIENT_ID`] — Application ID приложения «Cobble Launcher» из
//! Discord Developer Portal (владелец предоставил 2026-09-28).

use std::fs::File;
use std::io::{Read as _, Write as _};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// opcode HANDSHAKE — первый кадр после открытия pipe.
pub const OP_HANDSHAKE: u32 = 0;
/// opcode FRAME — обычные запросы (`SET_ACTIVITY`) и события (`READY`).
pub const OP_FRAME: u32 = 1;

/// D72: таймаут одной операции pipe (чтения/записи), 2 с. `std::fs::File` не
/// умеет `set_read_timeout`/`set_write_timeout` (они есть только у сокетов),
/// а новых крейтов нет ([`with_pipe_timeout`]): блокирующая операция уходит в
/// короткоживущий поток, результат ждём через канал не дольше потолка.
/// Раньше зависший Discord (не отвечает, но pipe открыт) блокировал поток
/// RPC навечно.
const PIPE_IO_TIMEOUT: Duration = Duration::from_secs(2);

/// D72: пауза между попытками переподключения к pipe Discord. Если Discord
/// закрыли, попытка reconnect на каждый `set_activity` (раз в минуту) — это
/// перебор pipe'ов `discord-ipc-0..9`; при живом, но «зависшем» Discord так
/// и вовсе спам открытых соединений. Ленивый reconnect: не чаще раза в 30 с.
const RECONNECT_PAUSE: Duration = Duration::from_secs(30);

/// Application ID Discord-приложения лаунчера — ЗАГЛУШКА-ПРИМЕР.
/// Шаг владельца (как ключи подписи): создать приложение на
/// https://discord.com/developers/applications и подставить сюда настоящий
/// Application ID. С чужим client_id Discord установит соединение, но presence
/// молча игнорирует — это не ошибка лаунчера.
pub const DISCORD_CLIENT_ID: &str = "1554156161027014666";

/// Живое соединение с локальным IPC Discord: пишет presence, переживает смерть
/// pipe ленивым переподключением (D72). D72: смерть соединения (ошибка
/// записи/чтения, таймаут 2 с) переводит его в «мёртвое» состояние
/// ([`RpcConnection::is_dead`]); следующий `set_activity` позже чем через
/// [`RECONNECT_PAUSE`] пересоздаёт pipe заново (handshake + READY).
pub struct RpcConnection {
    /// D72: `Some` — живой pipe; `None` — соединение помечено мёртвым
    /// (ждёт ленивого reconnect). `File` в `Drop` закрывает хэндл — Discord
    /// сам убирает presence при закрытии pipe.
    pipe: Option<File>,
    pid: u32,
    /// D72: client_id сохраняется — reconnect повторяет handshake с ним же.
    client_id: String,
    /// Время старта сессии (unix, мс) — `timestamps.start` для всех статусов.
    /// При reconnect НЕ пересоздаётся: таймер «играет с …» не сбрасывается.
    start_ms: u64,
    /// Счётчик nonce: уникальный id кадра `{pid}-{n}` (uuid не нужен).
    nonce_counter: AtomicU64,
    /// D72: момент последнего сбоя (свой или неудачного connect) — до истечения
    /// [`RECONNECT_PAUSE`] reconnect не предпринимается.
    dead_since: Option<Instant>,
}

impl RpcConnection {
    /// Подключиться к локальному IPC Discord (handshake).
    /// Discord не запущен / pipe не найден → `Ok(None)` (это не ошибка!).
    pub fn connect(pid: u32, client_id: &str) -> std::io::Result<Option<Self>> {
        let Some(pipe) = open_ipc_pipe()? else {
            return Ok(None);
        };
        let conn = Self {
            pipe: Some(pipe),
            pid,
            client_id: client_id.to_string(),
            start_ms: unix_ms(),
            nonce_counter: AtomicU64::new(0),
            dead_since: None,
        };
        let payload = build_handshake_payload(client_id).to_string();
        conn.write_frame(OP_HANDSHAKE, &payload)?;
        // Discord отвечает READY (evt) — прочитать, чтобы кадры не сдвигались
        // (D72: чтение с таймаутом 2 с — зависший Discord не подвешивает connect).
        let (_, body) = conn.read_reply()?;
        tracing::info!(
            "discord rpc: {}",
            String::from_utf8_lossy(&body).split("\"evt\"").nth(1).unwrap_or("READY").trim_start_matches(':').trim_end_matches('}')
        );
        Ok(Some(conn))
    }

    /// D72: соединение помечено мёртвым (сбой записи/чтения, ожидает reconnect)?
    pub fn is_dead(&self) -> bool {
        self.pipe.is_none()
    }

    /// Обновить статус (details = название инстанса).
    /// D72: при ошибке записи/чтения соединение помечается мёртвым
    /// ([`RpcConnection::mark_dead`]), а следующий вызов (цикл в run.rs зовёт
    /// раз в минуту) после паузы [`RECONNECT_PAUSE`] молча пересоздаёт
    /// соединение — присутствие Discord восстанавливается без правок run.rs.
    /// Ошибки логируются в warn; `Err` в паузе reconnect возвращается тихо.
    pub fn set_activity(&mut self, details: &str) -> std::io::Result<()> {
        // Мёртвое соединение: ленивый reconnect, не чаще раза в RECONNECT_PAUSE.
        if self.pipe.is_none() {
            let cooling = self
                .dead_since
                .is_some_and(|t| t.elapsed() < RECONNECT_PAUSE);
            if cooling {
                // Тихий отказ: спамить warn раз в минуту незачем.
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "discord rpc: соединение мертво, reconnect не раньше 30 с после сбоя",
                ));
            }
            self.reconnect()?;
        }
        let result = self.send_activity(details);
        match &result {
            Ok(()) => self.dead_since = None,
            Err(e) => {
                tracing::warn!("discord rpc: presence не обновлён, соединение помечено мёртвым: {e}");
                self.mark_dead();
            }
        }
        result
    }

    /// Закрыть соединение. D72: тело пустое — всю работу делает `Drop` (закрытие
    /// хэндла pipe; Discord сам убирает presence). Метод оставлен для
    /// совместимости вызывающих (run.rs).
    pub fn close(self) {}

    /// D72: пометить соединение мёртвым: хэндл pipe закрывается немедленно
    /// (pipe = None → `Drop` для `File`), Discord убирает presence; время
    /// сбоя запоминается для паузы reconnect.
    fn mark_dead(&mut self) {
        self.pipe = None;
        self.dead_since = Some(Instant::now());
    }

    /// D72: пересоздать соединение с нуля (pipe + handshake + READY).
    /// Держит инвариант «pipe = None, dead_since = момент попытки» при неудаче.
    fn reconnect(&mut self) -> std::io::Result<()> {
        self.dead_since = Some(Instant::now());
        let pipe = match open_ipc_pipe()? {
            Some(p) => p,
            None => {
                self.pipe = None;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "discord rpc: pipe Discord не найден (приложение закрыто?)",
                ));
            }
        };
        self.pipe = Some(pipe);
        let payload = build_handshake_payload(&self.client_id).to_string();
        let handshake = self
            .write_frame(OP_HANDSHAKE, &payload)
            .and_then(|()| self.read_reply());
        match handshake {
            Ok((_, body)) => {
                tracing::info!(
                    "discord rpc: переподключено, ответ: {}",
                    String::from_utf8_lossy(&body)
                );
                Ok(())
            }
            Err(e) => {
                self.pipe = None; // остаёмся мёртвыми до следующей попытки
                Err(e)
            }
        }
    }

    /// Обновить статус по живому соединению: SET_ACTIVITY + чтение ответа.
    fn send_activity(&mut self, details: &str) -> std::io::Result<()> {
        let n = self.nonce_counter.fetch_add(1, Ordering::Relaxed);
        let nonce = format!("{}-{n}", self.pid);
        let payload =
            build_set_activity_payload(self.pid, details, self.start_ms, &nonce).to_string();
        self.write_frame(OP_FRAME, &payload)?;
        // Ответ Discord (opcode + данные presence): читаем для диагностики —
        // если Discord отверг кадр, увидим причину в логе вместо тишины.
        let (opcode, body) = self.read_reply()?;
        tracing::info!(
            "discord rpc: ответ (opcode {}, {} байт): {}",
            opcode,
            body.len(),
            String::from_utf8_lossy(&body)
        );
        Ok(())
    }

    /// D72: записать один кадр протокола в pipe (с таймаутом 2 с). Запись идёт
    /// через дубликат хэндла (`try_clone`): при таймауте основной хэндл
    /// остаётся в структуре и закрывается в `mark_dead`, застрявший поток
    /// умрёт вместе с pipe.
    fn write_frame(&self, opcode: u32, payload: &str) -> std::io::Result<()> {
        let file = self
            .pipe
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "pipe мёртв"))?;
        let mut handle = file.try_clone()?;
        let frame = build_frame(opcode, payload);
        with_pipe_timeout(move || handle.write_all(&frame))
    }

    /// D72: прочитать ответ (заголовок 8 байт + тело) с таймаутом 2 с —
    /// тем же приёмом, что и запись (см. [`RpcConnection::write_frame`]).
    fn read_reply(&self) -> std::io::Result<(u32, Vec<u8>)> {
        let file = self
            .pipe
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "pipe мёртв"))?;
        let mut handle = file.try_clone()?;
        with_pipe_timeout(move || {
            let mut header = [0u8; 8];
            handle.read_exact(&mut header)?;
            let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
            let body = read_frame_body(&mut handle, len)?;
            let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
            Ok((opcode, body))
        })
    }
}

impl Drop for RpcConnection {
    fn drop(&mut self) {
        // D72: корректное закрытие при любом сценарии (close(), сбой, выход
        // потока): сброс Option закрывает хэндл pipe, Discord сам убирает
        // presence при закрытии pipe.
        self.pipe = None;
    }
}

/// D72: выполнить блокирующую pipe-операцию с потолком времени (замена
/// отсутствующих у `File` set_read_timeout/set_write_timeout). На таймауте
/// поток с операцией остаётся заблокированным до реальной смерти pipe —
/// это осознанный компромисс: соединение уже помечено мёртвым и пересоздаётся
/// с нуля, а застрявший дубликат хэндла освободится вместе с pipe.
fn with_pipe_timeout<T: Send + 'static>(
    op: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> std::io::Result<T> {
    with_pipe_timeout_dur(PIPE_IO_TIMEOUT, op)
}

/// Вариант с явным таймаутом (параметр — для тестов; боевой — 2 с).
fn with_pipe_timeout_dur<T: Send + 'static>(
    timeout: Duration,
    op: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> std::io::Result<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // При панике в op send не выполнится — Disconnected трактуем как ошибку.
        let _ = tx.send(op());
    });
    match rx.recv_timeout(timeout) {
        Ok(res) => res,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("discord rpc: pipe не ответил за {} мс", timeout.as_millis()),
        )),
        Err(_) => Err(std::io::Error::other(
            "discord rpc: поток pipe завершился аварийно",
        )),
    }
}

/// Потолок тела кадра: настоящие кадры Discord — единицы КБ, а транспорт —
/// блокирующий pipe (аудит 2026-10-06). Раньше `len.min(4096)` обрезал
/// длинный кадр, и остаток тела попадал в следующий заголовок — рассинхрон
/// потока; теперь длина сверх потолка → честная ошибка, соединение рвётся.
const MAX_FRAME_BODY: usize = 64 * 1024;

/// Прочитать тело кадра РОВНО `len` байт после заголовка. `len` сверх
/// [`MAX_FRAME_BODY`] — ошибка (читать такой кадр и дренажем, и частями
/// нельзя: pipe без таймаутов, огромный len — вечное чтение).
fn read_frame_body<R: std::io::Read>(pipe: &mut R, len: usize) -> std::io::Result<Vec<u8>> {
    if len > MAX_FRAME_BODY {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("кадр Discord слишком большой: {len} байт (лимит {MAX_FRAME_BODY})"),
        ));
    }
    let mut body = vec![0u8; len];
    pipe.read_exact(&mut body)?;
    Ok(body)
}

/// Открыть pipe Discord; нет ни одного → `Ok(None)`.
#[cfg(windows)]
fn open_ipc_pipe() -> std::io::Result<Option<File>> {
    use std::fs::OpenOptions;
    // Discord создаёт до 10 pipe'ов: discord-ipc-0..9. Перебираем по порядку;
    // ни одного не нашли — Discord не запущен → Ok(None) (не ошибка).
    for idx in 0..10u32 {
        let path = format!(r"\\.\pipe\discord-ipc-{idx}");
        match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(file) => return Ok(Some(file)),
            // Нет такого pipe — обычный случай (этот индекс не занят).
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            // Прочее (занят/нет доступа) — тоже считаем индекс недоступным.
            Err(e) => {
                tracing::warn!("discord rpc: pipe {path} недоступен: {e}");
                continue;
            }
        }
    }
    Ok(None)
}

/// На не-Windows Discord RPC недоступен — всегда `Ok(None)` (см. доки модуля).
#[cfg(not(windows))]
fn open_ipc_pipe() -> std::io::Result<Option<File>> {
    Ok(None)
}

/// Кадр протокола: 4 байта LE opcode + 4 байта LE длина тела + тело.
fn build_frame(opcode: u32, payload: &str) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8 + payload.len());
    frame.extend_from_slice(&opcode.to_le_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload.as_bytes());
    frame
}

/// Тело HANDSHAKE (opcode 0).
fn build_handshake_payload(client_id: &str) -> serde_json::Value {
    serde_json::json!({ "v": 1, "client_id": client_id })
}

/// Тело SET_ACTIVITY (opcode 1): статус с большим артом `minecraft`.
fn build_set_activity_payload(
    pid: u32,
    details: &str,
    start_ms: u64,
    nonce: &str,
) -> serde_json::Value {
    serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": pid,
            "activity": {
                "details": details,
                "assets": { "large_image": "minecraft" },
                "timestamps": { "start": start_ms }
            }
        },
        "nonce": nonce
    })
}

/// Текущее время в миллисекундах Unix; при сбое часов — 0.
fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// u32 из среза без unwrap (в тестах достаточно паники по индексу).
    fn le_u32(b: &[u8]) -> u32 {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }

    /// Кадр: opcode и длина тела — little-endian, тело идёт следом без изменений.
    #[test]
    fn build_frame_le_opcode_and_length() {
        let frame = build_frame(OP_FRAME, "{}");
        assert_eq!(frame.len(), 10, "8 байт заголовка + 2 байта тела");
        assert_eq!(le_u32(&frame[0..4]), 1, "opcode 1 little-endian");
        assert_eq!(le_u32(&frame[4..8]), 2, "длина тела little-endian");
        assert_eq!(&frame[8..], b"{}", "тело без изменений");
    }

    /// Пустое тело — заголовок из 8 байт с нулевой длиной.
    #[test]
    fn build_frame_empty_payload() {
        let frame = build_frame(OP_HANDSHAKE, "");
        assert_eq!(frame.len(), 8, "только заголовок");
        assert_eq!(le_u32(&frame[0..4]), 0, "opcode 0 little-endian");
        assert_eq!(le_u32(&frame[4..8]), 0, "нулевая длина");
    }

    /// Опциональный парсинг: кадр разбирается обратно в (opcode, длина, тело).
    #[test]
    fn frame_roundtrip_parse() {
        let payload = build_set_activity_payload(7, "инстанс", 42, "7-0").to_string();
        let frame = build_frame(OP_FRAME, &payload);
        assert_eq!(le_u32(&frame[0..4]), OP_FRAME);
        assert_eq!(le_u32(&frame[4..8]) as usize, payload.len());
        assert_eq!(
            std::str::from_utf8(&frame[8..]).expect("тело — валидный UTF-8"),
            payload
        );
    }

    /// HANDSHAKE: поля v и client_id.
    #[test]
    fn handshake_payload_fields() {
        let v = build_handshake_payload(DISCORD_CLIENT_ID);
        assert_eq!(v["v"], 1);
        assert_eq!(v["client_id"], "1554156161027014666");
        assert!(v.get("nonce").is_none(), "в handshake нет nonce");
    }

    /// SET_ACTIVITY: cmd, pid, details, арты, таймстамп, nonce.
    #[test]
    fn set_activity_payload_fields() {
        let v = build_set_activity_payload(4242, "Sky Factory 4", 1_700_000_000_000, "4242-0");
        assert_eq!(v["cmd"], "SET_ACTIVITY");
        assert_eq!(v["args"]["pid"], 4242);
        assert_eq!(v["args"]["activity"]["details"], "Sky Factory 4");
        assert_eq!(
            v["args"]["activity"]["assets"]["large_image"],
            "minecraft",
            "большой арт — minecraft"
        );
        assert_eq!(
            v["args"]["activity"]["timestamps"]["start"],
            1_700_000_000_000u64
        );
        assert_eq!(v["nonce"], "4242-0");
    }

    /// Аудит 2026-10-06: тело кадра читается ЦЕЛИКОМ (не обрезается на 4096,
    /// как раньше) — следующий заголовок не попадает на чужой остаток тела.
    #[test]
    fn read_frame_body_reads_full_length() {
        let payload = vec![b'a'; 10_000]; // больше прежних 4096
        let mut src = std::io::Cursor::new(payload.clone());
        let body = read_frame_body(&mut src, payload.len()).unwrap();
        assert_eq!(body, payload, "полная длина, без обрезки");
    }

    /// Длина сверх потолка — ошибка InvalidData до всякого чтения: кап
    /// защищает и от рассинхрона, и от вечного чтения блокирующего pipe.
    #[test]
    fn read_frame_body_rejects_oversized_length() {
        let mut src = std::io::Cursor::new(vec![0u8; 16]);
        let err = read_frame_body(&mut src, MAX_FRAME_BODY + 1)
            .expect_err("len > 64КБ должен рвать соединение");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData, "{err}");
    }

    /// Граничный случай: ровно потолок читается, нулевая длина — пустое тело.
    #[test]
    fn read_frame_body_boundary_and_empty() {
        let payload = vec![b'x'; MAX_FRAME_BODY];
        let mut src = std::io::Cursor::new(payload.clone());
        assert_eq!(read_frame_body(&mut src, MAX_FRAME_BODY).unwrap(), payload);

        let mut empty = std::io::Cursor::new(Vec::new());
        assert!(read_frame_body(&mut empty, 0).unwrap().is_empty());
    }

    /// Недобор данных (pipe оборвался посреди тела) — UnexpectedEof, как и
    /// раньше read_exact: вызывающий трактует это как смерть соединения.
    #[test]
    fn read_frame_body_truncated_is_unexpected_eof() {
        let mut src = std::io::Cursor::new(vec![0u8; 8]);
        let err = read_frame_body(&mut src, 64).expect_err("тело короче заявленного");
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof, "{err}");
    }

    // ---------- D72: таймауты pipe и семантика reconnect ----------

    /// D72: быстрая операция проходит сквозь таймаут-обёртку без изменений.
    #[test]
    fn pipe_timeout_passes_fast_result() {
        let res = with_pipe_timeout_dur(Duration::from_millis(500), || Ok(42u8));
        assert_eq!(res.unwrap(), 42);
        let err = with_pipe_timeout_dur(Duration::from_millis(500), || {
            Err::<u8, std::io::Error>(std::io::Error::other("boom"))
        })
        .expect_err("ошибка операции доходит как есть");
        assert_eq!(err.kind(), std::io::ErrorKind::Other, "{err}");
    }

    /// D72: зависшая операция (вечное чтение pipe без ответа) обрывается
    /// TimedOut по потолку, а не блокирует навечно.
    #[test]
    fn pipe_timeout_cuts_hanging_operation() {
        let started = Instant::now();
        let err = with_pipe_timeout_dur(Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_millis(400));
            Ok(())
        })
        .expect_err("зависшая операция обязана упасть по таймауту");
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut, "{err}");
        assert!(
            started.elapsed() < Duration::from_millis(300),
            "обрыв по таймауту, а не ожидание операции: {:?}",
            started.elapsed()
        );
    }

    /// D72: мёртвое соединение (pipe = None) в паузе reconnect отвечает
    /// тихим NotConnected, не предпринимая попыток (в тесте нет Discord —
    /// попытка reconnect была бы видна как NotFound, а не NotConnected).
    #[test]
    fn dead_connection_during_pause_returns_not_connected() {
        let mut conn = RpcConnection {
            pipe: None,
            pid: 1,
            client_id: DISCORD_CLIENT_ID.into(),
            start_ms: 0,
            nonce_counter: AtomicU64::new(0),
            dead_since: Some(Instant::now()),
        };
        assert!(conn.is_dead());
        let err = conn
            .set_activity("инстанс")
            .expect_err("в паузе reconnect — тихий отказ");
        assert_eq!(err.kind(), std::io::ErrorKind::NotConnected, "{err}");
        assert!(conn.is_dead(), "состояние не изменилось за паузу");
    }

    /// D72: константы валидации: пауза reconnect не короче 30 с (спека),
    /// таймаут pipe — ровно 2 с.
    #[test]
    fn reconnect_pause_and_pipe_timeout_constants() {
        assert!(RECONNECT_PAUSE >= Duration::from_secs(30), "спека D72");
        assert_eq!(PIPE_IO_TIMEOUT, Duration::from_secs(2));
    }
}
