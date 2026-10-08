//! Метаданные jar: зависимости/конфликты (F6/F9, D37-B).
//!
//! Читаем `fabric.mod.json` / `quilt.mod.json` прямо из jar (это zip) без
//! распаковки на диск. Моды без манифеста (форж/старые) и битые jar — НЕ
//! ошибка: метаданные пустые. `analyze_all` — панельный анализ набора:
//! missing (зависимости, которых нет среди установленных), disabled
//! (установлен, но выключен через `.disabled`), conflicts (breaks/conflicts
//! на установленный мод, независимо от его enabled).

use crate::errors::Result;
use crate::instances::content::{disk_path, resolve_content_file};
use crate::instances::{
    instance_dir, load_content_manifest, minecraft_dir, ContentEntry, ContentKind,
};
use crate::paths::Paths;
use std::collections::HashMap;
use std::io::Read as _;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModMetadata {
    pub file: String,
    pub id: Option<String>,
    pub missing_deps: Vec<String>,
    pub disabled_deps: Vec<String>,
    pub conflicts: Vec<String>,
}

/// Имена манифестов внутри jar (строго в корне архива).
const FABRIC_MANIFEST: &str = "fabric.mod.json";
const QUILT_MANIFEST: &str = "quilt.mod.json";

/// Лимит на размер манифеста внутри jar: настоящий fabric.mod.json — единицы
/// КБ; гигантскую запись (zip-bomb через заголовок) читать смысла нет.
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

/// Псевдо-зависимости окружения: игра, загрузчики, JVM. Они не лежат в mods/,
/// поэтому их отсутствие среди id установленных модов — не «не хватает»
/// (иначе каждый fabric-мод рапортовал бы missing по minecraft/fabricloader).
const ENVIRONMENT_DEPS: [&str; 4] = ["minecraft", "fabricloader", "quilt_loader", "java"];

/// Сырые метаданные из манифеста внутри jar (до сверки с набором).
struct RawModMeta {
    id: Option<String>,
    depends: Vec<String>,
    /// breaks И conflicts — оба идут в список конфликтов.
    breaks: Vec<String>,
    provides: Vec<String>,
}

/// Результат чтения манифеста из jar.
enum JarManifest {
    /// Манифест прочитан и распарсен.
    Found(RawModMeta),
    /// Jar читается, но fabric/quilt-манифеста нет (форж/старые) либо json
    /// битый — метаданных нет, не ошибка.
    Absent,
    /// Jar не читается (нет файла на диске, битый zip) — analyze_all такие
    /// строки пропускает с предупреждением.
    Unreadable(String),
}

/// Для одного мода: прочитать fabric.mod.json/quilt.mod.json из jar (моды
/// только kind=Mod; для остального контента вернётся пустое `id` — не ошибка).
/// Возвращает только `file` и `id`: сверку с набором (missing/disabled/
/// conflicts) делает [`analyze_all`].
pub fn analyze_jar(paths: &Paths, instance_id: &str, file: &str) -> Result<ModMetadata> {
    // load валидирует id инстанса (A1: иначе `..\x` ушёл бы за instances/) и
    // требует существования инстанса — тот же вход, что у toggle/remove.
    crate::instances::load(paths, instance_id)?;
    // Барьер traversal для пути файла (pub(crate) из content.rs — как в toggle).
    resolve_content_file(paths, instance_id, file)?;
    // Состояние на диске берём из манифеста (вкл/выкл = суффикс `.disabled`);
    // нет записи — пробуем активный файл.
    let enabled = load_content_manifest(paths, instance_id)
        .into_iter()
        .find(|e| e.file == file)
        .map(|e| e.enabled)
        .unwrap_or(true);
    let jar = minecraft_dir(&instance_dir(paths, instance_id)).join(disk_path(file, enabled));
    let id = match read_jar_manifest(&jar) {
        JarManifest::Found(raw) => raw.id,
        JarManifest::Absent => None,
        JarManifest::Unreadable(why) => {
            tracing::warn!("modmeta: jar не читается: {why}");
            None
        }
    };
    Ok(ModMetadata {
        file: file.to_string(),
        id,
        missing_deps: Vec::new(),
        disabled_deps: Vec::new(),
        conflicts: Vec::new(),
    })
}

