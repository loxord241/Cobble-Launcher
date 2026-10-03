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
    // A4: база для проверки вложенности — в том же виде, что и `target` ниже.
    // `long_path` префиксует `\\?\` только путям ≥240 символов, поэтому сырое
    // сравнение отвергало КОРРЕКТНЫЕ архивы, распакованные в глубокий каталог.
    let dest_cmp = crate::util::fs::normalize_for_compare(&dest_long);
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
        if !crate::util::fs::normalize_for_compare(&target).starts_with(&dest_cmp) {
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
        drop(out);
        on_file(&target);
        extracted.push(dest.join(rel));
    }
    Ok(extracted)
}





/// Вариант со СРЕЗАНИЕМ префикса (конвенция модпаков: содержимое `overrides/`
/// ложится поверх корня каталога игры). zip-slip-защита сохранена; лимиты
/// записей/размера те же, что в `extract_zip_filtered` — сюда приходят
/// НЕдоверенные архивы (.mrpack, CF-zip), защита от zip-bomb обязательна (D3).
pub fn extract_zip_stripping<R: Read + Seek>(
    reader: R,
    dest: &Path,
    strip: &str,
    _keep: impl Fn(&str) -> bool,
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
    // A4: та же нормализация базы, что и в `extract_zip_filtered`.
    let dest_cmp = crate::util::fs::normalize_for_compare(&dest_long);
    let mut out = Vec::new();
    let mut total_uncompressed: u64 = 0;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| LauncherError::Zip(format!("запись {i}: {e}")))?;
        let raw = file.name().to_string();
        if file.is_dir() || !raw.starts_with(strip) {
            continue;
        }
        // A24: симлинки из ЧУЖОГО архива не материализуем (как в filtered-варианте):
        // содержимое записи — цель ссылки, и запись её файлом только путает инструменты.
        if file.is_symlink() {
            tracing::warn!("zip: пропуск симлинка {raw}");
            continue;
        }
        total_uncompressed += file.size();
        if total_uncompressed > MAX_TOTAL_UNCOMPRESSED {
            return Err(LauncherError::Zip(
                "архив превышает лимит распаковки (возможен zip-bomb)".into(),
            ));
        }
        let rel_full = safe_relative_path(&raw)?;
        let rel = rel_full
            .strip_prefix(strip)
            .map(Path::to_path_buf)
            .unwrap_or(rel_full);
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = crate::util::fs::long_path(&dest.join(&rel));
        if !crate::util::fs::normalize_for_compare(&target).starts_with(&dest_cmp) {
            return Err(LauncherError::ZipSlip(raw));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::io::copy(&mut file, &mut std::fs::File::create(&target)?)?;
        on_file(&target);
        out.push(dest.join(rel));
    }
    Ok(out)
}

