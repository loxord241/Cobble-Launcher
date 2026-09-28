//! Снапшоты mods/ перед «Обновить все» с откатом (F8, D37).
//!
//! Каталог `instance_dir/snapshots/`, файл `mods-<unixts>.zip` — файлы
//! верхнего уровня mods/ (включая `.disabled`, без подкаталогов). Хранятся
//! 3 новейших (ротация после create). Restore удаляет файлы, указанные
//! в манифесте контента (в обоих состояниях имени), и распаковывает zip
//! в mods/; сам манифест не трогает — файлы из снапшота, которых в
//! манифесте нет, остаются как есть.

use crate::errors::{LauncherError, Result};
use crate::instances::{instance_dir, load_content_manifest, minecraft_dir};
use crate::paths::Paths;
use std::io::Write as _;
use std::path::PathBuf;

/// Сколько снапшотов храним (ротация после create).
const KEEP_SNAPSHOTS: usize = 3;

/// Информация о снапшоте для UI (зеркало в src/api/types.ts).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    pub name: String,
    pub bytes: u64,
    pub created_at: i64,
}

/// Имя файла снапшота: `mods-` + 10–13 цифр (unix-секунды/миллисекунды) + `.zip`.
/// Единственный барьер от traversal через параметр `name` (A1-принцип).
fn is_snapshot_name(name: &str) -> bool {
    regex::Regex::new(r"^mods-[0-9]{10,13}\.zip$")
        .map(|re| re.is_match(name))
        .unwrap_or(false)
}

/// Метка времени из имени снапшота (после проверки is_snapshot_name — цифры).
fn snapshot_ts(name: &str) -> i64 {
    name.trim_start_matches("mods-")
        .trim_end_matches(".zip")
        .parse()
        .unwrap_or(0)
}

/// Каталог снапшотов инстанса.
fn snapshots_dir(paths: &Paths, instance_id: &str) -> PathBuf {
    instance_dir(paths, instance_id).join("snapshots")
}

/// Имена валидных снапшотов в каталоге (файлы по маске, без сортировки).
fn snapshot_names(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(crate::util::fs::long_path(dir)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| is_snapshot_name(n))
        .collect()
}

/// Создать снапшот mods/ (только файлы верхнего уровня, включая `.disabled`).
/// Возвращает имя файла. Guard «инстанс запущен» намеренно не ставится:
/// create вызывается прямо перед «Обновить все», а сама запись mods/ у
/// работающей игры блокируется в update_all (B9, InstanceRunning).
pub fn create(paths: &Paths, instance_id: &str) -> Result<String> {
    crate::instances::load(paths, instance_id)?; // valid_id + NotFound
    let dir = snapshots_dir(paths, instance_id);
    std::fs::create_dir_all(crate::util::fs::long_path(&dir))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = format!("mods-{ts}.zip");
    let dest = dir.join(&name);
    let file = std::fs::File::create(crate::util::fs::long_path(&dest))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions = Default::default();
    let mods = minecraft_dir(&instance_dir(paths, instance_id)).join("mods");
    let mods_long = crate::util::fs::long_path(&mods);
    let mut count = 0usize;
    if mods_long.exists() {
        for e in std::fs::read_dir(&mods_long)? {
            let e = e?;
            // Только файлы верхнего уровня; симлинки (file_type не следует) — мимо.
            if !e.file_type()?.is_file() {
                continue;
            }
            let fname = e.file_name().to_string_lossy().into_owned();
            zip.start_file(fname.clone(), opts)
                .map_err(|err| LauncherError::Zip(format!("start_file {fname}: {err}")))?;
            let data = std::fs::read(crate::util::fs::long_path(&e.path()))?;
            zip.write_all(&data)?;
            count += 1;
        }
    }
    zip.finish()
        .map_err(|e| LauncherError::Zip(format!("finish: {e}")))?;
    rotate(&dir)?;
    tracing::info!("снапшот mods/: {name} ({count} файлов)");
    Ok(name)
}

/// Ротация: оставить `KEEP_SNAPSHOTS` новейших снапшотов, остальные удалить.
/// Чужие файлы каталога (не по маске) не трогаем.
fn rotate(dir: &std::path::Path) -> Result<()> {
    let mut snaps = snapshot_names(dir);
    snaps.sort_by_key(|n| std::cmp::Reverse(snapshot_ts(n)));
    for stale in snaps.into_iter().skip(KEEP_SNAPSHOTS) {
        let p = crate::util::fs::long_path(&dir.join(&stale));
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    Ok(())
}

/// Список снапшотов инстанса, новейшие первыми. Нет каталога — пустой список.
pub fn list(paths: &Paths, instance_id: &str) -> Result<Vec<SnapshotInfo>> {
    crate::instances::load(paths, instance_id)?; // valid_id + NotFound
    let dir = snapshots_dir(paths, instance_id);
    let mut out = Vec::new();
    for name in snapshot_names(&dir) {
        let meta = std::fs::metadata(crate::util::fs::long_path(&dir.join(&name)))?;
        out.push(SnapshotInfo {
            created_at: snapshot_ts(&name),
            bytes: meta.len(),
            name,
        });
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.created_at));
    Ok(out)
}

