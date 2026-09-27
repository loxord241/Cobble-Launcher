//! Контентный стор + жёсткие ссылки (спека §6.12): клиенты/библиотеки/ассеты
//! по хэшу один раз; в инстанс — hardlink (same volume), при неудаче — копия.

use crate::errors::Result;
use std::path::{Path, PathBuf};

/// Hardlink `src → dst` с фолбэком на копию (другой том).
pub fn link_or_copy(src: &Path, dst: &Path) -> Result<()> {
    let src_l = crate::util::fs::long_path(src);
    let dst_l = crate::util::fs::long_path(dst);
    if let Some(parent) = dst_l.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if dst_l.exists() {
        return Ok(()); // уже на месте
    }
    if std::fs::hard_link(&src_l, &dst_l).is_err() {
        // A22: сбой копирования (диск полон/оборвалось чтение) оставлял бы
        // усечённый файл, а следующий вызов видел бы `exists()` и считал его
        // готовым — недописанное убираем, чтобы повтор перекопировал честно.
        if let Err(e) = std::fs::copy(&src_l, &dst_l) {
            let _ = std::fs::remove_file(&dst_l);
            return Err(e.into());
        }
    }
    Ok(())
}

/// Проверить, является ли файл жёсткой ссылкой с количеством ссылок > 1
/// (для «очистить кэш»: удалять только объекты без ссылок).
pub fn hardlink_count(path: &Path) -> u64 {
    #[cfg(windows)]
    {
        windows_link_count(path).unwrap_or(1)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        crate::util::fs::long_path(path)
            .metadata()
            .map(|m| m.nlink())
            .unwrap_or(1)
    }
}

/// nNumberOfLinks через kernel32: `windows_by_handle` в std нестабилен,
/// а зависимость тянуть ради одного поля не стоит.
#[cfg(windows)]
fn windows_link_count(path: &Path) -> Option<u64> {
    use std::fs::File;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    #[allow(non_snake_case)]
    struct BY_HANDLE_FILE_INFORMATION {
        dwFileAttributes: u32,
        ftCreationTime: [u32; 2],
        ftLastAccessTime: [u32; 2],
        ftLastWriteTime: [u32; 2],
        dwVolumeSerialNumber: u32,
        nFileSizeHigh: u32,
        nFileSizeLow: u32,
        nNumberOfLinks: u32,
        nFileIndexHigh: u32,
        nFileIndexLow: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(
            handle: *mut core::ffi::c_void,
            info: *mut BY_HANDLE_FILE_INFORMATION,
        ) -> i32;
    }

    let file = File::open(crate::util::fs::long_path(path)).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION {
        dwFileAttributes: 0,
        ftCreationTime: [0; 2],
        ftLastAccessTime: [0; 2],
        ftLastWriteTime: [0; 2],
        dwVolumeSerialNumber: 0,
        nFileSizeHigh: 0,
        nFileSizeLow: 0,
        nNumberOfLinks: 0,
        nFileIndexHigh: 0,
        nFileIndexLow: 0,
    };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    if ok != 0 {
        Some(u64::from(info.nNumberOfLinks))
    } else {
        None
    }
}

/// Обход дерева: вернуть все файлы относительно корня.
pub fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(crate::util::fs::long_path(&dir)) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

/// Копирование дерева при дублировании инстанса (спека §6.12): файлы в
/// уникальных каталогах (saves/mods/…) — КОПИЯ байтами, остальные — hardlink.
pub fn copy_tree_mixed(src_root: &Path, dst_root: &Path) -> Result<()> {
    // A19: `walk_files` отдаёт потомков в форме, которую выбрал `long_path`
    // на каждом уровне (родитель короткий — без `\\?\`, длинный потомок — с ним).
    // Сырой `strip_prefix` из-за этой асимметрии не срабатывал, `rel` оставался
    // АБСОЛЮТНЫМ путём источника, и `dst_root.join(rel)` (join с абсолютным
    // путём заменяет всё) указывал на сам источник — файл не дублировался.
    let base = crate::util::fs::normalize_for_compare(src_root);
    for file in walk_files(src_root) {
        let file_cmp = crate::util::fs::normalize_for_compare(&file);
        let rel = file_cmp.strip_prefix(&base).unwrap_or(&file_cmp);
        let dst = dst_root.join(rel);
        let rel_lossy = rel.to_string_lossy().replace('\\', "/");
        let is_unique = super::UNIQUE_SUBDIRS
            .iter()
            .any(|d| rel_lossy.starts_with(&format!("{d}/")))
            || super::UNIQUE_FILES.contains(&rel_lossy.as_str());
        if is_unique {
            if let Some(parent) = crate::util::fs::long_path(&dst).parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(crate::util::fs::long_path(&file), crate::util::fs::long_path(&dst))?;
        } else {
            link_or_copy(&file, &dst)?;
        }
    }
    Ok(())
}

/// «Очистить кэш»: удалить объекты стора, на которые нет ссылок (спека §6.12).
pub fn cleanup_store(paths: &crate::paths::Paths) -> Result<usize> {
    let mut removed = 0;
    for root in [paths.libraries_store(), paths.assets_objects(), paths.clients_store()] {
        for file in walk_files(&root) {
            if hardlink_count(&file) <= 1 {
                crate::util::fs::remove_file_ignore(&file);
                removed += 1;
            }
        }
        // пустые каталоги
        remove_empty_dirs(&root);
    }
    Ok(removed)
}

