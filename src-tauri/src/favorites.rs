//! Избранное каталога Modrinth: локальный список id проектов, без сети и
//! аккаунта. Хранение — `favorites.json` в каталоге данных (рядом с
//! settings.json), формат — JSON-массив строк в порядке добавления; запись
//! атомарная (`util::fs::atomic_write`: tmp + rename).

use crate::errors::{LauncherError, Result};
use crate::util::fs::atomic_write;
use std::path::{Path, PathBuf};

/// Имя файла в каталоге данных.
const FILE_NAME: &str = "favorites.json";

/// Сериализация read-modify-write в рамках процесса (toggle из UI).
static FAV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Каталог данных по умолчанию — тот же, что settings.json (paths.rs).
fn default_dir() -> PathBuf {
    crate::paths::Paths::default_root()
}

fn file_path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// Битый favorites.json — в карантин: переименовать в `favorites.json.bad`
/// (занятый слот не затираем — тогда `.bad.1`, `.bad.2`, ...). Иначе первый же
/// toggle молча затёр бы файл пустым списком, а вторая порча — молча затёрла бы
/// прежний карантин: слоты оставляют данные для ручного восстановления
/// (та же схема слотов, что у settings.json).
fn quarantine_broken(path: &Path, reason: &str) {
    for i in 0..10u32 {
        let mut name = path.as_os_str().to_os_string();
        match i {
            0 => name.push(".bad"),
            _ => name.push(format!(".bad.{i}")),
        }
        let candidate = PathBuf::from(name);
        if candidate.exists() {
            continue;
        }
        match crate::util::fs::rename_with_retry(path, &candidate) {
            Ok(()) => {
                tracing::warn!("favorites.json невалиден ({reason}) — перемещён в {}", candidate.display());
                return;
            }
            Err(e) => {
                tracing::warn!("favorites.json невалиден ({reason}), карантин не удался: {e}");
                return;
            }
        }
    }
    tracing::warn!("favorites.json невалиден ({reason}); слоты карантина заняты — файл оставлен как есть");
}

/// Читать список; файла нет → пустой, битый → карантин + пустой (избранное не
/// критичные данные — не роняем UI).
fn load(dir: &Path) -> Vec<String> {
    let path = file_path(dir);
    let long = crate::util::fs::long_path(&path);
    if !long.exists() {
        return Vec::new();
    }
    match std::fs::read(&long) {
        Ok(data) => match serde_json::from_slice(&data) {
            Ok(ids) => ids,
            Err(e) => {
                quarantine_broken(&path, &e.to_string());
                Vec::new()
            }
        },
        Err(e) => {
            tracing::warn!("favorites.json не прочитан: {e}");
            Vec::new()
        }
    }
}

/// Атомарно записать (tmp + rename, как settings.json).
fn save(dir: &Path, ids: &[String]) -> Result<()> {
    let data = serde_json::to_vec_pretty(ids)?;
    atomic_write(&file_path(dir), &data)
}

/// Список избранного из указанного каталога данных (параметризация для тестов).
pub fn list_in(dir: &Path) -> Vec<String> {
    let _guard = FAV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    load(dir)
}

/// Список избранного (id проектов Modrinth, порядок добавления).
pub fn list_favorites() -> Vec<String> {
    list_in(&default_dir())
}

/// Toggle по указанному каталогу данных (параметризация для тестов).
/// Возвращает `true` — добавили, `false` — убрали.
pub fn toggle_in(dir: &Path, project_id: &str) -> Result<bool> {
    let id = project_id.trim();
    if id.is_empty() {
        return Err(LauncherError::InvalidInput(
            "project_id пуст — в избранное сохранять нечего".into(),
        ));
    }
    let _guard = FAV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ids = load(dir);
    let added = match ids.iter().position(|p| p == id) {
        Some(pos) => {
            ids.remove(pos);
            false
        }
        None => {
            ids.push(id.to_string());
            true
        }
    };
    save(dir, &ids)?;
    Ok(added)
}

/// Добавить/убрать проект в избранном. `true` — добавили, `false` — убрали.
pub fn toggle_favorite(project_id: &str) -> Result<bool> {
    toggle_in(&default_dir(), project_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_list_persist_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Пусто до первого toggle; файл появляется после записи.
        assert!(list_in(root).is_empty());
        assert!(toggle_in(root, "aabbccdd").unwrap());
        assert!(root.join(FILE_NAME).exists());

        // Второй toggle того же id — убрали (false).
        assert!(!toggle_in(root, "aabbccdd").unwrap());
        assert!(list_in(root).is_empty());

        // persist: список читается с диска в порядке добавления.
        for id in ["xx", "aa", "mm"] {
            assert!(toggle_in(root, id).unwrap());
        }
        assert_eq!(list_in(root), vec!["xx".to_string(), "aa".to_string(), "mm".to_string()]);

        // Удаление из середины сохраняет порядок остальных.
        assert!(!toggle_in(root, "aa").unwrap());
        assert_eq!(list_in(root), vec!["xx".to_string(), "mm".to_string()]);
    }

    #[test]
    fn missing_dir_and_file_are_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("нет-такого-каталога");
        assert!(list_in(&deep).is_empty());
    }

    #[test]
    fn broken_json_quarantined_and_toggle_starts_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, b"{ not an array").unwrap();

        assert!(list_in(dir.path()).is_empty(), "битый файл читается как пусто");
        assert!(path.with_file_name("favorites.json.bad").exists(), "битый файл в карантине");

        // Toggle не падает и начинает список заново (битый уже в карантине).
        assert!(toggle_in(dir.path(), "id1").unwrap());
        assert_eq!(list_in(dir.path()), vec!["id1".to_string()]);
    }

    /// Вторая порча подряд не затирает первый карантин — новый файл уезжает
    /// в свободный слот `.bad.1` (схема settings.json).
    #[test]
    fn quarantine_does_not_overwrite_previous_bad() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, b"{ first broken").unwrap();
        assert!(list_in(dir.path()).is_empty());
        assert!(path.with_file_name("favorites.json.bad").exists());

        std::fs::write(&path, b"{ second broken").unwrap();
        assert!(list_in(dir.path()).is_empty());
        let first = std::fs::read_to_string(dir.path().join("favorites.json.bad")).unwrap();
        assert_eq!(first, "{ first broken", "прежний карантин не затёрт");
        assert!(path.with_file_name("favorites.json.bad.1").exists(), "вторая порча в соседнем слоте");
    }

    #[test]
    fn empty_and_whitespace_project_id_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            toggle_in(dir.path(), ""),
            Err(LauncherError::InvalidInput(_))
        ));
        assert!(matches!(
            toggle_in(dir.path(), "   "),
            Err(LauncherError::InvalidInput(_))
        ));
        assert!(!dir.path().join(FILE_NAME).exists(), "файл не создаётся отказом");
    }

    #[test]
    fn trimmed_id_deduplicates_with_untrimmed_previous() {
        let dir = tempfile::tempdir().unwrap();
        assert!(toggle_in(dir.path(), "  aabb  ").unwrap(), "id обрезается по краям");
        // Повтор с другим обрамлением — тот же элемент, убрали, а не добавили.
        assert!(!toggle_in(dir.path(), "aabb").unwrap());
        assert!(list_in(dir.path()).is_empty());
    }
}
