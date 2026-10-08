//! Наигранные часы по дням (история активности инстансов). Хранение —
//! `playtime.json` в каталоге данных (рядом с settings.json):
//! `{"<instanceId>": {"2026-10-08": 3600, ...}}`; храним последние 90 дней,
//! при каждой записи более старые дни подчищаются. Запись атомарная
//! (`util::fs::atomic_write`: tmp + rename).
//!
//! В зависимостях нет chrono — даты считаем из unix-времени сами
//! (гражданский календарь Хиннанта); локальная дата на Windows — Win32
//! `GetLocalTime` (учитывает часовой пояс и переход на летнее время системы).

use crate::errors::Result;
use crate::util::fs::atomic_write;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Имя файла в каталоге данных.
const FILE_NAME: &str = "playtime.json";

/// Окно хранения: последние 90 дней (более старые дневные ключи удаляются при записи).
const KEEP_DAYS: u32 = 90;

/// Кламп одной сессии — 24 ч. `Instant` на Windows (QPC) тикает и во время сна
/// машины: ноутбук, уснувший с запущенной игрой, надул бы день гигантским
/// значением. Сутки — верхняя граница здравого смысла для одной сессии.
const MAX_SESSION_SECS: u64 = 24 * 60 * 60;

/// Сериализация read-modify-write в рамках процесса (запись идёт и из
/// супервизора игры, и из будущего IPC).
static PLAYTIME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Один день истории: дата `YYYY-MM-DD` и наигранные секунды.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlaytimeDay {
    pub date: String,
    pub secs: u64,
}

/// `<instanceId> -> <дата -> секунды>`. BTreeMap — стабильный порядок ключей в файле.
type Store = BTreeMap<String, BTreeMap<String, u64>>;

fn default_dir() -> PathBuf {
    crate::paths::Paths::default_root()
}

fn file_path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// Битый playtime.json уехать в `.bad`: иначе первая же запись сессии одного
/// инстанса молча затёрла бы историю всех остальных (лёгкий карантин, как у
/// settings.json).
fn quarantine_broken(path: &Path, reason: &str) {
    let mut bad = path.as_os_str().to_os_string();
    bad.push(".bad");
    match crate::util::fs::rename_with_retry(path, Path::new(&bad)) {
        Ok(()) => tracing::warn!("playtime.json невалиден ({reason}) — перемещён в {}", bad.display()),
        Err(e) => tracing::warn!("playtime.json невалиден ({reason}), карантин не удался: {e}"),
    }
}

/// Читать хранилище; файла нет → пустое, битый → карантин + пустое.
fn load(dir: &Path) -> Store {
    let path = file_path(dir);
    let long = crate::util::fs::long_path(&path);
    if !long.exists() {
        return Store::new();
    }
    match std::fs::read(&long) {
        Ok(data) => match serde_json::from_slice(&data) {
            Ok(store) => store,
            Err(e) => {
                quarantine_broken(&path, &e.to_string());
                Store::new()
            }
        },
        Err(e) => {
            tracing::warn!("playtime.json не прочитан: {e}");
            Store::new()
        }
    }
}

/// Атомарно записать (tmp + rename, как settings.json).
fn save(dir: &Path, store: &Store) -> Result<()> {
    let data = serde_json::to_vec_pretty(store)?;
    atomic_write(&file_path(dir), &data)
}

// --- Даты без chrono: гражданский календарь (Howard Hinnant, chrono/C++20). ---

/// Дни от эпохи (1970-01-01 = 0) → (год, месяц, день) по UTC.
fn days_to_civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (год, месяц, день) → дни от эпохи; обратная к days_to_civil.
fn civil_to_days(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as u64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe as i64 - 719_468
}

