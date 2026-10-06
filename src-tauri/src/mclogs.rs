//! Отправка логов на mclo.gs с санитизацией (F22, D37) + реестр архивных
//! логов инстанса (F23).
//!
//! Санитизация — на вызывающем: `upload` получает готовый текст (ядро
//! команд `log_share_mclogs` читает latest.log и прогоняет через
//! `util::redact` перед отправкой).

use crate::errors::{LauncherError, Result};
use crate::paths::Paths;

/// API mclo.gs: вынесен в константу — офлайн-тест не ходит в сеть.
const MCLOGS_API: &str = "https://api.mclo.gs/1/log";

/// Потолок тела ответа mclo.gs (короткий JSON `{success,url|error}`, спека
/// сервиса): адр-хок чтение `resp.text()` без границы тянуло в память всё,
/// что отдаст сервер (аудит 2026-10-06).
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// Читаемые архивные логи: `*.log` и ротаты `*.log.gz` (flate2 — прямая
/// зависимость, D42). Граница чтения одинаковая: не более `max_bytes`
/// РАСПАКОВАННОГО текста — иначе .gz стал бы декомпрессионной бомбой.
const READABLE_EXTS: [&str; 2] = ["log", "gz"];

/// Имя файла лога, разрешённое из UI/диска: латиница/цифры/`.`/`_`/`-`,
/// база 1–128 символов, расширение .log/.gz. Регекс-эквивалент
/// `^[A-Za-z0-9._\-]{1,128}\.(log|gz)$`; разделителей пути нет —
/// path traversal через имя невозможен (A23).
fn is_valid_log_name(name: &str) -> bool {
    let base = name
        .strip_suffix(".log")
        .or_else(|| name.strip_suffix(".gz"));
    let Some(base) = base else {
        return false;
    };
    !base.is_empty()
        && base.len() <= 128
        && base
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// Имя лога из IPC-границы: невалидное — ошибка, а не тихий пропуск.
fn require_valid_log_name(name: &str) -> Result<()> {
    if is_valid_log_name(name) {
        Ok(())
    } else {
        Err(LauncherError::InvalidInput(format!(
            "некорректное имя лога: {name}"
        )))
    }
}

/// Каталог логов игры инстанса: `instances/<id>/minecraft/logs`.
fn instance_logs_dir(paths: &Paths, instance_id: &str) -> Result<std::path::PathBuf> {
    // Валидация id до склейки пути: тот же барьер, что в instance_open_dir (A23).
    crate::instances::valid_id(instance_id)?;
    let dir = crate::instances::instance_dir(paths, instance_id);
    Ok(crate::instances::minecraft_dir(&dir).join("logs"))
}

/// Запись реестра архивных логов (зеркало `LogFileInfo` в src/api/types.ts).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFileInfo {
    pub name: String,
    pub bytes: u64,
    pub modified: i64,
}

/// mtime в секундах от эпохи; недоступное время → 0 (файл просто уходит в
/// конец списка, сортировка не ломается).
fn modified_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Список архивных логов инстанса (кроме текущего latest.log), по mtime убыв.
/// Битые/чужие имена пропускаются: один мусорный файл в каталоге не должен
/// ломать всю историю; IPO-граница валидируется в `read_archived`.
pub fn list_archived(paths: &Paths, instance_id: &str) -> Result<Vec<LogFileInfo>> {
    let logs_dir = instance_logs_dir(paths, instance_id)?;
    if !logs_dir.exists() {
        return Ok(Vec::new()); // игра ещё не запускалась — история пуста
    }
    let mut out: Vec<LogFileInfo> = Vec::new();
    for entry in std::fs::read_dir(crate::util::fs::long_path(&logs_dir))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "latest.log" || !READABLE_EXTS.iter().any(|e| name.ends_with(e)) {
            continue;
        }
        if !is_valid_log_name(&name) {
            continue;
        }
        let meta = entry.metadata()?;
        out.push(LogFileInfo {
            name,
            bytes: meta.len(),
            modified: modified_secs(&meta),
        });
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.modified));
    Ok(out)
}

