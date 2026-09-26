//! Zip-распаковка с защитой от zip-slip и zip-bomb (спека §3, §11).
//! Чужие zip (импорты, модпаки) — недоверенный вход.

use crate::errors::{LauncherError, Result};
use std::io::{Read, Seek};
use std::path::{Component, Path, PathBuf};

/// Лимиты на архив по умолчанию: защита от zip-bomb.
pub const MAX_TOTAL_UNCOMPRESSED: u64 = 8 * 1024 * 1024 * 1024; // 8 ГБ
pub const MAX_ENTRIES: usize = 200_000;

/// Проверить имя записи архива и вернуть безопасный относительный путь.
/// Запрещены: `..`, абсолютные пути, `\\`-трюки, NUL, буквы дисков.
pub fn safe_relative_path(raw_name: &str) -> Result<PathBuf> {
    if raw_name.contains('\0') {
        return Err(LauncherError::ZipSlip(raw_name.into()));
    }
    let normalized = raw_name.replace('\\', "/");
    let mut out = PathBuf::new();
    for comp in Path::new(&normalized).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            // `..`, `/`, `C:` — любые из них означают попытку выхода или абсолютный путь
            _ => return Err(LauncherError::ZipSlip(raw_name.into())),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(LauncherError::ZipSlip(raw_name.into()));
    }
    Ok(out)
}

/// Распаковать zip в `dest`. Возвращает список извлечённых файлов.
/// Каждый путь проверяется `safe_relative_path` + итог остаётся внутри `dest`.
pub fn extract_zip<R: Read + Seek>(
    reader: R,
    dest: &Path,
    on_file: impl FnMut(&Path),
) -> Result<Vec<PathBuf>> {
    extract_zip_filtered(reader, dest, |_| true, on_file)
}

/// То же с фильтром имён (для нативов: только dll/so/jnilib, без META-INF).
pub fn extract_zip_filtered<R: Read + Seek>(
    reader: R,
    dest: &Path,
    keep: impl Fn(&str) -> bool,
    mut on_file: impl FnMut(&Path),
) -> Result<Vec<PathBuf>> {
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| LauncherError::Zip(format!("не удалось открыть архив: {e}")))?;
    if archive.len() > MAX_ENTRIES {
        return Err(LauncherError::Zip(format!(
            "слишком много записей в архиве: {}",
            archive.len()
        )));
    }
    let dest_long = crate::util::fs::long_path(dest);
    std::fs::create_dir_all(&dest_long)?;
    let mut extracted = Vec::new();
    let mut total_uncompressed: u64 = 0;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| LauncherError::Zip(format!("запись {i}: {e}")))?;
        let name = file.name().to_string();

        if !keep(&name) {
            continue;
        }

        // Симлинки из чужих архивов не создаём.
        if file.is_symlink() {
            tracing::warn!("zip: пропуск симлинка {name}");
            continue;
        }

        let rel = safe_relative_path(&name)?;
        let target = crate::util::fs::long_path(&dest.join(&rel));

        // Двойная проверка: итоговый путь обязан остаться внутри dest.
        if !target.starts_with(&dest_long) {
            return Err(LauncherError::ZipSlip(name));
        }

        if file.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }

        total_uncompressed += file.size();
        if total_uncompressed > MAX_TOTAL_UNCOMPRESSED {
            return Err(LauncherError::Zip(
                "архив превышает лимит распаковки (возможен zip-bomb)".into(),
            ));
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut file, &mut out)?;
        out.sync_all()?;
        on_file(&target);
        extracted.push(dest.join(rel));
    }
    Ok(extracted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::SimpleFileOptions = Default::default();
            for (name, data) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(data).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn extracts_normal_entries() {
        let data = build_zip(&[("mods/a.jar", b"AAA"), ("config/x.txt", b"X")]);
        let dir = tempfile::tempdir().unwrap();
        let files = extract_zip(
            std::io::Cursor::new(data),
            dir.path(),
            |_| {},
        )
        .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(
            std::fs::read(dir.path().join("mods/a.jar")).unwrap(),
            b"AAA"
        );
    }

    #[test]
    fn rejects_traversal_up() {
        let data = build_zip(&[("../evil.txt", b"X")]);
        let dir = tempfile::tempdir().unwrap();
        let err = extract_zip(std::io::Cursor::new(data), dir.path(), |_| {}).unwrap_err();
        assert_eq!(err.code(), "zip_slip");
    }

    #[test]
    fn rejects_absolute_and_drive_paths() {
        for name in ["/etc/passwd", "C:/windows/evil", r"\\server\share\x"] {
            let data = build_zip(&[(name, b"X")]);
            let dir = tempfile::tempdir().unwrap();
            assert!(
                extract_zip(std::io::Cursor::new(data), dir.path(), |_| {}).is_err(),
                "должен отклонить {name}"
            );
        }
    }

    #[test]
    fn rejects_backslash_trick_and_nul() {
        for name in ["a\\..\\..\\evil", "ev\0il"] {
            assert!(safe_relative_path(name).is_err(), "должен отклонить {name:?}");
        }
        // Обратный слэш сам по себе — просто разделитель (нормализуется), но с .. — ошибка
        assert!(safe_relative_path("mods\\sub\\file.jar").is_ok());
    }

    #[test]
    fn truncated_zip_gives_clear_error() {
        let data = build_zip(&[("a.txt", b"hello world, long enough")]);
        let truncated = &data[..data.len() / 2];
        let dir = tempfile::tempdir().unwrap();
        let err = extract_zip(std::io::Cursor::new(truncated.to_vec()), dir.path(), |_| {})
            .unwrap_err();
        assert_eq!(err.code(), "zip");
    }
}
