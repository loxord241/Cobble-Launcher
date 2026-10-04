//! Конфиги модов: список/чтение/запись (F29, D37-C). Агент C4.
//!
//! Каталог — `minecraft/config` инстанса. rel — относительный путь внутри
//! него с ЖЁСТКОЙ валидацией (path traversal — главный риск, серия A):
//! компоненты только `[A-Za-z0-9._-]`, разделитель `/`, максимум 4 уровня,
//! компонент не начинается с точки — значит `.`, `..`, скрытые файлы и
//! любые `\` `:` не проходят ни в каком виде.

use crate::errors::{LauncherError, Result};
use crate::instances::{instance_dir, minecraft_dir, running_pid, valid_id};
use crate::paths::Paths;
use crate::util::fs::{atomic_write, long_path};
use std::path::{Path, PathBuf};

/// Лимит чтения/записи одного конфига: больше — честный InvalidInput
/// (редактор такой файл не тянет, UI показывает подсказку «открой в проводнике»).
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Максимум файлов в списке: дальшие игнорируются с warn (защита от
/// гигантских/мусорных деревьев config/).
const MAX_FILES: usize = 500;

/// Глубина рекурсии list и число уровней rel: ≤4 компонентов.
const MAX_LEVELS: usize = 4;

/// Максимальная длина одного компонента rel.
const MAX_COMPONENT_LEN: usize = 128;

/// Файл конфига мода для UI (зеркало `ConfigFile` в src/api/types.ts).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    pub rel: String,
    pub bytes: u64,
}

/// Каталог `config/` инстанса.
fn configs_dir(paths: &Paths, instance_id: &str) -> PathBuf {
    minecraft_dir(&instance_dir(paths, instance_id)).join("config")
}

/// ЖЁСТКАЯ валидация rel (по образцу mclogs::is_valid_log_name — байтовый
/// эквивалент регекса): 1–4 компонента, каждый `[A-Za-z0-9._-]{1,128}` и
/// не начинается с точки. Точка в середине допустима (`a..b.txt` — обычное
/// имя), ведущая — нет: отсекает `.`, `..` и скрытые файлы.
fn valid_rel(rel: &str) -> bool {
    let components: Vec<&str> = rel.split('/').collect();
    // «Максимум 4 уровня» = 4 компонента пути (config/xxx.json — 2, глубже).
    if components.len() > MAX_LEVELS {
        return false;
    }
    components.into_iter().all(valid_component)
}

/// Один компонент пути: непустой, ≤128, без ведущей точки, только
/// латиница/цифры/`.`/`_`/`-`. Разделителей (`/`, `\`) и `:` нет по набору.
fn valid_component(c: &str) -> bool {
    !c.is_empty()
        && c.len() <= MAX_COMPONENT_LEN
        && !c.starts_with('.')
        && c.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// rel → путь внутри config/: InvalidInput на невалидном rel (до диска).
fn resolve_rel(paths: &Paths, instance_id: &str, rel: &str) -> Result<PathBuf> {
    valid_id(instance_id)?;
    if !valid_rel(rel) {
        return Err(LauncherError::InvalidInput(format!(
            "некорректный путь конфига: {rel:?} (1–4 уровня из [A-Za-z0-9._-], без ведущей точки)"
        )));
    }
    Ok(configs_dir(paths, instance_id).join(rel))
}

/// Рекурсивный обход config/ (только файлы, подкаталоги до 4 уровней).
/// `prefix` — rel текущего каталога относительно корня ("" — корень),
/// `depth` — глубина файлов в нём (1 для корня). Файлы с именами, не
/// проходящими valid_rel, пропускаются: их всё равно не открыть, а один
/// мусорный файл не должен ломать весь список (как в mclogs::list_archived).
/// Возвращает false, если достигнут лимит MAX_FILES (обход остановлен).
fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<ConfigFile>) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return true; // нет каталога/нет доступа — просто пусто
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            tracing::warn!("config/: больше {MAX_FILES} файлов, дальшие скрыты");
            return false;
        }
        let Ok(ft) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if ft.is_dir() {
            // Каталог на 4-м уровне не входим: его файлы были бы 5-м уровнем rel.
            if depth >= MAX_LEVELS {
                continue;
            }
            if !walk(&entry.path(), &rel, depth + 1, out) {
                return false; // лимит файлов достигнут в глубине
            }
        } else if ft.is_file() && valid_rel(&rel) {
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(ConfigFile { rel, bytes });
        }
    }
    true
}