/// Последние `max_bytes` байт файла (ENV-9): seek от конца, в память попадает
/// только хвост. Раньше гигантский latest.log читался `std::fs::read` целиком
/// — при логе 500 МБ+ и `panic = "abort"` OOM ронял лаунчер. Файл короче
/// границы — читается целиком (возврат меньше `max_bytes` означает «весь файл»).
pub fn read_tail(path: &std::path::Path, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    if max_bytes == 0 {
        return Ok(Vec::new());
    }
    let mut file = std::fs::OpenOptions::new().read(true).open(path)?;
    let len = file.metadata()?.len();
    let skip = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(skip))?;
    let mut buf = Vec::with_capacity((len - skip) as usize);
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Прочитать архивный лог: последние `max_bytes` байт, не разрывая строки
/// (хвост обрезается до начала следующей строки). Для `*.log.gz` — то же,
/// но граница считается по РАСПАКОВАННОМУ тексту: `Read::take` не даёт
/// декомпрессионной бомбе раздуть память, сколько бы ни было на диске.
pub fn read_archived(
    paths: &Paths,
    instance_id: &str,
    name: &str,
    max_bytes: u64,
) -> Result<String> {
    let logs_dir = instance_logs_dir(paths, instance_id)?;
    require_valid_log_name(name)?;
    if max_bytes == 0 {
        return Ok(String::new());
    }
    let path = crate::util::fs::long_path(&logs_dir.join(name));
    if name.ends_with(".gz") {
        return gz_tail(&path, max_bytes);
    }
    // ENV-9: весь файл в память не тянем — только seek-хвост.
    let data = read_tail(&path, max_bytes)?;
    if (data.len() as u64) < max_bytes {
        // Файл меньше границы — вернулся целиком, резать хвост не нужно.
        return Ok(String::from_utf8_lossy(&data).into_owned());
    }
    let tail = String::from_utf8_lossy(&data);
    // Не рвать строку посреди: отступаем до первого \n в хвосте.
    match tail.find('\n') {
        Some(pos) => Ok(tail[pos + 1..].to_string()),
        None => Ok(tail.into_owned()), // одной строкой без переводов — как есть
    }
}

/// Потолок СУММАРНО распакованного объёма gzip-лога (аудит 2026-10-06):
/// хвостовой cap держит память, но не время — гигабайтная бомба читалась бы
/// минуты, подвешивая воркер. Больше потолка — честный отказ, а не чтение.
const GZ_TOTAL_CAP: u64 = 256 * 1024 * 1024;

/// Хвост РАСПАКОВАННОГО gzip-лога (последние `max_bytes`, выровненные по
/// началу строки). Потоковое чтение с обрезкой буфера спереди: память
/// ограничена max_bytes независимо от размера архива — декомпрессионная
/// бомба упирается в тот же потолок, что и обычный лог. Битый gzip —
/// честная io-ошибка из декодера; суммарный объём сверх [`GZ_TOTAL_CAP`] —
/// InvalidInput («слишком большой распакованный лог»).
fn gz_tail(path: &std::path::Path, max_bytes: u64) -> Result<String> {
    gz_tail_with_cap(path, max_bytes, GZ_TOTAL_CAP)
}