fn remove_empty_dirs(root: &Path) {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(crate::util::fs::long_path(&d)) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            }
        }
        dirs.push(d);
    }
    // самые глубокие первыми
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(crate::util::fs::long_path(&d));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_or_copy_creates_shared_content() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.jar");
        std::fs::write(&src, b"data").unwrap();
        let dst = dir.path().join("sub/dir/dst.jar");
        link_or_copy(&src, &dst).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"data");
        assert_eq!(hardlink_count(&dst), 2, "hardlink на том же томе");
        // Повтор — идемпотентно
        link_or_copy(&src, &dst).unwrap();
    }

    #[test]
    fn cleanup_removes_unlinked_keeps_linked() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        // Связанный объект: стор + ссылка в инстансе
        let linked = paths.libraries_store().join("a.jar");
        std::fs::write(&linked, b"keep").unwrap();
        let inst_copy = paths.instances_dir().join("i1/a.jar");
        link_or_copy(&linked, &inst_copy).unwrap();
        // Несвязанный
        let unlinked = paths.assets_objects().join("z/z.obj");
        std::fs::create_dir_all(crate::util::fs::long_path(unlinked.parent().unwrap())).unwrap();
        std::fs::write(&unlinked, b"drop").unwrap();

        let removed = cleanup_store(&paths).unwrap();
        assert_eq!(removed, 1);
        assert!(linked.exists(), "связанный остался");
        assert!(!unlinked.exists(), "несвязанный удалён");
    }

    /// A22: сбой копирования (фолбэк после неудачного hardlink) не оставляет
    /// на диске файл-призрак, который следующий вызов сочёл бы готовым.
    #[test]
    fn failed_copy_leaves_no_ghost_destination() {
        let dir = tempfile::tempdir().unwrap();
        // Источник — КАТАЛОГ: hardlink невозможен, копирование обязано упасть.
        // (На Unix `File::create` уже создаёт цель и бросает её при ошибке чтения;
        // на Windows CopyFileExW падает до создания цели — инвариант один.)
        let src_dir = dir.path().join("as-dir");
        std::fs::create_dir_all(&src_dir).unwrap();
        let dst = dir.path().join("out").join("dst.bin");
        let err = link_or_copy(&src_dir, &dst).unwrap_err();
        assert_eq!(err.code(), "io", "{err}");
        assert!(!dst.exists(), "недописанный файл не должен остаться: {dst:?}");
    }

    /// A19-регресс: глубокий уникальный файл (saves/…) обязан скопироваться в
    /// дубликат, а источник — остаться нетронутым. До фикса `strip_prefix`
    /// ломался на `\\?\`-асимметрии (родитель короткий, потомок длинный),
    /// `rel` становился абсолютным путём источника, и `join` заменял им
    /// каталог назначения — файл в копию не попадал.
    #[test]
    fn copy_tree_mixed_handles_deep_unique_paths() {
        let dir = tempfile::tempdir().unwrap();
        let src_root = dir.path().join("src");
        let seg = "abcdefghijklmnopqrstuvwxyz0123456789";
        // Промежуточный каталог сам длиннее порога префиксации (240): именно
        // тогда `read_dir` внутри walk_files зовётся на `\\?\`-пути и потомки
        // приходят в другой форме, чем корень.
        let mut deep_dir = src_root.join("saves");
        while deep_dir.to_string_lossy().len() < 245 {
            deep_dir.push(seg);
        }
        std::fs::create_dir_all(crate::util::fs::long_path(&deep_dir)).unwrap();
        let deep_file = deep_dir.join("level.dat");
        std::fs::write(crate::util::fs::long_path(&deep_file), b"save-bytes").unwrap();
        // Общий файл (как libraries): должен стать hardlink'ом.
        let lib = src_root.join("libraries").join("lib.jar");
        std::fs::create_dir_all(crate::util::fs::long_path(lib.parent().unwrap())).unwrap();
        std::fs::write(&lib, b"lib-bytes").unwrap();

        let dst_root = dir.path().join("dst");
        copy_tree_mixed(&src_root, &dst_root).unwrap();

        let rel_deep = deep_file.strip_prefix(&src_root).unwrap();
        let dst_deep = dst_root.join(rel_deep);
        assert_eq!(
            std::fs::read(crate::util::fs::long_path(&dst_deep)).unwrap(),
            b"save-bytes",
            "глубокая копия уникального файла обязана появиться"
        );
        assert_eq!(hardlink_count(&dst_deep), 1, "уникальное копируется байтами");
        assert_eq!(
            std::fs::read(crate::util::fs::long_path(&deep_file)).unwrap(),
            b"save-bytes",
            "источник не должен изменяться"
        );
        let dst_lib = dst_root.join("libraries").join("lib.jar");
        assert_eq!(std::fs::read(&dst_lib).unwrap(), b"lib-bytes");
        assert_eq!(hardlink_count(&dst_lib), 2, "общее линкуется, а не копируется");
    }
}
