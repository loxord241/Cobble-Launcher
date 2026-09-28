//! Discord Rich Presence без инъекций (F26, D37-C). Агент C5.
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

/// opcode HANDSHAKE — первый кадр после открытия pipe.
pub const OP_HANDSHAKE: u32 = 0;
/// opcode FRAME — обычные запросы (`SET_ACTIVITY`) и события (`READY`).
pub const OP_FRAME: u32 = 1;

/// Application ID Discord-приложения лаунчера — ЗАГЛУШКА-ПРИМЕР.
/// Шаг владельца (как ключи подписи): создать приложение на
/// https://discord.com/developers/applications и подставить сюда настоящий
/// Application ID. С чужим client_id Discord установит соединение, но presence
/// молча игнорирует — это не ошибка лаунчера.
pub const DISCORD_CLIENT_ID: &str = "1554156161027014666";

/// Живое соединение с локальным IPC Discord: пишет presence, молча умирает
/// при закрытии pipe (вызывающая сторона просто перестаёт обновлять/выбрасывает).
pub struct RpcConnection {
    pipe: File,
    pid: u32,
    /// Время старта сессии (unix, мс) — `timestamps.start` для всех статусов.
    start_ms: u64,
    /// Счётчик nonce: уникальный id кадра `{pid}-{n}` (uuid не нужен).
    nonce_counter: AtomicU64,
}

impl RpcConnection {
    /// Подключиться к локальному IPC Discord (handshake).
    /// Discord не запущен / pipe не найден → `Ok(None)` (это не ошибка!).
    pub fn connect(pid: u32, client_id: &str) -> std::io::Result<Option<Self>> {
        let Some(pipe) = open_ipc_pipe()? else {
            return Ok(None);
        };
        let mut conn = Self {
            pipe,
            pid,
            start_ms: unix_ms(),
            nonce_counter: AtomicU64::new(0),
        };
        conn.write_frame(OP_HANDSHAKE, &build_handshake_payload(client_id).to_string())?;
        // Discord отвечает READY (evt) — прочитать, чтобы кадры не сдвигались.
        let mut header = [0u8; 8];
        conn.pipe.read_exact(&mut header)?;
        let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let mut body = vec![0u8; len.min(4096)];
        conn.pipe.read_exact(&mut body)?;
        tracing::info!(
            "discord rpc: {}",
            String::from_utf8_lossy(&body).split("\"evt\"").nth(1).unwrap_or("READY").trim_start_matches(':').trim_end_matches('}')
        );
        Ok(Some(conn))
    }

    /// Обновить статус (details = название инстанса).
    /// Ошибки (pipe закрыт и т.п.) логируются в warn; `Err` означает, что
    /// соединение умерло — его можно молча выбросить (presence Discord
    /// уберёт сам при закрытии pipe).
    pub fn set_activity(&mut self, details: &str) -> std::io::Result<()> {
        let n = self.nonce_counter.fetch_add(1, Ordering::Relaxed);
        let nonce = format!("{}-{n}", self.pid);
        let payload =
            build_set_activity_payload(self.pid, details, self.start_ms, &nonce).to_string();
        if let Err(e) = self.write_frame(OP_FRAME, &payload) {
            tracing::warn!("discord rpc: presence не обновлён: {e}");
            return Err(e);
        }
        // Ответ Discord (opcode + данные presence): читаем для диагностики —
        // если Discord отверг кадр, увидим причину в логе вместо тишины.
        let mut header = [0u8; 8];
        self.pipe.read_exact(&mut header)?;
        let reply_len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let mut body = vec![0u8; reply_len.min(4096)];
        self.pipe.read_exact(&mut body)?;
        tracing::info!(
            "discord rpc: ответ (opcode {}, {} байт): {}",
            u32::from_le_bytes([header[0], header[1], header[2], header[3]]),
            reply_len,
            String::from_utf8_lossy(&body)
        );
        Ok(())
    }

    /// Закрыть соединение (pipe закроется в Drop). Отдельного «сбросить
    /// статус» не требуется — Discord сам убирает presence при закрытии pipe.
    pub fn close(self) {}
}

impl RpcConnection {
    /// Записать один кадр протокола в pipe.
    fn write_frame(&self, opcode: u32, payload: &str) -> std::io::Result<()> {
        let frame = build_frame(opcode, payload);
        let mut pipe = &self.pipe;
        pipe.write_all(&frame)
    }
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
}
