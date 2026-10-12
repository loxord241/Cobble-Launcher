//! Реестр бэкапов инстансов (F4): список для UI, удаление, ротация.
//!
//! Бэкапы — zip в `backups/` (Paths::backups_dir) с именем
//! «{имя инстанса}-{unix-сек}.zip»; создаёт их content::backup. Раньше
//! они копились бесконечно: теперь ядро умеет перечислять их для UI,
//! удалять по имени (с барьером traversal) и ронять старейшие сверх
//! лимита после каждого нового бэкапа (ротацию вызывает commands-обёртка,
//! т.к. слою instances настройки недоступны).

use crate::errors::{LauncherError, Result};
use crate::paths::Paths;
use serde::Serialize;

/// Одна запись каталога бэкапов. Все поля выводятся из имени файла,
/// поэтому имена не по формату в список просто не попадают (skip).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    /// Имя файла в каталоге бэкапов; его же UI передаёт в backup_delete.
    pub file: String,
    /// Имя инстанса — часть имени файла до последнего «-цифры».
    pub instance: String,
    /// Штамп создания (unix-секунды), извлечённый из имени файла.
    pub created_at: u64,
    /// Размер zip на диске, байт.
    pub size_bytes: u64,
}

/// Разобрать имя файла «{instance}-{unix}.zip» → (instance, unix-сек).
/// None — имя не по формату (не .zip, нет хвоста-числа, пустая часть
/// имени): список такие молча пропускает, удаление — отклоняет.
/// D71: учитывается суффикс коллизий «-1»…«-9» перед расширением
/// (content::create_archive_file): «Inst-1728000000-1.zip» — это имя
/// «Inst», штамп 1728000000 (раньше rsplit_once брал последний сегмент
/// «1» — имя инстанса искажалось, ротация сортировала такие файлы неверно).
/// Правило: если ДВА последних сегмента числовые, штамп — предпоследний,
/// суффикс отбрасывается, именем становится всё до него.
pub(crate) fn parse_backup_name(file_name: &str) -> Option<(String, u64)> {
    let stem = file_name.strip_suffix(".zip")?;
    let mut parts: Vec<&str> = stem.split('-').collect();
    let last = *parts.last()?;
    if parts.len() >= 2 && !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()) {
        let prev = parts[parts.len() - 2];
        if !prev.is_empty() && prev.bytes().all(|b| b.is_ascii_digit()) {
            parts.pop();
        }
    }
    let stamp = *parts.last()?;
    if parts.len() < 2 || stamp.is_empty() || !stamp.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let name = parts[..parts.len() - 1].join("-");
    if name.is_empty() {
        return None;
    }
    // Переполнение (немыслимо длинный «штамп») — тоже «битое имя».
    Some((name, stamp.parse().ok()?))
}

/// Список всех бэкапов, новые сверху (created_at по убыванию; при равных
/// штампах — по имени, чтобы порядок был детерминированным). Каталога нет
/// или он пуст — пустой список; битые имена и каталоги-«.zip» пропускаются.
pub fn list(paths: &Paths) -> Vec<BackupEntry> {
    let root = paths.backups_dir();
    let Ok(entries) = std::fs::read_dir(crate::util::fs::long_path(&root)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let Some((instance, created_at)) = parse_backup_name(&name) else {
            continue;
        };
        // Каталог с именем «inst-123.zip» — не бэкап.
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        out.push(BackupEntry {
            file: name,
            instance,
            created_at,
            size_bytes: meta.len(),
        });
    }
    out.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.file.cmp(&b.file))
    });
    out
}

/// Удалить бэкап по имени файла. `file` обязан быть ПРОСТЫМ именем
/// (без `/`, `\`, `..`), с расширением .zip, и реально лежать в каталоге
/// бэкапов: путь канонизируется и сверяется префиксом с канонизированным
/// backups_dir — traversal и симлинк наружу недопустимы (A1).
pub fn delete(paths: &Paths, file: &str) -> Result<()> {
    if file.is_empty()
        || file.contains('/')
        || file.contains('\\')
        || file.contains("..")
        || !file.ends_with(".zip")
    {
        return Err(LauncherError::InvalidInput(format!(
            "недопустимое имя файла бэкапа: {file:?}"
        )));
    }
    // Каталог бэкапов должен существовать, иначе нечего канонизировать.
    std::fs::create_dir_all(crate::util::fs::long_path(&paths.backups_dir()))?;
    // После проверок выше в `file` нет разделителей — join не выходит из каталога;
    // canonicalize нужен, чтобы сверка префикса не обманулась `..`/симлинками.
    let target = paths.backups_dir().join(file);
    let target_real = std::fs::canonicalize(crate::util::fs::long_path(&target))
        .map_err(|_| LauncherError::NotFound(format!("бэкап {file:?}")))?;
    let dir_real = std::fs::canonicalize(crate::util::fs::long_path(&paths.backups_dir()))?;
    if !target_real.starts_with(&dir_real) {
        return Err(LauncherError::InvalidInput(format!(
            "файл {} вне каталога бэкапов",
            target_real.display()
        )));
    }
    std::fs::remove_file(target_real)?;
    Ok(())
}

