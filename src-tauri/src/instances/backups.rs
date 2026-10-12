//! Реестр бэкапов инстансов (F4): список для UI, удаление, ротация,
//! восстановление конкретного бэкапа (D72-хвост).
//!
//! Бэкапы — zip в `backups/` (Paths::backups_dir) с именем
//! «{имя инстанса}-{unix-сек}.zip»; создаёт их content::backup. Раньше
//! они копились бесконечно: теперь ядро умеет перечислять их для UI,
//! удалять по имени (с барьером traversal) и ронять старейшие сверх
//! лимита после каждого нового бэкапа (ротацию вызывает commands-обёртка,
//! т.к. слою instances настройки недоступны). restore() заменяет содержимое
//! инстанса архивом целиком (цель — по id из instance.json внутри бэкапа,
//! удалённый инстанс создаётся заново) с авто-бэкапом текущего состояния.

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

/// Валидировать имя файла бэкапа и вернуть канонизированный путь к нему.
/// `file` обязан быть ПРОСТЫМ именем (без `/`, `\`, `..`), с расширением
/// .zip, и реально лежать в каталоге бэкапов: путь канонизируется и сверяется
/// префиксом с канонизированным backups_dir — traversal и симлинк наружу
/// недопустимы (A1). Общий барьер delete и restore.
fn validated_backup_path(paths: &Paths, file: &str) -> Result<std::path::PathBuf> {
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
    Ok(target_real)
}

pub fn delete(paths: &Paths, file: &str) -> Result<()> {
    let target_real = validated_backup_path(paths, file)?;
    std::fs::remove_file(target_real)?;
    Ok(())
}

/// Прочитать instance.json из бэкапа (запись `instance/instance.json`) —
/// он определяет цель восстановления (id) и несёт состояние инстанса
/// на момент бэкапа. Чужой/битый архив — честный InvalidInput.
fn read_archived_instance(zip_path: &std::path::Path) -> Result<crate::instances::Instance> {
    let f = std::fs::File::open(crate::util::fs::long_path(zip_path))?;
    let mut archive = zip::ZipArchive::new(f)
        .map_err(|e| LauncherError::Zip(format!("не удалось открыть архив: {e}")))?;
    let mut entry = archive
        .by_name("instance/instance.json")
        .map_err(|_| {
            LauncherError::InvalidInput(
                "в архиве нет instance/instance.json — это не бэкап инстанса".into(),
            )
        })?;
    // instance.json — маленький JSON; гигантская «запись» — не наш формат.
    if entry.size() > 1_000_000 {
        return Err(LauncherError::InvalidInput(
            "запись instance/instance.json аномально велика".into(),
        ));
    }
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut buf)?;
    serde_json::from_slice(&buf)
        .map_err(|e| LauncherError::InvalidInput(format!("битый instance.json в бэкапе: {e}")))
}

/// id инстанса, зашитый в бэкапе (командный слой гардит фазы по этому id
/// до тяжёлой дисковой работы restore).
pub fn archived_instance_id(paths: &Paths, file: &str) -> Result<String> {
    let zip_path = validated_backup_path(paths, file)?;
    if parse_backup_name(file).is_none() {
        return Err(LauncherError::InvalidInput(format!(
            "имя не по формату бэкапа «{{инстанс}}-{{штамп}}.zip»: {file:?}"
        )));
    }
    Ok(read_archived_instance(&zip_path)?.id)
}

/// Хвосты прошлых попыток восстановления (`tmp/restore-*`): без `old/` —
/// мусор, убираем; с `old/` — след НЕСРАБОТАВШЕГО отката, там пользовательские
/// данные, не трогаем (жалко байт, но потерять мир — хуже).
fn cleanup_stale_restore_dirs(paths: &Paths) {
    let tmp = paths.root().join("tmp");
    let Ok(entries) = std::fs::read_dir(crate::util::fs::long_path(&tmp)) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if !name.starts_with("restore-") {
            continue;
        }
        if crate::util::fs::long_path(&p.join("old")).exists() {
            tracing::warn!("tmp/{name} хранит несработавший откат восстановления — не трогаем");
            continue;
        }
        let _ = std::fs::remove_dir_all(crate::util::fs::long_path(&p));
    }
}

