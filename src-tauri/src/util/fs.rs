//! Файловые помощники: атомарные записи, потоковые хэши, long-пути (спека §1, §3).

use crate::errors::Result;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// `\\?\`-префикс для путей длиной ≥ 240 символов (спека §1: длинные пути Windows).
/// ВАЖНО: `\\?\` отключает нормализацию Win32 — прямые слэши внутри пути обязаны
/// стать обратными, иначе os error 123 (реальный кейс: maven rel_path с `/`).
pub fn long_path(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if s.starts_with(r"\\?\") {
            return p.to_path_buf();
        }
        if p.is_absolute() && s.len() >= 240 {
            let normalized = s.replace('/', r"\");
            if let Some(rest) = normalized.strip_prefix(r"\\") {
                // Сетевые пути: `\\server\share` → `\\?\UNC\server\share`
                return PathBuf::from(format!(r"\\?\UNC\{rest}"));
            }
            return PathBuf::from(format!(r"\\?\{normalized}"));
        }
    }
    p.to_path_buf()
}

/// Снять `\\?\`-префикс расширенного пути (A4/A19/A50). Нужен там, где путь
/// попадает в сравнение/`strip_prefix` или в Win32-вызов, который расширенные
/// пути не принимает (например `SetCurrentDirectoryW`). Это НЕ нормализация:
/// `.`/`..` не раскрываются — снимается только маркер формата.
pub fn strip_long_prefix(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            // Обратно в сетевой вид: `\\?\UNC\srv\share` → `\\srv\share`
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    p.to_path_buf()
}

/// Путь в едином виде для СРАВНЕНИЯ (A4, A19): `long_path` добавляет `\\?\`
/// только путям ≥240 символов, поэтому родитель (`C:\a\inst`) и его длинный
/// потомок (`\\?\C:\a\inst\...\deep.txt`) выглядят по-разному, и `starts_with`/
/// `strip_prefix` ложно не срабатывают (мнимый ZipSlip, потерянный rel-путь).
/// Приводим ОБЕ стороны сравнения к виду без префикса — это один и тот же файл.
pub fn normalize_for_compare(p: &Path) -> PathBuf {
    strip_long_prefix(p)
}

/// Путь к временному `.part`-файлу рядом с целью.
pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.push_str(".part");
    dest.with_file_name(name)
}

/// Атомарная запись байтов: данные → `.part` → rename поверх цели (спека §3).
pub fn atomic_write(dest: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(long_path(parent))?;
    }
    let part = part_path(dest);
    {
        let mut f = fs::File::create(long_path(&part))?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    // A26: rename на Windows падает с 5/32, когда файл на миг придержал
    // Defender/OneDrive/индексатор — повтор вместо потери настройки.
    rename_with_retry(&part, dest)?;
    Ok(())
}

/// 1 попытка + 3 повтора с паузами (A26): окно блокировки файла антивирусом —
/// сотни миллисекунд, дольше ждать операцию нет смысла.
const RETRY_DELAYS_MS: [u64; 3] = [100, 250, 500];

/// Временная ли ошибка: файл занят другим процессом, а не логический отказ.
fn is_transient_io(e: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    if matches!(
        e.kind(),
        ErrorKind::PermissionDenied | ErrorKind::WouldBlock | ErrorKind::Interrupted
    ) {
        return true;
    }
    // Windows не всегда маппит коды блокировок в PermissionDenied:
    // 5 ACCESS_DENIED, 32 SHARING_VIOLATION, 33 LOCK_VIOLATION.
    matches!(e.raw_os_error(), Some(5 | 32 | 33))
}

fn with_retry(mut op: impl FnMut() -> std::io::Result<()>, what: &str) -> Result<()> {
    for delay in RETRY_DELAYS_MS {
        match op() {
            Ok(()) => return Ok(()),
            Err(e) if is_transient_io(&e) => {
                tracing::debug!("{what}: занято ({e}), повтор через {delay} мс");
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            Err(e) => return Err(e.into()),
        }
    }
    // Последний повтор: ошибку отдаём наружу как есть.
    match op() {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!("{what}: не удалось после повторов: {e}");
            Err(e.into())
        }
    }
}

/// Переименование с повторами на Windows-блокировках (A26).
pub fn rename_with_retry(from: &Path, to: &Path) -> Result<()> {
    with_retry(|| fs::rename(long_path(from), long_path(to)), "rename")
}

/// Удаление с повторами; отсутствующий файл — не ошибка (идемпотентно).
pub fn remove_file_with_retry(path: &Path) -> Result<()> {
    match with_retry(|| fs::remove_file(long_path(path)), "remove_file") {
        Err(crate::errors::LauncherError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(())
        }
        other => other,
    }
}

