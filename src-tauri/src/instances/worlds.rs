//! Миры инстанса (F24, D37): список миров saves/, бэкап мира в zip,
//! удаление мира/Незера/Энда.
//!
//! Мир = каталог в saves/ с level.dat. Удаление — тем же стилем, что
//! instances::delete: в корзину ОС (восстановимо), а не remove_dir_all.

use crate::errors::{LauncherError, Result};
use crate::instances::{instance_dir, minecraft_dir, now_secs, running_pid, valid_id};
use crate::paths::Paths;
use crate::util::fs::long_path;
pub use crate::instances::content::ArchiveResult;
use std::path::{Path, PathBuf};

/// Объём удаления данных мира (F24): весь мир, только Незер (DIM-1)
/// или только Энд (DIM1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorldScope {
    All,
    Nether,
    End,
}

/// Сводка о мире из saves/ для UI (F24).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldInfo {
    pub name: String,
    pub last_modified: i64,
    pub size_bytes: u64,
    pub has_nether: bool,
    pub has_end: bool,
}

/// Каталог saves/ инстанса.
fn saves_dir(paths: &Paths, id: &str) -> PathBuf {
    minecraft_dir(&instance_dir(paths, id)).join("saves")
}

/// Мир = каталог с level.dat (иначе это мусор, а не мир — не показываем/не трогаем).
fn is_world_dir(path: &Path) -> bool {
    long_path(&path.join("level.dat")).is_file()
}

/// Имя мира (каталог в saves/): разрешено всё, кроме запрещённого —
/// 1–64 символа, без `<>:"/\|?*`, управляющих и последовательности `..`;
/// не `.`/`..`, края — не точка/пробел. Кириллица/CJK/эмодзи проходят
/// («Новый мир» — валидное имя). Путь остаётся внутри saves/:
/// разделители пути и traversal-имена отсечены. Regex не нужен — цикл по char.
fn valid_world_name(world: &str) -> bool {
    const FORBIDDEN: &str = "<>:\"/\\|?*";
    if !(1..=64).contains(&world.chars().count()) {
        return false;
    }
    if world == "." || world == ".." || world.contains("..") {
        return false;
    }
    if world.starts_with('.') || world.starts_with(' ') {
        return false;
    }
    if world.ends_with('.') || world.ends_with(' ') {
        return false;
    }
    !world.chars().any(|c| FORBIDDEN.contains(c) || c.is_control())
}

/// world → каталог saves/<world>: InvalidInput на плохом имени, NotFound,
/// если каталога с level.dat нет.
fn resolve_world(paths: &Paths, id: &str, world: &str) -> Result<PathBuf> {
    if !valid_world_name(world) {
        return Err(LauncherError::InvalidInput(format!(
            "некорректное имя мира: {world:?} (запрещены <>:\"/\\|?* и управляющие символы, до 64)"
        )));
    }
    let dir = saves_dir(paths, id).join(world);
    if !is_world_dir(&dir) {
        return Err(LauncherError::NotFound(format!("мир {world}")));
    }
    Ok(dir)
}