/// Откатить mods/ к снапшоту: удалить файлы, УКАЗАННЫЕ в манифесте контента
/// (в активном и `.disabled` виде), затем распаковать zip в mods/. Манифест
/// не трогаем: файлы из снапшота, которых в манифесте нет, остаются как есть.
pub fn restore(paths: &Paths, instance_id: &str, name: &str) -> Result<()> {
    crate::instances::load(paths, instance_id)?; // valid_id + NotFound
    if !is_snapshot_name(name) {
        return Err(LauncherError::InvalidInput(format!(
            "некорректное имя снапшота: {name}"
        )));
    }
    // B9: mods/ у работающей игры занят — восстановление только после выхода.
    if crate::instances::running_pid(&instance_dir(paths, instance_id)).is_some() {
        return Err(LauncherError::InstanceRunning(instance_id.to_string()));
    }
    let dir = snapshots_dir(paths, instance_id);
    let zip_path = dir.join(name);
    if !crate::util::fs::long_path(&zip_path).exists() {
        return Err(LauncherError::NotFound(zip_path.to_string_lossy().into()));
    }
    let base = minecraft_dir(&instance_dir(paths, instance_id));
    // 1) Манифестные записи mods/ — удалить в обоих состояниях имени.
    for e in load_content_manifest(paths, instance_id)
        .iter()
        .filter(|e| e.file.starts_with("mods/"))
    {
        // Барьер traversal: имя записи читается с диска — валидируем как IPC-вход.
        crate::instances::content::resolve_content_file(paths, instance_id, &e.file)?;
        for enabled in [true, false] {
            let p = crate::util::fs::long_path(
                &base.join(crate::instances::content::disk_path(&e.file, enabled)),
            );
            if p.exists() {
                std::fs::remove_file(p)?;
            }
        }
    }
    // 2) Распаковать снапшот (записи — имена файлов в корне) внутрь mods/.
    //    Свой архив — доверенный вход, но zip-slip-защита внутри extract_zip.
    let f = std::fs::File::open(crate::util::fs::long_path(&zip_path))?;
    crate::util::zip::extract_zip(f, &base.join("mods"), |_p| {})?;
    tracing::info!("снапшот {name} восстановлен в mods/ инстанса {instance_id}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{
        save_content_manifest, ContentEntry, ContentKind, ContentSource, Instance,
    };

    fn long(p: &std::path::Path) -> PathBuf {
        crate::util::fs::long_path(p)
    }

    /// Фейковый Paths в tempdir + инстанс с файлами в mods/ (как в content.rs).
    fn setup() -> (tempfile::TempDir, Paths, Instance) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let inst = Instance::new("Тест", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        std::fs::create_dir_all(long(&mc.join("mods/sub"))).unwrap();
        std::fs::write(long(&mc.join("mods/a.jar")), b"AAA").unwrap();
        std::fs::write(long(&mc.join("mods/b.jar.disabled")), b"BBB").unwrap();
        std::fs::write(long(&mc.join("mods/sub/inner.jar")), b"IN").unwrap();
        (dir, paths, inst)
    }

    fn entry(file: &str) -> ContentEntry {
        ContentEntry {
            kind: ContentKind::Mod,
            file: file.into(),
            source: ContentSource::Modrinth,
            project_id: Some("aaa".into()),
            version_id: Some("v1".into()),
            sha1: Some("deadbeef".into()),
            url: None,
            enabled: true,
        }
    }

    fn zip_names<R: std::io::Read + std::io::Seek>(z: &mut zip::ZipArchive<R>) -> Vec<String> {
        (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().to_string())
            .collect()
    }

    /// create пишет zip с файлами mods/ верхнего уровня: включая `.disabled`,
    /// без подкаталогов; имя совпадает с маской.
    #[test]
    fn create_writes_top_level_mods_zip() {
        let (_d, paths, inst) = setup();
        let name = create(&paths, &inst.id).unwrap();
        assert!(is_snapshot_name(&name), "имя по маске: {name}");
        let f = std::fs::File::open(long(&snapshots_dir(&paths, &inst.id).join(&name))).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let mut names = zip_names(&mut z);
        names.sort();
        assert_eq!(names, vec!["a.jar", "b.jar.disabled"], "sub/inner.jar не берём");
    }

    /// Ротация: после create остаются ровно 3 новейших, чужие файлы не тронуты.
    #[test]
    fn create_rotates_snapshots_to_three() {
        let (_d, paths, inst) = setup();
        let dir = snapshots_dir(&paths, &inst.id);
        std::fs::create_dir_all(long(&dir)).unwrap();
        for ts in ["1000000000", "1000000001", "1000000002"] {
            std::fs::write(long(&dir.join(format!("mods-{ts}.zip"))), b"old").unwrap();
        }
        std::fs::write(long(&dir.join("readme.txt")), b"x").unwrap();
        let name = create(&paths, &inst.id).unwrap();
        let mut left = snapshot_names(&dir);
        left.sort();
        assert_eq!(left.len(), 3, "оставлены 3 новейших: {left:?}");
        assert!(left.contains(&name), "свежий снапшот на месте: {left:?}");
        assert!(!left.contains(&"mods-1000000000.zip".to_string()), "старейший удалён");
        assert!(dir.join("readme.txt").exists(), "чужие файлы ротации не подлежат");
    }

    /// list: новейшие первыми, размер и метка времени из имени.
    #[test]
    fn list_returns_newest_first() {
        let (_d, paths, inst) = setup();
        assert!(list(&paths, &inst.id).unwrap().is_empty(), "нет каталога — пусто");
        let dir = snapshots_dir(&paths, &inst.id);
        std::fs::create_dir_all(long(&dir)).unwrap();
        std::fs::write(long(&dir.join("mods-1000000000.zip")), [0u8; 10]).unwrap();
        std::fs::write(long(&dir.join("mods-2000000000.zip")), [0u8; 5]).unwrap();
        let all = list(&paths, &inst.id).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "mods-2000000000.zip");
        assert_eq!(all[0].bytes, 5);
        assert_eq!(all[0].created_at, 2_000_000_000);
        assert_eq!(all[1].created_at, 1_000_000_000);
    }

    /// restore возвращает манифестные файлы из снапшота (оба состояния имени);
    /// файлы вне манифеста остаются как есть; манифест не меняется.
    #[test]
    fn restore_returns_manifest_files_from_snapshot() {
        let (_d, paths, inst) = setup();
        save_content_manifest(&paths, &inst.id, &[entry("mods/a.jar")]).unwrap();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        std::fs::write(long(&mc.join("mods/manual.jar")), b"MANUAL").unwrap();
        let name = create(&paths, &inst.id).unwrap();

        // «Неудачное обновление»: манифестный файл перезаписан, лишний появился.
        std::fs::write(long(&mc.join("mods/a.jar")), b"CHANGED").unwrap();
        std::fs::write(long(&mc.join("mods/b.jar.disabled")), b"CHANGED2").unwrap();
        std::fs::write(long(&mc.join("mods/new.jar")), b"NEW").unwrap();

        restore(&paths, &inst.id, &name).unwrap();
        assert_eq!(
            std::fs::read(long(&mc.join("mods/a.jar"))).unwrap(),
            b"AAA",
            "манифестный файл восстановлен из снапшота"
        );
        assert_eq!(
            std::fs::read(long(&mc.join("mods/b.jar.disabled"))).unwrap(),
            b"BBB",
            "выключенный восстановлен из снапшота"
        );
        assert_eq!(
            std::fs::read(long(&mc.join("mods/manual.jar"))).unwrap(),
            b"MANUAL",
            "вне манифеста, но в снапшоте — вернулся"
        );
        assert_eq!(
            std::fs::read(long(&mc.join("mods/new.jar"))).unwrap(),
            b"NEW",
            "не из снапшота и не в манифесте — остаётся как есть"
        );
        assert_eq!(load_content_manifest(&paths, &inst.id).len(), 1, "манифест не тронут");
    }

    /// restore: чужое имя — InvalidInput, отсутствие снапшота — NotFound,
    /// работающий инстанс — InstanceRunning (B9).
    #[test]
    fn restore_guards_bad_name_missing_and_running() {
        let (_d, paths, inst) = setup();
        let name = create(&paths, &inst.id).unwrap();
        let err = restore(&paths, &inst.id, "mods-1111111111.zip").unwrap_err();
        assert_eq!(err.code(), "not_found");
        let err = restore(&paths, &inst.id, "../evil.zip").unwrap_err();
        assert_eq!(err.code(), "invalid_input");
        // Имитируем живой процесс: .lock с PID текущего теста.
        let dir = instance_dir(&paths, &inst.id);
        std::fs::write(dir.join(".lock"), std::process::id().to_string()).unwrap();
        let err = restore(&paths, &inst.id, &name).unwrap_err();
        assert_eq!(err.code(), "instance_running");
    }

    /// create отсутствующего инстанса — NotFound, traversal-id отклоняется.
    #[test]
    fn create_unknown_instance_not_found() {
        let (_d, paths, _inst) = setup();
        let err = create(&paths, "ghost").unwrap_err();
        assert_eq!(err.code(), "not_found");
        assert!(create(&paths, "../evil").is_err());
    }
}