/// Панельный анализ всего набора: missing = depends, которых нет среди id
/// установленных (или provided); disabled = установленный, но выключенный;
/// conflicts = breaks/conflicts, чьи id установлены (независимо от enabled).
/// Ошибка одного jar не валит весь список: строка пропускается с warn.
pub fn analyze_all(paths: &Paths, instance_id: &str) -> Result<Vec<ModMetadata>> {
    // Валидация id + существование инстанса (NotFound/InvalidInput).
    crate::instances::load(paths, instance_id)?;
    let manifest = load_content_manifest(paths, instance_id);
    let base = minecraft_dir(&instance_dir(paths, instance_id));

    // Проход 1: читаем манифесты jar-модов (включая выключенные) и собираем
    // множество установленных id — id мода + его provides. При дубле id
    // владельцем считается первый в манифесте.
    let mut scanned: Vec<(ContentEntry, Option<RawModMeta>)> = Vec::new();
    let mut installed: HashMap<String, (String, bool)> = HashMap::new(); // id → (file, enabled)
    for e in manifest.iter().filter(|e| e.kind == ContentKind::Mod) {
        let jar = base.join(disk_path(&e.file, e.enabled));
        match read_jar_manifest(&jar) {
            JarManifest::Found(raw) => {
                if let Some(id) = &raw.id {
                    installed
                        .entry(id.clone())
                        .or_insert((e.file.clone(), e.enabled));
                }
                for provided in &raw.provides {
                    installed
                        .entry(provided.clone())
                        .or_insert((e.file.clone(), e.enabled));
                }
                scanned.push((e.clone(), Some(raw)));
            }
            // Форж/старые без манифеста: строка с пустыми полями (панель
            // показывает файл без зависимостей).
            JarManifest::Absent => scanned.push((e.clone(), None)),
            // Битый jar: строку пропускаем целиком.
            JarManifest::Unreadable(why) => {
                tracing::warn!("modmeta: пропускаю jar: {why}");
            }
        }
    }

    // Проход 2: сверяем depends/breaks каждого мода с установленным набором.
    Ok(scanned
        .into_iter()
        .map(|(e, raw)| match raw {
            None => ModMetadata {
                file: e.file,
                id: None,
                missing_deps: Vec::new(),
                disabled_deps: Vec::new(),
                conflicts: Vec::new(),
            },
            Some(raw) => {
                let mut missing = Vec::new();
                let mut disabled = Vec::new();
                // Алиасы provides указывают на тот же файл-владелец: если мод
                // уже отмечен выключенным, его псевдонимы не дублируем.
                let mut disabled_files: Vec<&str> = Vec::new();
                for dep in &raw.depends {
                    // Окружение (игра/загрузчик/JVM) не лежит в mods/ — его
                    // отсутствие среди id модов не считается «не хватает».
                    if ENVIRONMENT_DEPS.contains(&dep.as_str()) {
                        continue;
                    }
                    match installed.get(dep) {
                        None => missing.push(dep.clone()),
                        Some((file, false)) => {
                            let key = file.as_str();
                            if !disabled_files.contains(&key) {
                                disabled_files.push(key);
                                disabled.push(dep.clone());
                            }
                        }
                        Some((_, true)) => {}
                    }
                }
                let mut conflicts = Vec::new();
                for broken in &raw.breaks {
                    // Конфликт важен, только если второй мод реально установлен
                    // (пусть даже выключен); дубли breaks+conflicts схлопываем.
                    if installed.contains_key(broken) && !conflicts.contains(broken) {
                        conflicts.push(broken.clone());
                    }
                }
                ModMetadata {
                    file: e.file,
                    id: raw.id,
                    missing_deps: missing,
                    disabled_deps: disabled,
                    conflicts,
                }
            }
        })
        .collect())
}

/// Зависимости в fabric/quilt-манифестах бывают разных форм: map<string,
/// версия|массив версий> (fabric depends), массив строк, массив объектов
/// {id, versions} (quilt). Достаём только id.
fn collect_ids(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::Object(map)) => map.keys().cloned().collect(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Object(o) => o
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Распарсить fabric.mod.json / quilt.mod.json. None = не похож на манифест
/// (битый json, чужой формат) — мод без метаданных, не ошибка.
fn parse_manifest_json(bytes: &[u8]) -> Option<RawModMeta> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    // quilt.mod.json — обёртка {"quilt_loader": {...}}: работаем с её
    // содержимым; не выходит (нет quilt_loader/id) — поля просто пустые.
    let root = value.get("quilt_loader").unwrap_or(&value);
    let mut breaks = collect_ids(root.get("breaks"));
    breaks.extend(collect_ids(root.get("conflicts")));
    Some(RawModMeta {
        id: root
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        depends: collect_ids(root.get("depends")),
        breaks,
        provides: collect_ids(root.get("provides")),
    })
}