/// Размер каталога рекурсивно (сумма файлов). Симлинки не обходим:
/// считаем только размер самой ссылки — иначе цикл или чужое дерево.
fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(long_path(path)) else {
        return 0;
    };
    let mut total = 0u64;
    for e in entries.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            total += dir_size(&e.path());
        } else if ft.is_file() {
            total += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

/// modified level.dat в секундах UNIX (0, если недоступно — мир всё равно показываем).
fn level_dat_modified(world_dir: &Path) -> i64 {
    std::fs::metadata(long_path(&world_dir.join("level.dat")))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Список миров инстанса (F24): каталоги saves/ с level.dat, по имени.
/// Нет saves/ — пустой список (новый инстанс, не ошибка).
pub fn list_worlds(paths: &Paths, instance_id: &str) -> Result<Vec<WorldInfo>> {
    valid_id(instance_id)?;
    let saves = saves_dir(paths, instance_id);
    let Ok(entries) = std::fs::read_dir(long_path(&saves)) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir || !is_world_dir(&path) {
            continue;
        }
        out.push(WorldInfo {
            name: e.file_name().to_string_lossy().into_owned(),
            has_nether: long_path(&path.join("DIM-1")).is_dir(),
            has_end: long_path(&path.join("DIM1")).is_dir(),
            last_modified: level_dat_modified(&path),
            size_bytes: dir_size(&path),
        });
    }
    out.sort_by_key(|w| w.name.to_lowercase());
    Ok(out)
}

/// Бэкап мира (F24): saves/<world> целиком (включая DIM-1/DIM1) в zip
/// `backups/worlds/<мир>-<unixts>.zip` (рядом с бэкапами инстанса,
/// paths.backups_dir()). Работающий инстанс блокирует операцию.
pub fn backup_world(paths: &Paths, instance_id: &str, world: &str) -> Result<ArchiveResult> {
    valid_id(instance_id)?;
    if running_pid(&instance_dir(paths, instance_id)).is_some() {
        return Err(LauncherError::InstanceRunning(instance_id.to_string()));
    }
    let world_dir = resolve_world(paths, instance_id, world)?;
    let dest_dir = paths.backups_dir().join("worlds");
    std::fs::create_dir_all(long_path(&dest_dir))?;
    let dest = dest_dir.join(format!("{world}-{}.zip", now_secs()));

    // D64: пишем в соседний `.part` и переименовываем атомарно. Прежний
    // File::create(dest) при коллизии «та же секунда» усекал бы готовый архив
    // того же мира; крах посреди записи не трогает уже существующий бэкап.
    let part = crate::util::fs::part_path(&dest);
    let zipped = (|| -> Result<usize> {
        let file = std::fs::File::create(long_path(&part))?;
        let mut zip = zip::ZipWriter::new(file);
        // Префикс — имя каталога мира: распаковка архива прямо в saves/
        // восстанавливает мир под своим именем.
        let count = crate::util::zip::write_zip_dir(
            &world_dir,
            &mut zip,
            &format!("{world}/"),
            &|_| false,
        )?;
        zip.finish()
            .map_err(|e| LauncherError::Zip(format!("finish: {e}")))?;
        Ok(count)
    })();
    match zipped {
        Ok(count) => {
            // A26: rename с повторами на случай придержания архива антивирусом.
            crate::util::fs::rename_with_retry(&part, &dest)?;
            let bytes = std::fs::metadata(long_path(&dest))?.len();
            Ok(ArchiveResult {
                path: dest.to_string_lossy().into(),
                files: count,
                bytes,
            })
        }
        Err(e) => {
            // Недописанный `.part` не оставляем рядом с бэкапами (best-effort).
            crate::util::fs::remove_file_ignore(&part);
            Err(e)
        }
    }
}

/// Удаление данных мира (F24): All — весь каталог мира, Nether — только
/// DIM-1, End — только DIM1. Как instances::delete — в корзину ОС
/// (восстановимо). Работающий инстанс и отсутствие мира/измерения — ошибки.
pub fn delete_world_data(
    paths: &Paths,
    instance_id: &str,
    world: &str,
    scope: WorldScope,
) -> Result<()> {
    valid_id(instance_id)?;
    if running_pid(&instance_dir(paths, instance_id)).is_some() {
        return Err(LauncherError::InstanceRunning(instance_id.to_string()));
    }
    match scope {
        WorldScope::All => {
            let world_dir = resolve_world(paths, instance_id, world)?;
            super::recycle_dir(&world_dir)
        }
        WorldScope::Nether | WorldScope::End => {
            let dim = if scope == WorldScope::Nether { "DIM-1" } else { "DIM1" };
            let world_dir = resolve_world(paths, instance_id, world)?;
            let target = world_dir.join(dim);
            if !long_path(&target).is_dir() {
                return Err(LauncherError::NotFound(format!("{dim} мира {world}")));
            }
            super::recycle_dir(&target)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::Instance;

    fn setup() -> (tempfile::TempDir, Paths, Instance) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let inst = Instance::new("Тест", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        (dir, paths, inst)
    }

    fn long(p: &Path) -> PathBuf {
        long_path(p)
    }

    /// Фейковый мир в saves/ инстанса: level.dat + опциональные измерения.
    fn make_world(paths: &Paths, id: &str, name: &str, nether: bool, end: bool) -> PathBuf {
        let w = saves_dir(paths, id).join(name);
        std::fs::create_dir_all(long(&w.join("data"))).unwrap();
        std::fs::write(long(&w.join("level.dat")), b"W").unwrap();
        if nether {
            std::fs::create_dir_all(long(&w.join("DIM-1/regions"))).unwrap();
            std::fs::write(long(&w.join("DIM-1/regions/r.0")), b"N").unwrap();
        }
        if end {
            std::fs::create_dir_all(long(&w.join("DIM1/regions"))).unwrap();
            std::fs::write(long(&w.join("DIM1/regions/r.0")), b"E").unwrap();
        }
        w
    }

    fn zip_names(res: &ArchiveResult) -> Vec<String> {
        let f = std::fs::File::open(long(Path::new(&res.path))).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().to_string())
            .collect()
    }

    #[test]
    fn list_empty_without_saves() {
        let (_d, paths, inst) = setup();
        assert!(list_worlds(&paths, &inst.id).unwrap().is_empty());
    }

    #[test]
    fn list_two_worlds_skips_junk_and_flags_dimensions() {
        let (_d, paths, inst) = setup();
        make_world(&paths, &inst.id, "w1", true, false);
        make_world(&paths, &inst.id, "w2", false, true);
        // Мусор: каталог без level.dat и файл в saves — миром не считаются.
        std::fs::create_dir_all(long(&saves_dir(&paths, &inst.id).join("junk"))).unwrap();
        std::fs::write(long(&saves_dir(&paths, &inst.id).join("stray.zip")), b"z").unwrap();

        let worlds = list_worlds(&paths, &inst.id).unwrap();
        assert_eq!(worlds.len(), 2, "только каталоги с level.dat");
        assert_eq!(worlds[0].name, "w1");
        assert_eq!(worlds[1].name, "w2");
        assert!(worlds[0].has_nether && !worlds[0].has_end);
        assert!(!worlds[1].has_nether && worlds[1].has_end);
        // Размер: level.dat (1) + DIM-1/regions/r.0 (1) = 2 байта.
        assert_eq!(worlds[0].size_bytes, 2);
        assert!(worlds[0].last_modified > 0, "modified level.dat прочитан");
    }

    /// A1: некорректный id инстанса отклоняется — traversal невозможен.
    #[test]
    fn list_rejects_bad_instance_id() {
        let (_d, paths, _inst) = setup();
        for id in ["../x", "a/b", ""] {
            let err = list_worlds(&paths, id).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "id {id:?} отклонён");
        }
    }

    /// Имя мира валидируется ДО обращения к диску: traversal, пустое,
    /// слишком длинное, запрещённые символы/`..`/краевые точки-пробелы —
    /// InvalidInput. Кириллица, точка внутри и эмодзи допустимы; валидные
    /// имена проходят дальше и дают NotFound (мира нет).
    #[test]
    fn world_name_validation() {
        let (_d, paths, inst) = setup();
        let too_long = "x".repeat(65);
        // 65 кириллических символов: отсекается по числу СИМВОЛОВ, не байт.
        let too_long_cyr = "м".repeat(65);
        for bad in [
            "../evil",
            "a/b",
            r"a\b",
            "",
            "a:b",
            "мир: тест",
            "мир..тест",
            ".",
            "..",
            "мир ",   // пробел в конце
            " мир",   // пробел в начале
            ".мир",   // точка в начале
            "мир.",   // точка в конце
            "a\u{7}b", // управляющий символ
            too_long.as_str(),
            too_long_cyr.as_str(),
        ] {
            let err = backup_world(&paths, &inst.id, bad).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "имя {bad:?} отклонено");
        }
        for good in [
            "New World",
            "world_1",
            "a",
            "A-b C9",
            "x".repeat(64).as_str(),
            // Кириллица/эмодзи/точка внутри имени — валидны.
            "Новый мир",
            "мир",
            "my.world",
            "мир 🌍",
            "м".repeat(64).as_str(), // 64 кириллических символа = 128 байт, но 64 char
        ] {
            let err = backup_world(&paths, &inst.id, good).unwrap_err();
            assert_eq!(err.code(), "not_found", "имя {good:?} допустимо, мира нет");
        }
    }

    #[test]
    fn backup_zips_world_with_dimensions() {
        let (_d, paths, inst) = setup();
        make_world(&paths, &inst.id, "w1", true, true);
        let res = backup_world(&paths, &inst.id, "w1").unwrap();
        assert!(res.files >= 3, "level.dat + оба DIM: {}", res.files);
        assert!(res.bytes > 0);
        // Лежит в backups/worlds/ с именем мира и штампом.
        let dest = Path::new(&res.path);
        assert!(dest.parent().unwrap().ends_with(Path::new("backups").join("worlds")));
        assert!(dest.file_name().unwrap().to_string_lossy().starts_with("w1-"));
        // Архив содержит мир целиком под префиксом имени каталога.
        let names = zip_names(&res);
        assert!(names.iter().any(|n| n == "w1/level.dat"));
        assert!(names.iter().any(|n| n == "w1/DIM-1/regions/r.0"));
        assert!(names.iter().any(|n| n == "w1/DIM1/regions/r.0"));
    }

    /// D64: запись архива идёт через `.part` + атомарный rename — два бэкапа
    /// одного мира за одну секунду не усекают готовый архив, временных файлов
    /// рядом с бэкапами не остаётся.
    #[test]
    fn backup_twice_leaves_no_part_and_valid_zips() {
        let (_d, paths, inst) = setup();
        make_world(&paths, &inst.id, "w1", true, false);
        let r1 = backup_world(&paths, &inst.id, "w1").unwrap();
        let r2 = backup_world(&paths, &inst.id, "w1").unwrap();

        let dest_dir = paths.backups_dir().join("worlds");
        let no_part = std::fs::read_dir(long(&dest_dir))
            .unwrap()
            .filter_map(|e| e.ok())
            .all(|e| !e.file_name().to_string_lossy().ends_with(".part"));
        assert!(no_part, "`.part` не остаётся рядом с бэкапами");
        // Оба архива валидны (ZipArchive проверяет центральный каталог).
        assert!(!zip_names(&r1).is_empty());
        assert!(!zip_names(&r2).is_empty());
    }

    #[test]
    fn backup_missing_world_is_not_found() {
        let (_d, paths, inst) = setup();
        let err = backup_world(&paths, &inst.id, "ghost").unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    /// Работающий инстанс (живой PID в .lock — свой PID теста) блокирует
    /// бэкап и удаление кодом instance_running; чтение списка разрешено.
    #[test]
    fn running_instance_blocks_backup_and_delete() {
        let (_d, paths, inst) = setup();
        make_world(&paths, &inst.id, "w1", false, false);
        let dir = instance_dir(&paths, &inst.id);
        std::fs::write(dir.join(".lock"), std::process::id().to_string()).unwrap();

        let err = backup_world(&paths, &inst.id, "w1").unwrap_err();
        assert_eq!(err.code(), "instance_running");
        let err = delete_world_data(&paths, &inst.id, "w1", WorldScope::All).unwrap_err();
        assert_eq!(err.code(), "instance_running");
        let err = delete_world_data(&paths, &inst.id, "w1", WorldScope::Nether).unwrap_err();
        assert_eq!(err.code(), "instance_running");
        assert!(list_worlds(&paths, &inst.id).is_ok(), "чтение не блокируется");

        let _ = std::fs::remove_file(dir.join(".lock"));
        assert!(backup_world(&paths, &inst.id, "w1").is_ok(), "после снятия лока можно");
    }

    #[test]
    fn delete_nether_keeps_end_and_level() {
        let (_d, paths, inst) = setup();
        let w = make_world(&paths, &inst.id, "w1", true, true);
        delete_world_data(&paths, &inst.id, "w1", WorldScope::Nether).unwrap();
        assert!(!long(&w.join("DIM-1")).exists(), "Незер удалён");
        assert!(long(&w.join("DIM1")).exists(), "Энд не тронут");
        assert!(long(&w.join("level.dat")).exists(), "сам мир не тронут");
        let worlds = list_worlds(&paths, &inst.id).unwrap();
        assert_eq!(worlds.len(), 1);
        assert!(!worlds[0].has_nether && worlds[0].has_end);

        // Удаление Энда работает; повторное — NotFound (измерения больше нет).
        delete_world_data(&paths, &inst.id, "w1", WorldScope::End).unwrap();
        assert!(!long(&w.join("DIM1")).exists());
        assert!(long(&w.join("level.dat")).exists(), "мир остался без измерений");
        let err = delete_world_data(&paths, &inst.id, "w1", WorldScope::End).unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn delete_all_removes_whole_world_dir() {
        let (_d, paths, inst) = setup();
        let w = make_world(&paths, &inst.id, "w1", true, true);
        make_world(&paths, &inst.id, "w2", false, false);
        delete_world_data(&paths, &inst.id, "w1", WorldScope::All).unwrap();
        assert!(!long(&w).exists(), "каталог мира удалён");
        assert!(long(&saves_dir(&paths, &inst.id).join("w2")).exists(), "соседний мир не тронут");
        assert!(list_worlds(&paths, &inst.id).unwrap().iter().all(|x| x.name == "w2"));
        // Повторное удаление — NotFound.
        let err = delete_world_data(&paths, &inst.id, "w1", WorldScope::All).unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    /// Контракт IPC: скоуп приходит из UI строкой в нижнем регистре.
    #[test]
    fn scope_serde_lowercase() {
        for (text, expected) in [
            ("all", WorldScope::All),
            ("nether", WorldScope::Nether),
            ("end", WorldScope::End),
        ] {
            let scope: WorldScope = serde_json::from_str(&format!("\"{text}\"")).unwrap();
            assert_eq!(scope, expected);
            assert_eq!(serde_json::to_string(&scope).unwrap(), format!("\"{text}\""));
        }
        assert!(serde_json::from_str::<WorldScope>("\"All\"").is_err(), "только lowercase");
    }
}