/// SHA1 файла, потоково (файл не грузится в память целиком).
pub fn sha1_file(path: &Path) -> Result<String> {
    let mut f = fs::File::open(long_path(path))?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// SHA256 файла, потоково.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = fs::File::open(long_path(path))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// SHA1 байтов в памяти.
pub fn sha1_bytes(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Удалить файл, игнорируя «не существует».
pub fn remove_file_ignore(path: &Path) {
    let _ = fs::remove_file(long_path(path));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_replaces_and_cleans_part() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a").join("file.json");
        atomic_write(&dest, b"v1").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v1");
        atomic_write(&dest, b"v2-longer").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v2-longer");
        assert!(!part_path(&dest).exists());
    }

    #[test]
    fn sha1_bytes_known_vector() {
        assert_eq!(
            sha1_bytes(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn sha1_file_matches_known() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha1_file(&p).unwrap(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn sha256_file_matches_known() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[cfg(windows)]
    #[test]
    fn long_path_prefixes_deep_paths() {
        let mut deep = PathBuf::from(r"C:\");
        let seg = "abcdefghijklmnopqrstuvwxyz0123456789"; // 36 символов
        for _ in 0..8 {
            deep.push(seg);
        }
        assert!(deep.to_string_lossy().len() >= 240);
        let lp = long_path(&deep);
        assert!(lp.to_string_lossy().starts_with(r"\\?\C:\"));
        // Короткие пути не трогаем
        assert_eq!(long_path(Path::new(r"C:\x")), PathBuf::from(r"C:\x"));
    }

    #[cfg(windows)]
    #[test]
    fn long_path_normalizes_forward_slashes() {
        // Реальный кейс M3: длинный maven-путь с `/` внутри + префикс \\?\ =
        // os error 123, если не нормализовать сепараторы.
        let mut deep = PathBuf::from(r"C:\");
        let seg = "abcdefghijklmnopqrstuvwxyz0123456789";
        for _ in 0..8 {
            deep.push(seg);
        }
        let with_slashes = deep.join("com/google/guava/x.jar");
        let lp = long_path(&with_slashes).to_string_lossy().into_owned();
        assert!(lp.starts_with(r"\\?\C:\"));
        assert!(!lp.contains('/'), "в \\?\\-пути не должно быть прямых слэшей: {lp}");
    }

    #[cfg(windows)]
    #[test]
    fn normalize_for_compare_aligns_long_and_short_forms() {
        // A4/A19-регресс: родитель КОРОТКИЙ (без префикса), потомок ДЛИННЫЙ
        // (long_path его префиксует) — в исходном виде сравнение префиксов
        // ложно не срабатывало (мнимый ZipSlip, потерянный rel-путь).
        let root = PathBuf::from(r"C:\short");
        let seg = "abcdefghijklmnopqrstuvwxyz0123456789";
        let mut child = root.clone();
        for _ in 0..7 {
            child.push(seg);
        }
        let child = child.join("file.txt");
        let child_long = long_path(&child);
        assert!(child.to_string_lossy().len() >= 240);
        assert!(child_long.to_string_lossy().starts_with(r"\\?\"));
        // Асимметрия, которая и ломала starts_with/strip_prefix:
        assert!(
            !child_long.starts_with(&root),
            "сырые формы одного дерева несовместимы"
        );
        // После нормализации обеих сторон отношение сохраняется.
        let root_cmp = normalize_for_compare(&root);
        let child_cmp = normalize_for_compare(&child_long);
        assert_eq!(root_cmp, root);
        assert_eq!(child_cmp, child);
        assert!(child_cmp.starts_with(&root_cmp), "потомок остаётся внутри базы");
    }

    #[cfg(windows)]
    #[test]
    fn strip_long_prefix_returns_unc_and_plain() {
        // UNC-форма возвращается к `\\srv\share` — её принимает SetCurrentDirectoryW.
        assert_eq!(
            strip_long_prefix(Path::new(r"\\?\UNC\srv\share\dir")),
            PathBuf::from(r"\\srv\share\dir")
        );
        assert_eq!(
            strip_long_prefix(Path::new(r"\\?\C:\x\y")),
            PathBuf::from(r"C:\x\y")
        );
        // Путь без префикса не меняется.
        assert_eq!(strip_long_prefix(Path::new(r"C:\x")), PathBuf::from(r"C:\x"));
    }

    #[test]
    fn rename_with_retry_moves_and_errors_on_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.tmp");
        let to = dir.path().join("a.bin");
        fs::write(&from, b"v1").unwrap();
        rename_with_retry(&from, &to).unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"v1");
        assert!(!from.exists());
        // Логическая ошибка (нет источника) не зацикливается и видна наружу.
        let err = rename_with_retry(&from, &to).unwrap_err();
        assert_eq!(err.code(), "io");
    }

    /// A26: файл, придержанный другим процессом (ERROR_SHARING_VIOLATION),
    /// не должен валить переименование — повтор обязан дождаться снятия лока.
    #[cfg(windows)]
    #[test]
    fn rename_with_retry_survives_share_violation() {
        use std::os::windows::fs::OpenOptionsExt;
        use std::time::Duration;
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("locked.tmp");
        let to = dir.path().join("locked.bin");
        fs::write(&from, b"payload").unwrap();
        // share_mode(0) — соседние открытия/переименование обязаны падать.
        let holder = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&from)
            .unwrap();
        let from2 = from.clone();
        let to2 = to.clone();
        let t = std::thread::spawn(move || rename_with_retry(&from2, &to2));
        // Снимаем лок внутри окна повторов (100+250+500 мс).
        std::thread::sleep(Duration::from_millis(150));
        drop(holder);
        t.join().unwrap().unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"payload");
    }

    #[test]
    fn remove_file_with_retry_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("gone");
        remove_file_with_retry(&p).unwrap(); // файла нет — не ошибка
        fs::write(&p, b"x").unwrap();
        remove_file_with_retry(&p).unwrap();
        assert!(!p.exists());
    }
}