/// Восстановить бэкап по имени файла (кнопка «Восстановить» в реестре).
/// Цель определяется по id из архива (instance.json), а не по имени в имени
/// файла: инстанс с этим id заменяется содержимым бэкапа ЦЕЛИКОМ (замена
/// каталога через rename, не распаковка поверх — иначе слились бы старые и
/// новые файлы); если инстанса нет — он создаётся заново (вернуть удалённый).
/// Перед заменой существующего читаемого состояния делается обычный бэкап
/// (content::backup) — операция обратима через тот же список бэкапов.
/// Работающий инстанс — отказ InstanceRunning и до, и внутри замены.
pub fn restore(paths: &Paths, file: &str) -> Result<crate::instances::Instance> {
    let zip_path = validated_backup_path(paths, file)?;
    if parse_backup_name(file).is_none() {
        return Err(LauncherError::InvalidInput(format!(
            "имя не по формату бэкапа «{{инстанс}}-{{штамп}}.zip»: {file:?}"
        )));
    }
    let archived = read_archived_instance(&zip_path)?;
    super::valid_id(&archived.id)?;
    let target = super::instance_dir(paths, &archived.id);
    if super::running_pid(&target).is_some() {
        return Err(LauncherError::InstanceRunning(archived.id.clone()));
    }
    cleanup_stale_restore_dirs(paths);

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let work = paths.root().join("tmp").join(format!(
        "restore-{}-{}",
        stamp,
        std::process::id()
    ));
    std::fs::create_dir_all(crate::util::fs::long_path(&work))?;

    // Замена выполнена (или откат состоялся) — рабочую папку можно сносить;
    // несработавший откат оставляет `old` и запрещает уборку.
    let mut safe_to_clean = true;
    let outcome = (|| -> Result<crate::instances::Instance> {
        let f = std::fs::File::open(crate::util::fs::long_path(&zip_path))?;
        let extracted = work.join("instance");
        crate::util::zip::extract_zip_stripping(f, &extracted, "instance/", |_| true, |_| {})?;
        if !crate::util::fs::long_path(&extracted.join("instance.json")).exists() {
            return Err(LauncherError::InvalidInput(
                "в архиве нет instance/instance.json — это не бэкап инстанса".into(),
            ));
        }
        if target.exists() {
            // Страховка: текущее состояние — обычным бэкапом (ротация applies).
            match super::load(paths, &archived.id) {
                Ok(current) => {
                    super::content::backup(paths, &current)?;
                }
                Err(e) => tracing::warn!(
                    "restore: instance.json цели не читается ({e}) — замена без авто-бэкапа"
                ),
            }
            let old = work.join("old");
            crate::util::fs::rename_with_retry(&target, &old)?;
            if let Err(e) = crate::util::fs::rename_with_retry(&extracted, &target) {
                // Откат: вернуть прежний каталог на место; если не выйдет —
                // данные остаются под work/old, уборка запрещена.
                if let Err(re) = crate::util::fs::rename_with_retry(&old, &target) {
                    safe_to_clean = false;
                    tracing::error!(
                        "restore: откат не удался ({re}); прежний каталог: {}",
                        old.display()
                    );
                }
                return Err(e);
            }
            // Прежний каталог уже спасён авто-бэкапом — мусор убираем. Сбой
            // (занятый антивирусом файл) не отменяет восстановление.
            if let Err(e) = std::fs::remove_dir_all(crate::util::fs::long_path(&old)) {
                tracing::warn!("restore: не убран прежний каталог {}: {e}", old.display());
            }
        } else {
            std::fs::create_dir_all(crate::util::fs::long_path(&paths.instances_dir()))?;
            crate::util::fs::rename_with_retry(&extracted, &target)?;
        }
        super::load(paths, &archived.id)
    })();

    if safe_to_clean {
        let _ = std::fs::remove_dir_all(crate::util::fs::long_path(&work));
    }
    outcome
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

    /// Хелпер restore-тестов: инстанс с модом и миром на диске.
    fn setup_with_instance() -> (tempfile::TempDir, Paths, crate::instances::Instance) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let inst = crate::instances::Instance::new("Тест", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        let mc = crate::instances::minecraft_dir(&crate::instances::instance_dir(
            &paths,
            &inst.id,
        ));
        std::fs::create_dir_all(long(&mc.join("mods"))).unwrap();
        std::fs::create_dir_all(long(&mc.join("saves/w1"))).unwrap();
        std::fs::write(long(&mc.join("mods/a.jar")), b"OLD").unwrap();
        std::fs::write(long(&mc.join("saves/w1/level.dat")), b"W1").unwrap();
        (dir, paths, inst)
    }

    /// restore: замена ЦЕЛИКОМ (не распаковка поверх) — старое состояние
    /// вытеснено, новое дописывание не смешано; авто-бэкап текущего появился
    /// в реестре.
    #[test]
    fn restore_replaces_state_whole_and_makes_safety_backup() {
        let (_d, paths, inst) = setup_with_instance();
        let saved = crate::instances::content::backup(&paths, &inst).unwrap();
        let file = std::path::Path::new(&saved.path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        // Текущее состояние ушло вперёд: мод переписан, добавлен второй.
        let mc = crate::instances::minecraft_dir(&crate::instances::instance_dir(
            &paths,
            &inst.id,
        ));
        std::fs::write(long(&mc.join("mods/a.jar")), b"NEW").unwrap();
        std::fs::write(long(&mc.join("mods/b.jar")), b"B").unwrap();

        let restored = restore(&paths, &file).unwrap();
        assert_eq!(restored.id, inst.id);
        assert_eq!(restored.name, inst.name, "состояние бэкапа вернулось");

        assert_eq!(
            std::fs::read(long(&mc.join("mods/a.jar"))).unwrap(),
            b"OLD",
            "мод из бэкапа вернулся"
        );
        assert!(
            !long(&mc.join("mods/b.jar")).exists(),
            "появившийся после бэкапа мод вытеснен целиком, не смешан"
        );
        assert_eq!(
            std::fs::read(long(&mc.join("saves/w1/level.dat"))).unwrap(),
            b"W1"
        );

        // Авто-бэкап: в реестре два архива этого инстанса — исходный и
        // страховочный (новейший).
        let mine: Vec<BackupEntry> = list(&paths)
            .into_iter()
            .filter(|e| e.instance == "Тест")
            .collect();
        assert_eq!(mine.len(), 2, "страховочный бэкап появился: {mine:?}");
        assert!(
            std::fs::read(long(&paths.backups_dir().join(&mine[0].file)))
                .map(|b| !b.is_empty())
                .unwrap_or(false),
            "страховочный архив не пуст"
        );
    }

    /// restore: инстанс удалён — создаётся заново из архива (вернуть
    /// случайно удалённый инстанс), с тем же id и появляется в списке.
    #[test]
    fn restore_recreates_deleted_instance() {
        let (_d, paths, inst) = setup_with_instance();
        let saved = crate::instances::content::backup(&paths, &inst).unwrap();
        let file = std::path::Path::new(&saved.path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        std::fs::remove_dir_all(long(&crate::instances::instance_dir(&paths, &inst.id))).unwrap();
        assert!(crate::instances::list(&paths).is_empty());

        let restored = restore(&paths, &file).unwrap();
        assert_eq!(restored.id, inst.id);
        assert_eq!(restored.name, "Тест");
        assert_eq!(
            crate::instances::list(&paths).len(),
            1,
            "инстанс снова в списке"
        );
        let mc = crate::instances::minecraft_dir(&crate::instances::instance_dir(
            &paths,
            &inst.id,
        ));
        assert_eq!(std::fs::read(long(&mc.join("mods/a.jar"))).unwrap(), b"OLD");
    }

    /// restore: работающий инстанс — отказ instance_running; мусорные имена
    /// и чужие архивы отклоняются; ничего не заменяется.
    #[test]
    fn restore_guards_running_and_bad_input() {
        let (_d, paths, inst) = setup_with_instance();
        let saved = crate::instances::content::backup(&paths, &inst).unwrap();
        let file = std::path::Path::new(&saved.path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        // Работающий: .lock с живым PID.
        std::fs::write(
            long(&crate::instances::instance_dir(&paths, &inst.id).join(".lock")),
            std::process::id().to_string(),
        )
        .unwrap();
        let err = restore(&paths, &file).unwrap_err();
        assert_eq!(err.code(), "instance_running", "{err}");
        assert_eq!(
            std::fs::read(
                long(&crate::instances::minecraft_dir(&crate::instances::instance_dir(
                    &paths, &inst.id
                ))
                .join("mods/a.jar"))
            )
            .unwrap(),
            b"OLD",
            "состояние не тронуто отказом"
        );
        std::fs::remove_file(
            long(&crate::instances::instance_dir(&paths, &inst.id).join(".lock")),
        )
        .unwrap();

        // Чужой архив без instance/instance.json — InvalidInput.
        let foreign = paths.backups_dir().join("X-100.zip");
        let f = std::fs::File::create(long(&foreign)).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = Default::default();
        w.start_file("other/file.txt", opts).unwrap();
        use std::io::Write as _;
        w.write_all(b"x").unwrap();
        w.finish().unwrap();
        let err = restore(&paths, "X-100.zip").unwrap_err();
        assert_eq!(err.code(), "invalid_input", "{err}");

        // Не zip вовсе, но имя по формату — ошибка распаковки/парса.
        std::fs::write(long(&paths.backups_dir().join("Y-100.zip")), b"garbage").unwrap();
        assert!(restore(&paths, "Y-100.zip").is_err());

        // Traversal и не-простые имена — тот же барьер, что у delete.
        for bad in ["../evil.zip", "a/b.zip", "a\\b.zip", "", "nozip"] {
            let err = restore(&paths, bad).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "имя {bad:?}: {err}");
        }
        // Не формат «{инстанс}-{штамп}.zip» — отклоняется, хоть файл и валиден.
        std::fs::write(long(&paths.backups_dir().join("plain.zip")), b"z").unwrap();
        let err = restore(&paths, "plain.zip").unwrap_err();
        assert_eq!(err.code(), "invalid_input", "{err}");
    }

    /// archived_instance_id: совпадает с id инстанса в бэкапе; для мусора —
    /// отказ (командный слой гардит фазы по этому id до тяжёлой работы).
    #[test]
    fn archived_instance_id_reads_backup_target() {
        let (_d, paths, inst) = setup_with_instance();
        let saved = crate::instances::content::backup(&paths, &inst).unwrap();
        let file = std::path::Path::new(&saved.path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(archived_instance_id(&paths, &file).unwrap(), inst.id);
        assert!(archived_instance_id(&paths, "plain.zip").is_err());
    }
}