/// Рабочая версия с явным потолком суммарного объёма (параметр — для тестов).
fn gz_tail_with_cap(path: &std::path::Path, max_bytes: u64, total_cap: u64) -> Result<String> {
    use std::io::Read as _;
    let mut decoder = flate2::read::GzDecoder::new(std::fs::File::open(path)?);
    let mut buf: Vec<u8> = Vec::new();
    let mut total = 0usize; // весь распакованный объём — решает «хвост или целиком»
    let mut chunk = [0u8; 8192];
    loop {
        let n = decoder.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        total += n;
        if total as u64 > total_cap {
            return Err(LauncherError::InvalidInput(format!(
                "слишком большой распакованный лог: > {total_cap} байт"
            )));
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > max_bytes as usize {
            let keep = max_bytes as usize;
            buf.drain(..buf.len() - keep);
        }
    }
    if total as u64 <= max_bytes {
        return Ok(String::from_utf8_lossy(&buf).into_owned());
    }
    let start = buf.len() - max_bytes as usize;
    let tail = String::from_utf8_lossy(&buf[start..]).into_owned();
    Ok(match tail.find('\n') {
        Some(pos) => tail[pos + 1..].to_string(),
        None => tail,
    })
}

/// Отправить СОДЕРЖИМОЕ на mclo.gs (санитизация снаружи — вызывающий
/// передаёт готовый текст). Возвращает короткую ссылку на лог.
pub async fn upload(client: &crate::net::http::HttpClient, content: &str) -> Result<String> {
    // F16: офлайн-гейт до любых сетевых вызовов.
    if client.offline() {
        return Err(LauncherError::OfflineMode("mclo.gs".into()));
    }
    // Предохранитель на случай молчащего сервера (общий таймаут у HttpClient
    // снят, аудит 2026-10-06).
    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        client
            .raw()
            .post(MCLOGS_API)
            .form(&[("content", content)]) // urlencoded: % и \n кодируются
            .send(),
    )
    .await
    .map_err(|_| LauncherError::network("mclo.gs не отвечает"))??;
    // Content-Length известен заранее: известный перебор — ошибка до чтения
    // тела (как в modrinth/mrpack.rs).
    if let Some(len) = resp.content_length() {
        if len > MAX_RESPONSE_BYTES as u64 {
            return Err(LauncherError::network(format!(
                "mclo.gs: ответ слишком большой ({len} > {MAX_RESPONSE_BYTES} байт)"
            )));
        }
    }
    let body = resp.bytes().await?;
    if body.len() > MAX_RESPONSE_BYTES {
        // Честный ответ без Content-Length (chunked) — пост-проверка.
        return Err(LauncherError::network(format!(
            "mclo.gs: ответ слишком большой ({} > {MAX_RESPONSE_BYTES} байт)",
            body.len()
        )));
    }
    parse_response(&String::from_utf8_lossy(&body))
}