/// Запаковать каталог в ZipWriter: записи идут под `prefix/`, пустые
/// каталоги не создаются. `skip` получает путь (каталог или файл)
/// относительно `dir`, слэши; true для каталога = поддерево целиком.
/// Возвращает число записанных файлов. Наш собственный zip — доверенный вход.
pub fn write_zip_dir(
    dir: &Path,
    zip: &mut zip::ZipWriter<impl std::io::Write + std::io::Seek>,
    prefix: &str,
    skip: &dyn Fn(&str) -> bool,
) -> Result<usize> {
    let opts: zip::write::SimpleFileOptions = Default::default();
    let mut count = 0usize;
    fn walk(
        base: &Path,
        cur: &Path,
        zip: &mut zip::ZipWriter<impl std::io::Write + std::io::Seek>,
        opts: &zip::write::SimpleFileOptions,
        prefix: &str,
        skip: &dyn Fn(&str) -> bool,
        count: &mut usize,
    ) -> Result<()> {
        let it = std::fs::read_dir(crate::util::fs::long_path(cur))?;
        let mut entries: Vec<_> = it.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            let rel = path
                .strip_prefix(base)
                .map_err(|_| LauncherError::InvalidInput("walk: префикс".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            let ft = e.file_type()?;
            if ft.is_dir() {
                if skip(&rel) {
                    continue;
                }
                walk(base, &path, zip, opts, prefix, skip, count)?;
            } else if ft.is_file() && !skip(&rel) {
                let name = format!("{prefix}{rel}");
                zip.start_file(name, *opts)
                    .map_err(|err| LauncherError::Zip(format!("start_file {rel}: {err}")))?;
                // PERF#4: поток (io::copy, буфер 64 КБ) вместо чтения файла
                // целиком в RAM — моды бывают по 80 МБ.
                let mut f = std::fs::File::open(crate::util::fs::long_path(&path))?;
                std::io::copy(&mut f, zip)?;
                *count += 1;
            }
        }
        Ok(())
    }
    walk(dir, dir, zip, &opts, prefix, skip, &mut count)?;
    Ok(count)
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
    fn stripping_extracts_and_respects_limits() {
        // D3: stripping-вариант работает как раньше и считает лимит по своим файлам.
        let data = build_zip(&[
            ("overrides/mods/a.jar", b"AAA"),
            ("meta/junk.txt", b"JUNKJUNK"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let files = extract_zip_stripping(
            std::io::Cursor::new(data),
            dir.path(),
            "overrides/",
            |_| true,
            |_| {},
        )
        .unwrap();
        assert_eq!(files, vec![dir.path().join("mods/a.jar")]);
        assert_eq!(std::fs::read(dir.path().join("mods/a.jar")).unwrap(), b"AAA");
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

    /// Каталог, который сам ещё «короткий» (< порога префиксации), но
    /// `каталог + запись архива` его переваливает — ровно случай A4.
    fn base_just_below_long_threshold(base: &Path) -> PathBuf {
        let mut p = base.to_path_buf();
        let seg = "abcdefghijklmnopqrstuvwxyz0123456789";
        while p.to_string_lossy().len() + seg.len() + 1 < 232 {
            p.push(seg);
        }
        p
    }

    /// Имя записи, которого хватает, чтобы `dest.join(name)` стало ≥240 символов.
    fn deep_entry_name(base: &Path) -> String {
        let base_len = base.to_string_lossy().len();
        let mut name = String::new();
        while base_len + 1 + name.len() + "file.txt".len() < 245 {
            name.push_str("dir123456789012345678901234567890/");
        }
        name.push_str("file.txt");
        name
    }

    /// A4-регресс: длинный ИТОГОВЫЙ путь не должен давать ложный ZipSlip.
    #[cfg(windows)]
    #[test]
    fn deep_dest_does_not_trigger_false_zipslip() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_just_below_long_threshold(dir.path());
        let name = deep_entry_name(&base);
        let joined = base.join(&name);
        // Сам каталог распаковки — «короткий» (без `\\?\`), а итоговый файл — уже нет.
        let dest_long = crate::util::fs::long_path(&base);
        let target_long = crate::util::fs::long_path(&joined);
        assert!(!dest_long.to_string_lossy().starts_with(r"\\?\"));
        assert!(target_long.to_string_lossy().starts_with(r"\\?\"));
        // Именно эта проверка стояла в коде до фикса и ложно отвергала валидный архив:
        assert!(
            !target_long.starts_with(&dest_long),
            "асимметрия представлений обязана воспроизводиться"
        );

        let data = build_zip(&[(&name, b"AAA")]);
        let files = extract_zip(std::io::Cursor::new(data.clone()), &base, |_| {})
            .expect("валидный архив не должен получать ZipSlip");
        assert_eq!(files.len(), 1);
        assert_eq!(std::fs::read(base.join(&name)).unwrap(), b"AAA");

        // Тот же случай для stripping-варианта (overrides/ из модпака).
        let strip_name = format!("overrides/{name}");
        let sdata = build_zip(&[(&strip_name, b"BBB")]);
        let sbase = base_just_below_long_threshold(dir.path());
        let files = extract_zip_stripping(
            std::io::Cursor::new(sdata),
            &sbase,
            "overrides/",
            |_| true,
            |_| {},
        )
        .expect("stripping-вариант не должен получать ложный ZipSlip");
        assert_eq!(files.len(), 1);
        assert_eq!(std::fs::read(sbase.join(&name)).unwrap(), b"BBB");
    }

    /// A24: симлинк из чужого архива не материализуется (как в filtered-варианте).
    #[test]
    fn stripping_skips_symlink_entries() {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::SimpleFileOptions = Default::default();
            w.add_symlink("overrides/link.txt", "../../outside.txt", opts)
                .unwrap();
            w.start_file("overrides/real.txt", opts).unwrap();
            w.write_all(b"OK").unwrap();
            w.finish().unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let files = extract_zip_stripping(
            std::io::Cursor::new(buf.into_inner()),
            dir.path(),
            "overrides/",
            |_| true,
            |_| {},
        )
        .unwrap();
        assert_eq!(files.len(), 1, "распакован только обычный файл: {files:?}");
        assert!(
            !dir.path().join("link.txt").exists(),
            "симлинк не должен появляться на диске"
        );
        assert_eq!(std::fs::read(dir.path().join("real.txt")).unwrap(), b"OK");
    }
}