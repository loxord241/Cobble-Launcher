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
    fs::rename(long_path(&part), long_path(dest))?;
    Ok(())
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
}