/// Ответ mclo.gs: `{success, url?, error?}`.
#[derive(serde::Deserialize)]
struct MclogsResponse {
    success: bool,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Разбор ответа mclo.gs (отдельная чистая функция — покрывается юнит-тестами
/// без сети): `!success` → Network с текстом ошибки сервиса.
fn parse_response(body: &str) -> Result<String> {
    let parsed: MclogsResponse = serde_json::from_str(body)
        .map_err(|e| LauncherError::network(format!("mclo.gs: некорректный ответ: {e}")))?;
    if !parsed.success {
        return Err(LauncherError::network(format!(
            "mclo.gs: {}",
            parsed
                .error
                .unwrap_or_else(|| "сервер отклонил загрузку".into())
        )));
    }
    parsed
        .url
        .ok_or_else(|| LauncherError::network("mclo.gs: в ответе нет ссылки"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        (dir, paths)
    }

    /// Каталог логов инстанса в тестовом руте (без создания instance.json —
    /// mclogs работает с файловой системой напрямую).
    fn make_logs_dir(paths: &Paths, id: &str) -> std::path::PathBuf {
        let dir = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, id))
            .join("logs");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Поставить файлу явный mtime (секунды от эпохи) — для проверки сортировки.
    fn set_mtime(path: &std::path::Path, secs: u64) {
        let f = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs),
        ))
        .unwrap();
    }

    // ---------- валидация имени ----------

    #[test]
    fn valid_log_names_match_regex_equivalent() {
        for ok in [
            "2026-09-28-1.log",
            "latest-1.log",
            "crash_report.log",
            "a.log",
            "debug.log.gz",
            "A9-_.log",
        ] {
            assert!(is_valid_log_name(ok), "должно пройти: {ok}");
        }
    }

    #[test]
    fn invalid_log_names_rejected() {
        for bad in [
            "",
            ".log",
            ".gz",
            "../evil.log",
            r"..\evil.log",
            "a/b.log",
            r"a\b.log",
            "a b.log",
            "лог.log", // не-ASCII
            "a.exe",
            "a.log.txt",
            "a.LOG", // регистр расширения строгий
        ] {
            assert!(!is_valid_log_name(bad), "должно быть отклонено: {bad}");
        }
    }

    #[test]
    fn too_long_base_name_rejected() {
        let ok = format!("{}.log", "a".repeat(128));
        let bad = format!("{}.log", "a".repeat(129));
        assert!(is_valid_log_name(&ok));
        assert!(!is_valid_log_name(&bad));
    }

    // ---------- list_archived ----------

    /// Записать настоящий gzip-файл (для тестов чтения ротатов).
    fn write_gz(path: &std::path::Path, content: &[u8]) {
        use std::io::Write as _;
        let mut enc = flate2::write::GzEncoder::new(
            std::fs::File::create(path).unwrap(),
            flate2::Compression::default(),
        );
        enc.write_all(content).unwrap();
        enc.finish().unwrap();
    }

    #[test]
    fn list_includes_gz_rotations_sorted_by_mtime_desc() {
        let (_guard, paths) = test_paths();
        let logs = make_logs_dir(&paths, "inst1");
        std::fs::write(logs.join("latest.log"), "current").unwrap();
        std::fs::write(logs.join("2026-01-01-1.log"), "old").unwrap();
        std::fs::write(logs.join("2026-02-01-1.log"), "newer").unwrap();
        write_gz(&logs.join("2026-03-01-1.log.gz"), b"newest");
        std::fs::write(logs.join("readme.txt"), "не лог").unwrap();
        set_mtime(&logs.join("2026-01-01-1.log"), 1_000);
        set_mtime(&logs.join("2026-02-01-1.log"), 2_000);
        set_mtime(&logs.join("2026-03-01-1.log.gz"), 3_000);

        let list = list_archived(&paths, "inst1").unwrap();
        let names: Vec<&str> = list.iter().map(|f| f.name.as_str()).collect();
        // gz-ротат читается наравне с .log (D42); latest.log — текущий, txt — не лог.
        assert_eq!(
            names,
            vec!["2026-03-01-1.log.gz", "2026-02-01-1.log", "2026-01-01-1.log"]
        );
        assert_eq!(
            list[0].bytes,
            std::fs::metadata(logs.join("2026-03-01-1.log.gz"))
                .unwrap()
                .len()
        );
        assert_eq!(list[0].modified, 3_000, "свежий первым (mtime убыв.)");
    }

    #[test]
    fn list_empty_when_no_logs_dir_or_no_files() {
        let (_guard, paths) = test_paths();
        assert!(list_archived(&paths, "fresh").unwrap().is_empty());
        let logs = make_logs_dir(&paths, "empty1");
        std::fs::write(logs.join("latest.log"), "x").unwrap();
        assert!(list_archived(&paths, "empty1").unwrap().is_empty());
    }

    #[test]
    fn list_rejects_bad_instance_id() {
        let (_guard, paths) = test_paths();
        let err = list_archived(&paths, "../../windows").expect_err("traversal id");
        assert!(matches!(err, LauncherError::InvalidInput(_)));
    }

    #[test]
    fn log_file_info_serializes_camel_case() {
        let info = LogFileInfo {
            name: "a.log".into(),
            bytes: 12,
            modified: 3,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"name\""), "{json}");
        assert!(json.contains("\"bytes\""), "{json}");
        assert!(json.contains("\"modified\""), "{json}");
    }

    // ---------- read_archived ----------

    #[test]
    fn read_small_file_returns_full_content() {
        let (_guard, paths) = test_paths();
        let logs = make_logs_dir(&paths, "inst1");
        std::fs::write(logs.join("a.log"), "line1\nline2\n").unwrap();
        let text = read_archived(&paths, "inst1", "a.log", 1024).unwrap();
        assert_eq!(text, "line1\nline2\n");
    }

    #[test]
    fn read_truncates_to_whole_lines_from_tail() {
        let (_guard, paths) = test_paths();
        let logs = make_logs_dir(&paths, "inst1");
        let content = "line1\nline2\nline3\n"; // 18 байт
        std::fs::write(logs.join("a.log"), content).unwrap();
        // Последние 10 байт: "ne2\nline3\n" — первая строка рваная, отступаем до \n.
        let text = read_archived(&paths, "inst1", "a.log", 10).unwrap();
        assert_eq!(text, "line3\n");
    }

    #[test]
    fn read_zero_cap_gives_empty() {
        let (_guard, paths) = test_paths();
        let logs = make_logs_dir(&paths, "inst1");
        std::fs::write(logs.join("a.log"), "data").unwrap();
        assert_eq!(read_archived(&paths, "inst1", "a.log", 0).unwrap(), "");
    }

    #[test]
    fn read_rejects_traversal_and_garbage_names() {
        let (_guard, paths) = test_paths();
        for bad in ["../x.log", "x/y.log", "x y.log", "x.txt", ""] {
            let err = read_archived(&paths, "inst1", bad, 1024).expect_err(bad);
            assert!(
                matches!(err, LauncherError::InvalidInput(_)),
                "{bad} → InvalidInput"
            );
        }
    }

    #[test]
    fn read_gz_small_returns_full_content() {
        let (_guard, paths) = test_paths();
        let logs = make_logs_dir(&paths, "inst1");
        write_gz(&logs.join("a.log.gz"), b"line1\nline2\n");
        let text = read_archived(&paths, "inst1", "a.log.gz", 1024).unwrap();
        assert_eq!(text, "line1\nline2\n");
    }

    #[test]
    fn read_gz_tail_aligned_to_line_start() {
        let (_guard, paths) = test_paths();
        let logs = make_logs_dir(&paths, "inst1");
        // 3 строки: последние 10 распакованных байт = "ne2\nline3\n" → "line3\n".
        write_gz(&logs.join("a.log.gz"), b"line1\nline2\nline3\n");
        let text = read_archived(&paths, "inst1", "a.log.gz", 10).unwrap();
        assert_eq!(text, "line3\n");
    }

    #[test]
    fn read_gz_bomb_is_capped_by_tail_cap() {
        let (_guard, paths) = test_paths();
        let logs = make_logs_dir(&paths, "inst1");
        // ~1 МБ распакованного из крошечного архива: cap держит память.
        let big = vec![b'x'; 1024 * 1024];
        write_gz(&logs.join("big.log.gz"), &big);
        let text = read_archived(&paths, "inst1", "big.log.gz", 1024).unwrap();
        assert!(text.len() <= 1024, "хвост не больше cap");
        assert!(text.ends_with('x'), "хвост содержит конец данных");
    }

    /// Аудит 2026-10-06: суммарный распакованный объём сверх потолка —
    /// честный InvalidInput БЫСТРО (крошечный gz распаковывается в 300 КБ,
    /// потолок в тесте 64 КБ; с боевыми 256 МБ это были бы минуты чтения).
    #[test]
    fn read_gz_over_total_cap_is_fast_error() {
        let (_guard, paths) = test_paths();
        let logs = make_logs_dir(&paths, "inst1");
        // Синтетическая «бомба»: сжатый архив крошечный, распаковка — 300 КБ.
        let bomb = vec![b'x'; 300 * 1024];
        write_gz(&logs.join("bomb.log.gz"), &bomb);
        let path = crate::util::fs::long_path(&logs.join("bomb.log.gz"));
        let err = gz_tail_with_cap(&path, 1024, 64 * 1024)
            .expect_err("распаковка сверх потолка должна дать ошибку");
        assert!(
            matches!(err, LauncherError::InvalidInput(ref m) if m.contains("слишком большой")),
            "ожидался InvalidInput про размер, получено: {err:?}"
        );
        // Читаемый потолок выше реального распакованного размера — проходит.
        let ok = gz_tail_with_cap(&path, 1024, 1024 * 1024).unwrap();
        assert!(ok.ends_with('x'), "под потолком читается хвост");
    }

    #[test]
    fn read_gz_corrupt_is_honest_io_error() {
        let (_guard, paths) = test_paths();
        let logs = make_logs_dir(&paths, "inst1");
        std::fs::write(logs.join("bad.log.gz"), b"\x1f\x8b").unwrap(); // обрезанный gzip
        let err = read_archived(&paths, "inst1", "bad.log.gz", 1024).expect_err("битый gzip");
        assert!(
            matches!(err, LauncherError::Io(_)),
            "битый архив — io-ошибка декодера, получено: {err:?}"
        );
    }

    #[test]
    fn read_missing_file_is_not_found() {
        let (_guard, paths) = test_paths();
        make_logs_dir(&paths, "inst1");
        let err = read_archived(&paths, "inst1", "ghost.log", 1024).expect_err("нет файла");
        assert!(matches!(err, LauncherError::NotFound(_)) || matches!(err, LauncherError::Io(_)));
    }

    // ---------- read_tail (ENV-9) ----------

    /// Гигантский файл не читается целиком: хвост ровно cap и совпадает
    /// с концом файла; маленький файл возвращается весь.
    #[test]
    fn read_tail_caps_at_max_and_keeps_file_end() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.log");
        let mut content = b"HEAD line\n".to_vec();
        content.extend(vec![b'x'; 1024 * 1024]);
        std::fs::write(&big, &content).unwrap();
        let cap = 512 * 1024usize;
        let tail = read_tail(&big, cap as u64).unwrap();
        assert_eq!(tail.len(), cap, "хвост ровно cap для файла больше cap");
        assert_eq!(&tail[..], &content[content.len() - cap..], "это конец файла");

        let small = dir.path().join("small.log");
        std::fs::write(&small, b"abc").unwrap();
        assert_eq!(read_tail(&small, cap as u64).unwrap(), b"abc", "меньше cap — целиком");
        assert!(read_tail(&small, 0).unwrap().is_empty(), "cap=0 — пусто");
    }

    /// Плоский .log на ~1 МБ читается в пределах cap и содержит конец файла —
    /// как gz-хвост выше (раньше здесь был цельный `std::fs::read`).
    #[test]
    fn read_plain_big_log_is_capped_by_tail() {
        let (_guard, paths) = test_paths();
        let logs = make_logs_dir(&paths, "inst1");
        let big = vec![b'x'; 1024 * 1024];
        std::fs::write(logs.join("big.log"), &big).unwrap();
        let text = read_archived(&paths, "inst1", "big.log", 1024).unwrap();
        assert!(text.len() <= 1024, "хвост не больше cap");
        assert!(text.ends_with('x'), "хвост содержит конец данных");
    }

    // ---------- upload / parse_response ----------

    #[test]
    fn parse_response_success_returns_url() {
        let url = parse_response(r#"{"success":true,"url":"https://mclo.gs/abcd"}"#).unwrap();
        assert_eq!(url, "https://mclo.gs/abcd");
    }

    #[test]
    fn parse_response_failure_is_network_with_service_error() {
        let err = parse_response(r#"{"success":false,"error":"Missing content"}"#)
            .expect_err("отказ сервиса");
        assert!(matches!(err, LauncherError::Network(_)));
        assert!(err.to_string().contains("Missing content"), "{err}");
    }

    #[test]
    fn parse_response_success_without_url_is_error() {
        let err = parse_response(r#"{"success":true}"#).expect_err("нет url");
        assert!(matches!(err, LauncherError::Network(_)));
    }

    #[test]
    fn parse_response_garbage_is_network_error() {
        let err = parse_response("<html>502</html>").expect_err("мусор");
        assert!(matches!(err, LauncherError::Network(_)));
    }

    /// F16: офлайн-гейт срабатывает до похода в сеть (127.0.0.1:1 — если бы
    /// запрос ушёл, получили бы Http/Network после таймаута, а не OfflineMode).
    #[tokio::test]
    async fn upload_offline_fails_before_io() {
        let client = crate::net::http::HttpClient::new(None).unwrap();
        client.set_offline(true);
        let err = upload(&client, "log text").await.expect_err("офлайн");
        assert!(matches!(err, LauncherError::OfflineMode(_)));
        assert_eq!(err.code(), "offline_mode");
    }

    /// Реальная сеть — #[ignore], гонять на приёмке (спека §9), как
    /// real_github_release_fetch в commands.
    #[tokio::test]
    #[ignore]
    async fn real_mclogs_upload() {
        let client = crate::net::http::HttpClient::new(None).unwrap();
        let url = upload(&client, "[12:34:56] [main/INFO]: test\n").await.unwrap();
        assert!(url.starts_with("https://mclo.gs/"), "{url}");
    }
}