/// Прочитать манифест из jar. Ошибки чтения/распаковки — Unreadable (не
/// паникуем и не роняем анализ), отсутствие манифеста — Absent.
fn read_jar_manifest(jar: &std::path::Path) -> JarManifest {
    let file = match std::fs::File::open(crate::util::fs::long_path(jar)) {
        Ok(f) => f,
        Err(e) => return JarManifest::Unreadable(format!("{}: {e}", jar.display())),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return JarManifest::Unreadable(format!("{}: {e}", jar.display())),
    };
    // Имена записей через by_index (без привязки к вариантам ZipError):
    // манифест ищем строго в корне архива.
    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();
    // Приоритет у fabric.mod.json: quilt-моды часто кладут и fabric-совместимый
    // манифест, а чисто quilt-моды — quilt.mod.json.
    for name in [FABRIC_MANIFEST, QUILT_MANIFEST] {
        if !names.iter().any(|n| n == name) {
            continue;
        }
        let entry = match archive.by_name(name) {
            Ok(entry) => entry,
            Err(e) => return JarManifest::Unreadable(format!("{jar:?}: {name}: {e}")),
        };
        // Читаем с лимитом (take): верить размеру из заголовка zip нельзя.
        let mut bytes = Vec::new();
        if let Err(e) = entry.take(MAX_MANIFEST_BYTES as u64 + 1).read_to_end(&mut bytes) {
            return JarManifest::Unreadable(format!("{jar:?}: {name}: {e}"));
        }
        if bytes.len() > MAX_MANIFEST_BYTES {
            tracing::warn!("modmeta: {name} в {jar:?} крупнее лимита — пропущен");
            continue;
        }
        match parse_manifest_json(&bytes) {
            Some(raw) => return JarManifest::Found(raw),
            // Битый json: пробуем второе имя, иначе — «метаданных нет».
            None => continue,
        }
    }
    JarManifest::Absent
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{save_content_manifest, ContentSource, Instance};
    use std::io::Write as _;
    use std::path::{Path, PathBuf};

    fn long(p: &Path) -> PathBuf {
        crate::util::fs::long_path(p)
    }

    /// Фейковый Paths в tempdir + инстанс с пустым mods/ (как в content.rs).
    fn setup() -> (tempfile::TempDir, Paths, Instance) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let inst = Instance::new("Тест", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        std::fs::create_dir_all(long(&mc.join("mods"))).unwrap();
        (dir, paths, inst)
    }

    fn mods_dir(paths: &Paths, inst: &Instance) -> PathBuf {
        minecraft_dir(&instance_dir(paths, &inst.id))
    }

    fn entry(file: &str, enabled: bool) -> ContentEntry {
        ContentEntry {
            kind: ContentKind::Mod,
            file: file.into(),
            source: ContentSource::Local,
            project_id: None,
            version_id: None,
            sha1: None,
            url: None,
            enabled,
        }
    }

    /// Собрать jar (это zip) на диске с заданными записями.
    fn write_jar(path: &Path, entries: &[(&str, &str)]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(long(parent)).unwrap();
        }
        let f = std::fs::File::create(long(path)).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = Default::default();
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn parse_fabric_forms_map_and_array() {
        // depends — map (значение — строка ИЛИ массив версий); breaks — map;
        // conflicts — массив строк; provides — массив строк.
        let json = br#"{
            "schemaVersion": 1, "id": "dep", "version": "1.0.0",
            "depends": {"api": "*", "fabricloader": ">=0.15"},
            "breaks": {"enemy": "*"},
            "conflicts": ["rival", "enemy"],
            "provides": ["dep-alt"]
        }"#;
        let raw = parse_manifest_json(json).expect("манифест распарсен");
        assert_eq!(raw.id.as_deref(), Some("dep"));
        assert!(raw.depends.contains(&"api".to_string()));
        assert!(raw.depends.contains(&"fabricloader".to_string()));
        assert!(raw.breaks.contains(&"enemy".to_string()), "breaks map");
        assert!(raw.breaks.contains(&"rival".to_string()), "conflicts array");
        assert_eq!(raw.provides, vec!["dep-alt".to_string()]);
    }

    #[test]
    fn parse_quilt_wrapper() {
        // quilt_loader-обёртка; depends — массив строк и объектов с id.
        let json = br#"{"quilt_loader": {
            "group": "ru.example", "id": "qmod", "version": "1.0.0",
            "depends": [{"id": "fabricloader", "versions": ">=0.15"}, "api", {"unknown": 1}]
        }}"#;
        let raw = parse_manifest_json(json).expect("quilt-манифест распарсен");
        assert_eq!(raw.id.as_deref(), Some("qmod"));
        assert!(raw.depends.contains(&"api".to_string()));
        assert!(raw.depends.contains(&"fabricloader".to_string()));
    }

    #[test]
    fn parse_garbage_is_none() {
        assert!(parse_manifest_json(b"{ not json").is_none());
    }

    /// analyze_jar: читает id (в т.ч. из .disabled по манифесту); битый jar,
    /// jar без манифеста и отсутствующий файл — пустые поля, НЕ ошибки.
    #[test]
    fn analyze_jar_reads_id_and_tolerates_broken() {
        let (_d, paths, inst) = setup();
        let mods = mods_dir(&paths, &inst);
        write_jar(&mods.join("mods/dep.jar"), &[(
            "fabric.mod.json",
            r#"{"schemaVersion":1,"id":"dep","depends":{"api":"*"}}"#,
        )]);
        let meta = analyze_jar(&paths, &inst.id, "mods/dep.jar").unwrap();
        assert_eq!(meta.file, "mods/dep.jar");
        assert_eq!(meta.id.as_deref(), Some("dep"));
        // Сверку missing/disabled/conflicts делает analyze_all (см. тест ниже).
        assert!(meta.missing_deps.is_empty());
        assert!(meta.disabled_deps.is_empty());
        assert!(meta.conflicts.is_empty());

        // Выключенный мод: файл на диске с суффиксом .disabled, запись
        // enabled=false — id всё равно читается.
        save_content_manifest(&paths, &inst.id, &[entry("mods/dep.jar", false)]).unwrap();
        std::fs::rename(
            long(&mods.join("mods/dep.jar")),
            long(&mods.join("mods/dep.jar.disabled")),
        )
        .unwrap();
        let meta = analyze_jar(&paths, &inst.id, "mods/dep.jar").unwrap();
        assert_eq!(meta.id.as_deref(), Some("dep"), "читает .disabled-состояние");

        // Битый jar (не zip) → пустые поля, не ошибка.
        std::fs::write(long(&mods.join("mods/broken.jar")), b"garbage, not a zip").unwrap();
        let meta = analyze_jar(&paths, &inst.id, "mods/broken.jar").unwrap();
        assert_eq!(meta.id, None);
        assert!(meta.missing_deps.is_empty());

        // Jar без fabric/quilt-манифеста (форж/старые) → пустые поля.
        write_jar(&mods.join("mods/forge.jar"), &[("META-INF/mods.toml", "x")]);
        let meta = analyze_jar(&paths, &inst.id, "mods/forge.jar").unwrap();
        assert_eq!(meta.id, None);

        // Файла нет на диске → пустые поля, не ошибка.
        let meta = analyze_jar(&paths, &inst.id, "mods/ghost.jar").unwrap();
        assert_eq!(meta.id, None);

        // Чужой id инстанса / traversal в пути файла — честные ошибки.
        assert_eq!(
            analyze_jar(&paths, "ghost-id", "mods/dep.jar").unwrap_err().code(),
            "not_found"
        );
        assert_eq!(
            analyze_jar(&paths, &inst.id, "../escape").unwrap_err().code(),
            "invalid_input"
        );
    }

    /// Панельный анализ: dep зависит от выключенного api (disabled), от
    /// предоставленного api provides (не missing) и от отсутствующего ghost
    /// (missing); breaks на установленный api — конфликт, даже выключенный.
    #[test]
    fn analyze_all_missing_disabled_conflicts() {
        let (_d, paths, inst) = setup();
        let mods = mods_dir(&paths, &inst);
        // api: выключен, дополнительно предоставляет api-alt.
        write_jar(&mods.join("mods/api.jar.disabled"), &[(
            "fabric.mod.json",
            r#"{"id":"api","provides":["api-alt"]}"#,
        )]);
        // dep: включён; depends — map с окружением, api, api-alt, ghost;
        // breaks — массив строк.
        write_jar(&mods.join("mods/dep.jar"), &[(
            "fabric.mod.json",
            r#"{"id":"dep","depends":{"minecraft":"*","fabricloader":">=0.15","api":"*","api-alt":"*","ghost":"*"},"breaks":["api"]}"#,
        )]);
        save_content_manifest(
            &paths,
            &inst.id,
            &[entry("mods/api.jar", false), entry("mods/dep.jar", true)],
        )
        .unwrap();

        let rows = analyze_all(&paths, &inst.id).unwrap();
        assert_eq!(rows.len(), 2, "строка на каждый kind=Mod");

        let api = rows.iter().find(|r| r.file == "mods/api.jar").unwrap();
        assert_eq!(api.id.as_deref(), Some("api"));
        assert!(api.missing_deps.is_empty());
        assert!(api.conflicts.is_empty());

        let dep = rows.iter().find(|r| r.file == "mods/dep.jar").unwrap();
        assert_eq!(dep.id.as_deref(), Some("dep"));
        assert_eq!(
            dep.missing_deps,
            vec!["ghost".to_string()],
            "окружение (minecraft/fabricloader) не считается missing"
        );
        assert_eq!(dep.disabled_deps, vec!["api".to_string()]);
        assert_eq!(
            dep.conflicts,
            vec!["api".to_string()],
            "breaks на установленный (пусть выключенный) — конфликт"
        );
    }

    /// Битый jar пропускается (строки нет), форж без манифеста даёт пустую
    /// строку, не-модовый контент (ресурспак) не анализируется.
    #[test]
    fn analyze_all_skips_broken_jars_and_non_mods() {
        let (_d, paths, inst) = setup();
        let mods = mods_dir(&paths, &inst);
        write_jar(&mods.join("mods/ok.jar"), &[("fabric.mod.json", r#"{"id":"ok"}"#)]);
        // Битый jar: не zip.
        std::fs::write(long(&mods.join("mods/broken.jar")), b"not a zip").unwrap();
        // Форж: zip без fabric/quilt-манифеста.
        write_jar(&mods.join("mods/forge.jar"), &[("META-INF/mods.toml", "x")]);

        let mut rp = entry("resourcepacks/p.zip", true);
        rp.kind = ContentKind::ResourcePack;
        save_content_manifest(
            &paths,
            &inst.id,
            &[
                entry("mods/ok.jar", true),
                entry("mods/broken.jar", true),
                entry("mods/forge.jar", true),
                rp,
            ],
        )
        .unwrap();

        let rows = analyze_all(&paths, &inst.id).unwrap();
        assert_eq!(rows.len(), 2, "битый jar пропущен, ресурспак не входит");
        assert!(rows.iter().any(|r| r.file == "mods/ok.jar"));
        let forge = rows.iter().find(|r| r.file == "mods/forge.jar").unwrap();
        assert_eq!(forge.id, None, "без манифеста — пустые поля");

        // Нет инстанса → ошибка.
        assert_eq!(analyze_all(&paths, "ghost").unwrap_err().code(), "not_found");
    }

    /// Контракт с UI (src/api/types.ts): camelCase-ключи.
    #[test]
    fn mod_metadata_serializes_camel_case() {
        let meta = ModMetadata {
            file: "mods/a.jar".into(),
            id: Some("a".into()),
            missing_deps: vec!["x".into()],
            disabled_deps: vec![],
            conflicts: vec![],
        };
        let v = serde_json::to_value(&meta).unwrap();
        assert_eq!(v["file"], "mods/a.jar");
        assert_eq!(v["id"], "a");
        assert_eq!(v["missingDeps"], serde_json::json!(["x"]));
        assert_eq!(v["disabledDeps"], serde_json::json!([]));
        assert_eq!(v["conflicts"], serde_json::json!([]));
        assert!(v.get("missing_deps").is_none(), "snake_case не сериализуется");
    }
}