/// Ротация бэкапов инстанса: оставить `keep` новейших (по штампу из имени),
/// старейшие сверх лимита — удалить. `keep = 0` — безлимит, ничего не трогаем.
/// Best-effort: сбой удаления одного файла (занят антивирусом и т.п.) не
/// мешает остальным и не возвращает ошибку — ротация вызывается после
/// успешного бэкапа и не должна его «отменять».
pub fn rotate(paths: &Paths, instance: &str, keep: u32) {
    if keep == 0 {
        return;
    }
    // В имени файла лежит ОЧИЩЕННОЕ имя инстанса (content::backup пишет через
    // sanitize_user_name) — очищаем аргумент так же, иначе «Мир 1» не совпал
    // бы с именем после санитайза (схлопнутые пробелы и пр.).
    let wanted = crate::util::names::sanitize_user_name(instance, "instance");
    // list() отдаёт новые сверху — хвост после keep и есть старейшие лишние.
    let mut mine: Vec<BackupEntry> = list(paths)
        .into_iter()
        .filter(|e| e.instance == wanted)
        .collect();
    if mine.len() <= keep as usize {
        return;
    }
    for stale in mine.drain(keep as usize..) {
        if let Err(e) = delete(paths, &stale.file) {
            tracing::warn!("ротация бэкапов: не удалён {}: {e}", stale.file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn setup() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        (dir, paths)
    }

    fn long(p: &std::path::Path) -> PathBuf {
        crate::util::fs::long_path(p)
    }

    fn put(paths: &Paths, name: &str, bytes: &[u8]) {
        std::fs::write(long(&paths.backups_dir().join(name)), bytes).unwrap();
    }

    fn exists(paths: &Paths, name: &str) -> bool {
        long(&paths.backups_dir().join(name)).exists()
    }

    /// list: валидные имена парсятся (включая пробелы в имени инстанса),
    /// битые и каталоги-«.zip» пропускаются; порядок — новые сверху;
    /// сериализация в camelCase (контракт фронта).
    #[test]
    fn list_parses_valid_skips_broken_and_sorts_newest_first() {
        let (_d, paths) = setup();
        put(&paths, "Alpha-1717000000.zip", b"AAA");
        put(&paths, "My Mod-1717000001.zip", b"BBBBBB");
        // Битые имена — skip, не ошибка.
        put(&paths, "plain.zip", b"x"); // нет «-цифры»
        put(&paths, "Alpha-notanumber.zip", b"x");
        put(&paths, "Alpha-.zip", b"x"); // пустой штамп
        put(&paths, "-1717000002.zip", b"x"); // пустое имя инстанса
        put(&paths, "Alpha-99999999999999999999.zip", b"x"); // штамп не влезает в u64
        put(&paths, "notes.txt", b"x"); // не .zip
        std::fs::create_dir(long(&paths.backups_dir().join("Dir-1717000003.zip"))).unwrap();

        let all = list(&paths);
        assert_eq!(all.len(), 2, "только валидные имена: {all:?}");
        assert_eq!(all[0].file, "My Mod-1717000001.zip", "новые сверху");
        assert_eq!(all[0].instance, "My Mod");
        assert_eq!(all[0].created_at, 1_717_000_001);
        assert_eq!(all[0].size_bytes, 6);
        assert_eq!(all[1].file, "Alpha-1717000000.zip");
        assert_eq!(all[1].instance, "Alpha");
        assert_eq!(all[1].size_bytes, 3);

        // camelCase на IPC-границе.
        let json = serde_json::to_string(&all[0]).unwrap();
        assert!(json.contains("\"createdAt\":1717000001"), "{json}");
        assert!(json.contains("\"sizeBytes\":6"), "{json}");
        assert!(json.contains("\"file\":\"My Mod-1717000001.zip\""), "{json}");
        assert!(json.contains("\"instance\":\"My Mod\""), "{json}");
    }

    /// list: каталога бэкапов нет — пустой список, а не ошибка.
    #[test]
    fn list_without_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        assert!(list(&paths).is_empty());
    }

    /// rotate: keep=3 из 5 — удаляются 2 старейших, 3 новейших остаются;
    /// чужой инстанс не тронут; keep=0 — безлимит; имена с пробелами
    /// сопоставляются после того же санитайза, что пишет content::backup.
    #[test]
    fn rotate_keeps_newest_and_spares_other_instances() {
        let (_d, paths) = setup();
        for stamp in 1000..=1004u64 {
            put(&paths, &format!("Alpha-{stamp}.zip"), b"A");
        }
        put(&paths, "Beta-1000.zip", b"B");
        put(&paths, "Beta-1004.zip", b"B");

        rotate(&paths, "Alpha", 3);
        assert!(!exists(&paths, "Alpha-1000.zip"), "старейший удалён");
        assert!(!exists(&paths, "Alpha-1001.zip"), "второй старейший удалён");
        assert!(exists(&paths, "Alpha-1002.zip"), "новейшие остаются");
        assert!(exists(&paths, "Alpha-1003.zip"));
        assert!(exists(&paths, "Alpha-1004.zip"));
        assert!(exists(&paths, "Beta-1000.zip"), "чужой инстанс не тронут");
        assert!(exists(&paths, "Beta-1004.zip"), "чужой инстанс не тронут");

        // keep=0 — безлимит: ничего не удаляется.
        rotate(&paths, "Alpha", 0);
        assert!(exists(&paths, "Alpha-1002.zip"));
        assert_eq!(list(&paths).iter().filter(|e| e.instance == "Alpha").count(), 3);

        // Имя с пробелом: сопоставление через sanitize_user_name.
        for stamp in [500u64, 501, 502] {
            put(&paths, &format!("My Mod-{stamp}.zip"), b"M");
        }
        rotate(&paths, "My Mod", 1);
        assert!(!exists(&paths, "My Mod-500.zip"));
        assert!(!exists(&paths, "My Mod-501.zip"));
        assert!(exists(&paths, "My Mod-502.zip"), "новейший остался");
    }

    /// rotate: лишнего нет — ничего не удаляется (в т.ч. чужие файлы).
    #[test]
    fn rotate_without_surplus_deletes_nothing() {
        let (_d, paths) = setup();
        put(&paths, "Alpha-100.zip", b"A");
        put(&paths, "Beta-101.zip", b"B");
        rotate(&paths, "Alpha", 3);
        assert!(exists(&paths, "Alpha-100.zip"));
        assert!(exists(&paths, "Beta-101.zip"));
    }

    /// D71: суффикс коллизии «-N» перед расширением не ломает разбор —
    /// штамп берётся из ПРЕДПОСЛЕДНЕГО числового сегмента, именем становится
    /// всё, что до него (обычные имена и «битые» — как раньше).
    #[test]
    fn parse_backup_name_handles_collision_suffix() {
        assert_eq!(
            parse_backup_name("Inst-1728000000-1.zip"),
            Some(("Inst".into(), 1_728_000_000)),
            "суффикс -1 отброшен, штамп настоящий"
        );
        assert_eq!(
            parse_backup_name("My Mod-1717000001-9.zip"),
            Some(("My Mod".into(), 1_717_000_001))
        );
        // Обычное имя без суффикса — прежнее поведение.
        assert_eq!(
            parse_backup_name("Alpha-1717000000.zip"),
            Some(("Alpha".into(), 1_717_000_000))
        );
        // Не-числовой сегмент перед хвостом: суффикс не похож на коллизию
        // «-N после штампа» — разбор по-легаси (хвост = штамп), как было.
        assert_eq!(
            parse_backup_name("Alpha-notanumber-1.zip"),
            Some(("Alpha-notanumber".into(), 1))
        );
        assert_eq!(parse_backup_name("Alpha-notanumber.zip"), None);
        assert_eq!(parse_backup_name("-1717000002-1.zip"), None);
        assert_eq!(parse_backup_name("Alpha-99999999999999999999-1.zip"), None);
    }

    /// delete: traversal и «не простые имена» отклоняются; файл вне каталога
    /// бэкапов не трогается; валидное имя удаляется; несуществующий — NotFound.
    #[test]
    fn delete_rejects_traversal_and_bad_names() {
        let (_d, paths) = setup();
        put(&paths, "Good-100.zip", b"G");
        // «Жертва» traversal снаружи каталога бэкапов.
        let outside = paths.root().join("evil.zip");
        std::fs::write(long(&outside), b"E").unwrap();

        for bad in [
            "../evil.zip",
            "a/b.zip",
            "a\\b.zip",
            "sub\\Good-100.zip",
            "..",
            ".",
            "",
            "nozip",
            "Good-100.txt",
            "Good-100.zip.bak",
        ] {
            let err = delete(&paths, bad).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "имя {bad:?} отклонено: {err}");
        }
        assert!(long(&outside).exists(), "файл снаружи не тронут");
        assert!(exists(&paths, "Good-100.zip"), "чужие файлы не тронуты");

        delete(&paths, "Good-100.zip").unwrap();
        assert!(!exists(&paths, "Good-100.zip"), "валидный бэкап удалён");

        let err = delete(&paths, "Ghost-100.zip").unwrap_err();
        assert_eq!(err.code(), "not_found", "несуществующий — NotFound: {err}");
    }
}
