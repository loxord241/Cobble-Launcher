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
        std::fs::copy(&src_l, &dst_l)?;
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
    for file in walk_files(src_root) {
        let rel = file
            .strip_prefix(crate::util::fs::long_path(src_root))
            .unwrap_or(&file);
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
}