/// Список конфигов инстанса (F29): файлы `config/` рекурсивно (≤4 уровней,
/// ≤500 — дальшие игнорируются с warn), размеры, сортировка по rel.
/// Нет config/ — пустой список (моды ещё не создавали конфиги, не ошибка).
pub fn list(paths: &Paths, instance_id: &str) -> Result<Vec<ConfigFile>> {
    valid_id(instance_id)?;
    let root = configs_dir(paths, instance_id);
    let mut out = Vec::new();
    walk(&long_path(&root), "", 1, &mut out);
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

/// Прочитать конфиг: cap 1 МБ (больше — InvalidInput). Конфиги модов бывают
/// не UTF-8 — битые байты честно заменяются через from_utf8_lossy (U+FFFD),
/// а не ошибка: редактор всё равно показывает текст.
pub fn read(paths: &Paths, instance_id: &str, rel: &str) -> Result<String> {
    let path = resolve_rel(paths, instance_id, rel)?;
    let long = long_path(&path);
    if !long.is_file() {
        return Err(LauncherError::NotFound(format!("конфиг {rel}")));
    }
    let data = std::fs::read(&long)?;
    if data.len() as u64 > MAX_CONFIG_BYTES {
        return Err(LauncherError::InvalidInput(format!(
            "файл {rel} больше 1 МБ ({} байт) — откройте его в проводнике",
            data.len()
        )));
    }
    Ok(String::from_utf8_lossy(&data).into_owned())
}

/// Записать конфиг (перезапись). Работающий инстанс держит конфиги открытыми
/// (InstanceRunning — как у миров). Создание подкаталогов запрещено:
/// родительский каталог должен существовать (иначе NotFound). Размер текста
/// ≤ 1 МБ. Запись атомарная (util::fs::atomic_write) — обрыв не оставит
/// полконфига.
pub fn write(paths: &Paths, instance_id: &str, rel: &str, text: &str) -> Result<()> {
    valid_id(instance_id)?;
    if running_pid(&instance_dir(paths, instance_id)).is_some() {
        return Err(LauncherError::InstanceRunning(instance_id.to_string()));
    }
    let path = resolve_rel(paths, instance_id, rel)?;
    if text.len() as u64 > MAX_CONFIG_BYTES {
        return Err(LauncherError::InvalidInput(format!(
            "текст {rel} больше 1 МБ ({} байт)",
            text.len()
        )));
    }
    let Some(parent) = path.parent() else {
        return Err(LauncherError::Internal("у пути конфига нет родителя".into()));
    };
    // atomic_write делает create_dir_all родителя — поэтому существование
    // каталога проверяем ДО записи: подкаталоги через редактор не создаются.
    if !long_path(parent).is_dir() {
        return Err(LauncherError::NotFound(format!(
            "каталог {}",
            parent.display()
        )));
    }
    atomic_write(&path, text.as_bytes())
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

    /// Каталог config/ инстанса в тестовом руте.
    fn config_dir(paths: &Paths, id: &str) -> PathBuf {
        minecraft_dir(&instance_dir(paths, id)).join("config")
    }

    // ---------- валидация rel (таблицей) ----------

    #[test]
    fn rel_validation_accepts_good_paths() {
        for good in [
            "options.txt",
            "sodium-options.json",
            "a",
            "A9-_.txt",
            "a..b.txt", // точки в середине — обычное имя
            "a/b",
            "a/b/c",
            "a/b/c/d", // ровно 4 уровня — граница
            "x.x/x.x/x.x/x.x",
            "x".repeat(128).as_str(), // компонент максимальной длины
        ] {
            assert!(valid_rel(good), "должно пройти: {good:?}");
        }
    }

    #[test]
    fn rel_validation_rejects_bad_paths() {
        let long_ok = "x".repeat(128);
        let long_bad = "x".repeat(129);
        for bad in [
            "",
            "..",
            ".",
            "../evil",
            "a/../b",
            ".hidden",
            "a/.hidden",
            "папка/a.txt", // не-ASCII
            "a b.txt",     // пробел
            r"a\b",        // обратный слэш
            "a:b.txt",     // двоеточие (Windows-диск/ADS)
            "a/b/c/d/e",   // 5 уровней
            "/abs",        // ведущий разделитель → пустой компонент
            "a/",          // хвостовой разделитель → пустой компонент
            "a//b",
            long_bad.as_str(),
        ] {
            assert!(!valid_rel(bad), "должно быть отклонено: {bad:?}");
        }
        assert!(valid_rel(&long_ok), "128 символов — граница допустима");
    }

    // ---------- list ----------

    #[test]
    fn list_empty_without_config_dir() {
        let (_d, paths, inst) = setup();
        assert!(list(&paths, &inst.id).unwrap().is_empty());
    }

    #[test]
    fn list_recursive_sorted_with_sizes() {
        let (_d, paths, inst) = setup();
        let cfg = config_dir(&paths, &inst.id);
        std::fs::create_dir_all(long(&cfg.join("sodium/deep"))).unwrap();
        std::fs::write(long(&cfg.join("z.txt")), "12345").unwrap();
        std::fs::write(long(&cfg.join("a.txt")), "1").unwrap();
        std::fs::write(long(&cfg.join("sodium/opt.json")), "12").unwrap();
        std::fs::write(long(&cfg.join("sodium/deep/d.txt")), "1234").unwrap();

        let files = list(&paths, &inst.id).unwrap();
        let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        // Сортировка по rel, файлы на всех глубинах 1–4.
        assert_eq!(
            rels,
            vec!["a.txt", "sodium/deep/d.txt", "sodium/opt.json", "z.txt"]
        );
        assert_eq!(files[0].bytes, 1);
        assert_eq!(files[1].bytes, 4);
        assert_eq!(files[3].bytes, 5);
    }

    /// Глубже 4 уровней и файлы с невалидными именами — мимо списка.
    #[test]
    fn list_skips_too_deep_and_invalid_names() {
        let (_d, paths, inst) = setup();
        let cfg = config_dir(&paths, &inst.id);
        // 4 компонента — граница (виден), глубже — мимо списка.
        std::fs::create_dir_all(long(&cfg.join("a/b/c/d"))).unwrap();
        std::fs::write(long(&cfg.join("a/b/c/d.txt")), "на границе — виден").unwrap();
        std::fs::write(long(&cfg.join("a/b/c/d/e.txt")), "x").unwrap();
        std::fs::write(long(&cfg.join(".hidden")), "h").unwrap();
        std::fs::write(long(&cfg.join("плохой.txt")), "p").unwrap();
        std::fs::write(long(&cfg.join("ok.txt")), "o").unwrap();

        let rels: Vec<String> = list(&paths, &inst.id)
            .unwrap()
            .into_iter()
            .map(|f| f.rel)
            .collect();
        assert_eq!(rels, vec!["a/b/c/d.txt", "ok.txt"]);
    }

    /// Лимит 500 файлов: дальшие игнорируются (список ровно 500).
    #[test]
    fn list_caps_at_500_files() {
        let (_d, paths, inst) = setup();
        let cfg = config_dir(&paths, &inst.id);
        std::fs::create_dir_all(&cfg).unwrap();
        for i in 0..505 {
            std::fs::write(long(&cfg.join(format!("f{i:04}.txt"))), "x").unwrap();
        }
        let files = list(&paths, &inst.id).unwrap();
        assert_eq!(files.len(), MAX_FILES, "ровно лимит, без паники");
        assert!(files.windows(2).all(|w| w[0].rel < w[1].rel), "отсортировано");
    }

    /// A1: некорректный id инстанса отклоняется — traversal невозможен.
    #[test]
    fn list_rejects_bad_instance_id() {
        let (_d, paths, _inst) = setup();
        for id in ["../x", "a/b", ""] {
            let err = list(&paths, id).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "id {id:?} отклонён");
        }
    }

    #[test]
    fn config_file_serializes_rel_and_bytes() {
        let f = ConfigFile {
            rel: "a.txt".into(),
            bytes: 12,
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"rel\":\"a.txt\""), "{json}");
        assert!(json.contains("\"bytes\":12"), "{json}");
    }

    // ---------- read ----------

    #[test]
    fn read_roundtrip_and_lossy_utf8() {
        let (_d, paths, inst) = setup();
        let cfg = config_dir(&paths, &inst.id);
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(long(&cfg.join("a.txt")), "привет").unwrap();
        assert_eq!(read(&paths, &inst.id, "a.txt").unwrap(), "привет");

        // Не-UTF-8 байты: lossy-замена, а не ошибка (конфиги бывают разные).
        std::fs::write(long(&cfg.join("raw.cfg")), b"ok\xff\xfe!").unwrap();
        assert_eq!(
            read(&paths, &inst.id, "raw.cfg").unwrap(),
            "ok\u{FFFD}\u{FFFD}!"
        );
    }

    /// Cap 1 МБ: ровно на границе читается, больше — InvalidInput.
    #[test]
    fn read_rejects_over_cap() {
        let (_d, paths, inst) = setup();
        let cfg = config_dir(&paths, &inst.id);
        std::fs::create_dir_all(&cfg).unwrap();
        let big = vec![b'a'; MAX_CONFIG_BYTES as usize + 1];
        std::fs::write(long(&cfg.join("big.txt")), &big).unwrap();
        let err = read(&paths, &inst.id, "big.txt").unwrap_err();
        assert_eq!(err.code(), "invalid_input", "больше 1 МБ отклонено");
        assert!(err.to_string().contains("1 МБ"), "{err}");

        let exact = vec![b'a'; MAX_CONFIG_BYTES as usize];
        std::fs::write(long(&cfg.join("exact.txt")), &exact).unwrap();
        assert!(read(&paths, &inst.id, "exact.txt").is_ok(), "ровно 1 МБ читается");
    }

    #[test]
    fn read_missing_file_is_not_found() {
        let (_d, paths, inst) = setup();
        let err = read(&paths, &inst.id, "ghost.txt").unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    /// Невалидный rel — InvalidInput до обращения к диску (traversal и мусор).
    #[test]
    fn read_rejects_invalid_rel() {
        let (_d, paths, inst) = setup();
        for bad in ["../evil.txt", "a/b/c/d/e.txt", ".hidden", r"a\b", "", "a b.txt"] {
            let err = read(&paths, &inst.id, bad).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "rel {bad:?} отклонён");
        }
    }

    // ---------- write ----------

    #[test]
    fn write_overwrites_and_refreshes_size() {
        let (_d, paths, inst) = setup();
        let cfg = config_dir(&paths, &inst.id);
        std::fs::create_dir_all(&cfg).unwrap();

        write(&paths, &inst.id, "opt.json", "v1").unwrap();
        assert_eq!(read(&paths, &inst.id, "opt.json").unwrap(), "v1");
        write(&paths, &inst.id, "opt.json", "v2-длиннее").unwrap();
        assert_eq!(read(&paths, &inst.id, "opt.json").unwrap(), "v2-длиннее");

        let files = list(&paths, &inst.id).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].bytes, "v2-длиннее".len() as u64, "размер обновился");
    }

    /// Новый файл в СУЩЕСТВУЮЩЕМ каталоге разрешён; отсутствующий каталог —
    /// NotFound, и он НЕ создаётся (подкаталоги через редактор запрещены).
    #[test]
    fn write_requires_existing_directory() {
        let (_d, paths, inst) = setup();
        // config/ нет вовсе.
        let err = write(&paths, &inst.id, "new.txt", "x").unwrap_err();
        assert_eq!(err.code(), "not_found", "нет config/ → NotFound");
        assert!(!long(&config_dir(&paths, &inst.id)).exists(), "config/ не создан");

        // config/ есть, подкаталога sub/ нет — запись в sub/new.txt мимо.
        std::fs::create_dir_all(long(&config_dir(&paths, &inst.id))).unwrap();
        let err = write(&paths, &inst.id, "sub/new.txt", "x").unwrap_err();
        assert_eq!(err.code(), "not_found");
        assert!(
            !long(&config_dir(&paths, &inst.id).join("sub")).exists(),
            "подкаталог не должен создаваться"
        );

        // Новый файл в существующем корне — ок.
        write(&paths, &inst.id, "new.txt", "x").unwrap();
        assert_eq!(read(&paths, &inst.id, "new.txt").unwrap(), "x");
    }

    #[test]
    fn write_rejects_too_big_text() {
        let (_d, paths, inst) = setup();
        std::fs::create_dir_all(long(&config_dir(&paths, &inst.id))).unwrap();
        let big = "a".repeat(MAX_CONFIG_BYTES as usize + 1);
        let err = write(&paths, &inst.id, "big.txt", &big).unwrap_err();
        assert_eq!(err.code(), "invalid_input");
        assert!(err.to_string().contains("1 МБ"), "{err}");
        assert!(!long(&config_dir(&paths, &inst.id).join("big.txt")).exists(), "файл не записан");
    }

    /// Работающий инстанс (живой PID в .lock — свой PID теста) блокирует
    /// запись кодом instance_running; чтение списка/файла разрешено.
    #[test]
    fn write_guard_instance_running() {
        let (_d, paths, inst) = setup();
        let cfg = config_dir(&paths, &inst.id);
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(long(&cfg.join("opt.txt")), "old").unwrap();
        let dir = instance_dir(&paths, &inst.id);
        std::fs::write(dir.join(".lock"), std::process::id().to_string()).unwrap();

        let err = write(&paths, &inst.id, "opt.txt", "new").unwrap_err();
        assert_eq!(err.code(), "instance_running");
        assert_eq!(
            read(&paths, &inst.id, "opt.txt").unwrap(),
            "old",
            "файл не тронут"
        );
        assert!(list(&paths, &inst.id).is_ok(), "чтение не блокируется");

        let _ = std::fs::remove_file(dir.join(".lock"));
        write(&paths, &inst.id, "opt.txt", "new").unwrap();
        assert_eq!(read(&paths, &inst.id, "opt.txt").unwrap(), "new");
    }

    /// rel валидируется и в write: traversal/mусор — InvalidInput.
    #[test]
    fn write_rejects_invalid_rel() {
        let (_d, paths, inst) = setup();
        std::fs::create_dir_all(long(&config_dir(&paths, &inst.id))).unwrap();
        for bad in ["../evil.txt", "a/../../x", ".env", "a/b/c/d/e/f.txt"] {
            let err = write(&paths, &inst.id, bad, "x").unwrap_err();
            assert_eq!(err.code(), "invalid_input", "rel {bad:?} отклонён");
        }
    }
}