/// Всегда `YYYY-MM-DD` (нулями до разряда).
fn format_ymd(y: i64, m: u32, d: u32) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM-DD` → дни от эпохи; None для мусора (чужие/битые ключи файла).
fn parse_date(s: &str) -> Option<i64> {
    let mut it = s.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(civil_to_days(y, m, d))
}

/// Unix-секунды → UTC-дата (fallback локальной даты на не-Windows).
#[cfg(not(windows))]
fn utc_date_str(unix_secs: u64) -> String {
    let (y, m, d) = days_to_civil((unix_secs / 86_400) as i64);
    format_ymd(y, m, d)
}

/// Текущий день в «днях от эпохи» по UTC (fallback опорной даты, если
/// переданная не парсится — на практике недостижимо).
fn utc_today_days() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (now / 86_400) as i64
}

/// Локальная дата (YYYY-MM-DD) на момент вызова. Windows — `GetLocalTime`
/// (стенное время системы, TZ/DST учитывает сама Win32, без новых зависимостей);
/// прочие ОС (сборка туда не целится) — UTC-дата из unix-времени.
#[cfg(windows)]
pub(crate) fn local_today() -> String {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(lpSystemTime: *mut SystemTime);
    }
    let mut st = SystemTime::default();
    unsafe { GetLocalTime(&mut st) };
    format_ymd(st.year as i64, st.month as u32, st.day as u32)
}

#[cfg(not(windows))]
pub(crate) fn local_today() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    utc_date_str(now)
}

// --- Хранение. ---

/// Удалить ключи старше окна хранения и непарсящиеся (мусор из рук редактора).
/// Сравнение — в «днях от эпохи», чтобы окно не зависело от сортировки строк.
fn trim_days(days: &mut BTreeMap<String, u64>, today_days: i64) {
    let cutoff = today_days - (KEEP_DAYS as i64 - 1);
    days.retain(|k, _| parse_date(k).is_some_and(|d| d >= cutoff));
}

/// Записать сессию в указанный каталог (параметризация для тестов/IPC).
/// `date` — локальная дата записи (`YYYY-MM-DD`).
pub fn record_in_dir(dir: &Path, instance_id: &str, secs: u64) -> Result<()> {
    let date = local_today();
    let _guard = PLAYTIME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    record_on(dir, instance_id, &date, secs)
}

/// Ядро записи с явной датой — так тесты детерминированы без подмены часов.
fn record_on(dir: &Path, instance_id: &str, date: &str, secs: u64) -> Result<()> {
    // Нулевая сессия (мгновенный вылет до супервизии) — нечего ни копить, ни писать.
    if instance_id.is_empty() || secs == 0 {
        return Ok(());
    }
    // secs > 24 ч — кламп (см. MAX_SESSION_SECS: сон машины раздувает Instant);
    // secs < 0 невозможен по типу u64 (и_elapsed() монотонный) — защита от
    // «бреда» уже на границе сигнатуры.
    let secs = secs.min(MAX_SESSION_SECS);
    let Some(today_days) = parse_date(date) else {
        return Ok(()); // непарсящаяся дата — не пишем мусорный ключ
    };
    let mut store = load(dir);
    let days = store.entry(instance_id.to_string()).or_default();
    // Сессии одного дня суммируются.
    *days.entry(date.to_string()).or_insert(0) += secs;
    for days in store.values_mut() {
        trim_days(days, today_days);
    }
    save(dir, &store)
}

/// История по указанному каталогу (параметризация для тестов/IPC).
/// `today` — локальная дата `YYYY-MM-DD`, относительно которой считаем окно.
pub fn stats_in_dir(dir: &Path, instance_id: &str, days: u32, today: &str) -> Vec<PlaytimeDay> {
    if days == 0 {
        return Vec::new();
    }
    // Старше 90 дней ничего не хранится — больше и не возвращаем.
    let days = days.min(KEEP_DAYS) as i64;
    let _guard = PLAYTIME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let store = load(dir);
    let empty = BTreeMap::new();
    let played = store.get(instance_id).unwrap_or(&empty);
    // Опорный день: today; если нет — текущая UTC-дата (недостижимо на практике).
    let today_days = parse_date(today).unwrap_or_else(utc_today_days);
    // От старых к свежим — слева направо для графика; дни без игры — нули.
    (0..days)
        .rev()
        .map(|offset| {
            let (y, m, d) = days_to_civil(today_days - offset);
            let date = format_ymd(y, m, d);
            let secs = played.get(&date).copied().unwrap_or(0);
            PlaytimeDay { date, secs }
        })
        .collect()
}

/// История наигранных секунд инстанса за последние `days` дней, включая
/// нулевые; от старых к свежим. Дней больше 90 не бывает (окно хранения).
pub fn playtime_stats(instance_id: &str, days: u32) -> Vec<PlaytimeDay> {
    stats_in_dir(&default_dir(), instance_id, days, &local_today())
}

/// Записать сессию (секунды от старта супервизора до выхода игры). Ошибки
/// записи не роняют вызывавшего — статистика не критична для запуска.
pub(crate) fn record_playtime(instance_id: &str, secs: u64) {
    if let Err(e) = record_in_dir(&default_dir(), instance_id, secs) {
        tracing::warn!("playtime: сессия {instance_id} не записана: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-08 — опорная дата задачи; якорь алгоритма Хиннанта.
    #[test]
    fn civil_roundtrip_known_dates() {
        assert_eq!(civil_to_days(2026, 10, 8), 20_734);
        assert_eq!(days_to_civil(20_734), (2026, 10, 8));
        assert_eq!(days_to_civil(0), (1970, 1, 1));
        assert_eq!(days_to_civil(-1), (1969, 12, 31));
        assert_eq!(civil_to_days(2024, 2, 29), 19_782, "високосный февраль");
        assert_eq!(days_to_civil(civil_to_days(2024, 2, 29)), (2024, 2, 29));
        // Прямой+обратный проход по границам високосных лет.
        for y in [1999, 2000, 2001, 2023, 2024, 2025, 2100] {
            for m in 1..=12u32 {
                let (yy, mm, dd) = days_to_civil(civil_to_days(y, m, 1));
                assert_eq!((yy, mm, dd), (y, m, 1));
            }
        }
    }

    #[test]
    fn date_formatting_and_parsing() {
        assert_eq!(format_ymd(2026, 10, 8), "2026-10-08");
        assert_eq!(format_ymd(999, 1, 2), "0999-01-02");
        assert_eq!(parse_date("2026-10-08"), Some(20_734));
        // Мусор из рук редактора не парсится.
        for bad in ["", "2026-10", "2026-10-08x", "x", "2026-13-01", "2026-00-10", "2026-10-00", "2026-10-32"] {
            assert_eq!(parse_date(bad), None, "{bad:?} не дата");
        }
    }

    #[test]
    fn sessions_same_day_accumulate() {
        let dir = tempfile::tempdir().unwrap();
        record_on(dir.path(), "inst", "2026-10-08", 3600).unwrap();
        record_on(dir.path(), "inst", "2026-10-08", 1800).unwrap();
        record_on(dir.path(), "inst", "2026-10-09", 60).unwrap();
        assert_eq!(
            load(dir.path())["inst"]["2026-10-08"],
            5400,
            "сессии одного дня суммируются"
        );
        assert_eq!(load(dir.path())["inst"]["2026-10-09"], 60);
    }

    #[test]
    fn trim_keeps_only_last_90_days_on_write() {
        let dir = tempfile::tempdir().unwrap();
        // Старые дни (за окном) и на границе окна: 2026-10-08 = 20734 день,
        // окно хранения −89..сегодня (2026-07-10 = −90 выпадает).
        record_on(dir.path(), "inst", "2026-04-01", 100).unwrap(); // ~190 дней назад
        record_on(dir.path(), "inst", "2026-07-11", 200).unwrap(); // граница −89
        record_on(dir.path(), "inst", "2026-07-10", 111).unwrap(); // −90, за окном
        record_on(dir.path(), "inst", "2026-10-08", 300).unwrap(); // «сегодня»

        let store = load(dir.path());
        assert!(!store["inst"].contains_key("2026-04-01"), "за окном — удалён");
        assert!(!store["inst"].contains_key("2026-07-10"), "ровно −90 удалён, окно −89..сегодня");
        assert_eq!(store["inst"]["2026-07-11"], 200, "граница окна сохранена");
        assert_eq!(store["inst"]["2026-10-08"], 300);
    }

    #[test]
    fn unparsable_keys_dropped_on_write() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(crate::util::fs::long_path(dir.path())).unwrap();
        let raw = r#"{"inst": {"2026-10-08": 10, "мусор": 5, "2026-99-99": 7}}"#;
        std::fs::write(crate::util::fs::long_path(&dir.path().join(FILE_NAME)), raw).unwrap();
        record_on(dir.path(), "inst", "2026-10-08", 5).unwrap();
        let store = load(dir.path());
        assert_eq!(store["inst"].len(), 1, "непарсящиеся ключи подчищены");
        assert_eq!(store["inst"]["2026-10-08"], 15);
    }

    #[test]
    fn zero_secs_and_empty_instance_are_noop() {
        let dir = tempfile::tempdir().unwrap();
        record_on(dir.path(), "inst", "2026-10-08", 0).unwrap();
        record_on(dir.path(), "", "2026-10-08", 100).unwrap();
        assert!(!dir.path().join(FILE_NAME).exists(), "пустая запись файл не создаёт");
    }

    #[test]
    fn session_clamped_to_24h() {
        let dir = tempfile::tempdir().unwrap();
        record_on(dir.path(), "inst", "2026-10-08", 25 * 3600).unwrap();
        assert_eq!(load(dir.path())["inst"]["2026-10-08"], 86_400);
        record_on(dir.path(), "inst", "2026-10-09", 86_399).unwrap();
        assert_eq!(load(dir.path())["inst"]["2026-10-09"], 86_399, "меньше клампа не тронуто");
    }

    #[test]
    fn stats_include_zero_days_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        record_on(dir.path(), "inst", "2026-10-01", 7200).unwrap();
        record_on(dir.path(), "inst", "2026-10-05", 1800).unwrap();

        let stats = stats_in_dir(dir.path(), "inst", 7, "2026-10-07");
        assert_eq!(stats.len(), 7, "N дней включая нулевые");
        assert_eq!(stats[0].date, "2026-10-01", "от старых к свежим");
        assert_eq!(stats[0].secs, 7200);
        assert_eq!(stats[2].date, "2026-10-03");
        assert_eq!(stats[2].secs, 0, "день без игры — ноль");
        assert_eq!(stats[4].secs, 1800);
        assert_eq!(stats[6].date, "2026-10-07", "последний — сегодня");
        assert_eq!(stats[6].secs, 0);
    }

    #[test]
    fn stats_other_instances_isolated_and_missing_empty() {
        let dir = tempfile::tempdir().unwrap();
        record_on(dir.path(), "a", "2026-10-08", 60).unwrap();
        assert_eq!(stats_in_dir(dir.path(), "b", 7, "2026-10-08")[6].secs, 0, "чужие секунды не видны");
        // Файла нет вовсе — N нулевых дней.
        let dir2 = tempfile::tempdir().unwrap();
        let stats = stats_in_dir(dir2.path(), "a", 3, "2026-10-08");
        assert_eq!(stats.len(), 3);
        assert!(stats.iter().all(|d| d.secs == 0));
    }

    #[test]
    fn stats_days_zero_empty_and_clamped_to_kept_window() {
        let dir = tempfile::tempdir().unwrap();
        assert!(stats_in_dir(dir.path(), "a", 0, "2026-10-08").is_empty());
        // Запрошено 500 — хранится только 90.
        assert_eq!(stats_in_dir(dir.path(), "a", 500, "2026-10-08").len(), 90);
    }

    #[test]
    fn broken_json_quarantined_other_history_not_wiped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(crate::util::fs::long_path(dir.path())).unwrap();
        std::fs::write(
            crate::util::fs::long_path(&dir.path().join(FILE_NAME)),
            b"{ broken",
        )
        .unwrap();
        // Запись сессии не падает и не затирает чужую историю молча — битый
        // файл уходит в карантин, история начинается с чистого листа.
        record_on(dir.path(), "inst", "2026-10-08", 30).unwrap();
        assert!(dir.path().join("playtime.json.bad").exists());
        assert_eq!(stats_in_dir(dir.path(), "inst", 1, "2026-10-08")[0].secs, 30);
    }

    #[test]
    fn playtime_day_serializes_camel_case_friendly() {
        let day = PlaytimeDay { date: "2026-10-08".into(), secs: 60 };
        let v = serde_json::to_value(&day).unwrap();
        assert_eq!(v["date"], "2026-10-08");
        assert_eq!(v["secs"], 60);
    }
}
