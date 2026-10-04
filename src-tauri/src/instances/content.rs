//! Управление установленным контентом инстанса: вкл/выкл (суффикс
//! `.disabled` — паттерн Prism, загрузчики его игнорируют), удаление,
//! бэкап zip и экспорт .mrpack (формат Modrinth, spec modrinth.com).
//!
//! Манифест хранит логический путь (`mods/sodium.jar`) без суффикса;
//! выключение = переименование файла на диске + `enabled: false`.

use crate::errors::{LauncherError, Result};
use crate::instances::{
    instance_dir, load_content_manifest, minecraft_dir, save_content_manifest, ContentEntry,
    Instance,
};
use crate::paths::Paths;
use serde::Serialize;
use std::io::Write as _;
use std::path::PathBuf;

const DISABLED_SUFFIX: &str = ".disabled";

/// Каталоги контента, которыми управляет манифест (вкл/выкл/удаление).
/// F7: datapacks — датапаки (minecraft/datapacks), снапшоты их не берут.
const CONTENT_DIRS: [&str; 4] = ["mods", "resourcepacks", "shaderpacks", "datapacks"];

/// Каталоги-шум: ни в бэкап, ни в экспорт (логи, краши, временный кэш).
const NOISE_DIRS: [&str; 3] = ["logs", "crash-reports", "cache"];

/// Сейвы — личные и тяжёлые: в бэкап входят, в шаринговый .mrpack нет.
const EXPORT_SKIP_DIRS: [&str; 1] = ["saves"];

/// Результат архивной операции (бэкап/экспорт) для UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveResult {
    pub path: String,
    pub files: usize,
    pub bytes: u64,
}

/// `file` ("mods/x.jar") → абсолютный путь внутри minecraft/ инстанса.
/// Только каталоги контента, только вниз: traversal и чужие каталоги — ошибка.
/// pub(crate): переиспользуется snapshots.rs (restore) как traversal-барьер.
pub(crate) fn resolve_content_file(paths: &Paths, id: &str, file: &str) -> Result<PathBuf> {
    let norm = file.replace('\\', "/");
    let parts: Vec<&str> = norm.split('/').collect();
    if parts.len() < 2
        || !CONTENT_DIRS.contains(&parts[0])
        || parts[1..].iter().any(|p| p.is_empty() || *p == ".." || p.contains(':'))
    {
        return Err(LauncherError::InvalidInput(format!(
            "недопустимый путь контента: {file}"
        )));
    }
    let mut p = minecraft_dir(&instance_dir(paths, id));
    for seg in &parts {
        p.push(seg);
    }
    Ok(p)
}

/// pub(crate): переиспользуется snapshots.rs (restore) для обоих состояний имени.
pub(crate) fn disk_path(entry_file: &str, enabled: bool) -> PathBuf {
    let mut s = entry_file.replace('/', "\\");
    if !enabled {
        s.push_str(DISABLED_SUFFIX);
    }
    PathBuf::from(s)
}

/// Вкл/выкл: переименовать файл (+/- `.disabled`) и перевернуть флаг.
/// Логический путь в манифесте не меняется.
pub fn toggle(paths: &Paths, id: &str, file: &str) -> Result<ContentEntry> {
    resolve_content_file(paths, id, file)?;
    let mut manifest = load_content_manifest(paths, id);
    let entry = manifest
        .iter_mut()
        .find(|e| e.file == file)
        .ok_or_else(|| LauncherError::InvalidInput(format!("нет записи в манифесте: {file}")))?;
    let new_enabled = !entry.enabled;
    let base = minecraft_dir(&instance_dir(paths, id));
    let from = base.join(disk_path(file, entry.enabled));
    let to = base.join(disk_path(file, new_enabled));
    let from_long = crate::util::fs::long_path(&from);
    let to_long = crate::util::fs::long_path(&to);
    if from_long.exists() {
        // ENV-11b: антивирус на миг придерживает .jar (sharing violation) —
        // голый rename падал навсегда; повторяем, как atomic_write (A26).
        crate::util::fs::rename_with_retry(&from_long, &to_long)?;
    } else if !to_long.exists() {
        // Ни текущего, ни целевого файла нет — переименовывать нечего.
        return Err(LauncherError::NotFound(from.to_string_lossy().into()));
    }
    // Иначе: файла в текущем состоянии нет, но целевой уже лежит на диске
    // (рассинхрон) — диск уже в целевом состоянии, просто переворачиваем запись.
    entry.enabled = new_enabled;
    let updated = entry.clone();
    save_content_manifest(paths, id, &manifest)?;
    Ok(updated)
}

/// Удалить контент: файл в обоих вариантах имени + запись из манифеста.
pub fn remove(paths: &Paths, id: &str, file: &str) -> Result<()> {
    resolve_content_file(paths, id, file)?;
    let mut manifest = load_content_manifest(paths, id);
    let before = manifest.len();
    manifest.retain(|e| e.file != file);
    if manifest.len() == before {
        return Err(LauncherError::InvalidInput(format!(
            "нет записи в манифесте: {file}"
        )));
    }
    let base = minecraft_dir(&instance_dir(paths, id));
    for enabled in [true, false] {
        let p = crate::util::fs::long_path(&base.join(disk_path(file, enabled)));
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    save_content_manifest(paths, id, &manifest)
}

/// Копировать контент между инстансами (F10): файл в активном состоянии +
/// запись в манифест приёмника (projectId/versionId/sha1/url/kind сохраняются,
/// enabled = true). Оба инстанса должны быть остановлены (B9: файлы занятой
/// игры на Windows нельзя читать/писать).
pub fn copy_entry(paths: &Paths, from_id: &str, to_id: &str, file: &str) -> Result<ContentEntry> {
    // load валидирует id и существование обоих инстансов (нет — NotFound).
    crate::instances::load(paths, from_id)?;
    crate::instances::load(paths, to_id)?;
    for id in [from_id, to_id] {
        if crate::instances::running_pid(&instance_dir(paths, id)).is_some() {
            return Err(LauncherError::InstanceRunning(id.to_string()));
        }
    }
    // Барьер traversal для пути файла в ОБЕИХ раскладках (одинаковый префикс).
    resolve_content_file(paths, from_id, file)?;
    resolve_content_file(paths, to_id, file)?;
    let src_entry = load_content_manifest(paths, from_id)
        .into_iter()
        .find(|e| e.file == file)
        .ok_or_else(|| LauncherError::InvalidInput(format!("нет записи в манифесте: {file}")))?;
    let mut to_manifest = load_content_manifest(paths, to_id);
    if to_manifest.iter().any(|e| e.file == file) {
        return Err(LauncherError::InvalidInput(format!("уже установлен: {file}")));
    }
    let base = minecraft_dir(&instance_dir(paths, from_id));
    let src = base.join(disk_path(file, true)); // активное состояние (без .disabled)
    let dst = minecraft_dir(&instance_dir(paths, to_id)).join(disk_path(file, true));
    if !crate::util::fs::long_path(&src).exists() {
        return Err(LauncherError::NotFound(src.to_string_lossy().into()));
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(crate::util::fs::long_path(parent))?;
    }
    std::fs::copy(crate::util::fs::long_path(&src), crate::util::fs::long_path(&dst))?;
    let entry = ContentEntry {
        enabled: true,
        ..src_entry
    };
    to_manifest.push(entry.clone());
    save_content_manifest(paths, to_id, &to_manifest)?;
    Ok(entry)
}

/// Бэкап инстанса целиком (zip): instance.json, манифест контента, versions/,
/// minecraft/ (моды, сейвы, конфиги) — без логов/крашей/кэша и .lock.
pub fn backup(paths: &Paths, inst: &Instance) -> Result<ArchiveResult> {
    let src = instance_dir(paths, &inst.id);
    if !crate::util::fs::long_path(&src).exists() {
        return Err(LauncherError::NotFound(src.to_string_lossy().into()));
    }
    std::fs::create_dir_all(crate::util::fs::long_path(&paths.backups_dir()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = format!(
        "{}-{}.zip",
        crate::util::names::sanitize_user_name(&inst.name, "instance"),
        stamp
    );
    let dest = paths.backups_dir().join(name);

    let file = std::fs::File::create(crate::util::fs::long_path(&dest))?;
    let mut zip = zip::ZipWriter::new(file);
    let count = crate::util::zip::write_zip_dir(&src, &mut zip, "instance/", &|rel| {
        rel == ".lock" || is_skipped_dir(rel, &NOISE_DIRS)
    })?;
    zip.finish()
        .map_err(|e| LauncherError::Zip(format!("finish: {e}")))?;
    let bytes = std::fs::metadata(crate::util::fs::long_path(&dest))?.len();
    Ok(ArchiveResult {
        path: dest.to_string_lossy().into(),
        files: count,
        bytes,
    })
}

/// Является ли `rel` (путь относительно корня инстанса) пропускаемым
/// поддеревом minecraft/ (`minecraft/<dir>` или глубже).
fn is_skipped_dir(rel: &str, dirs: &[&str]) -> bool {
    let mut it = rel.split('/');
    let first = it.next();
    let second = it.next();
    first == Some("minecraft") && second.is_some_and(|d| dirs.contains(&d))
}

/// Экспорт в .mrpack (формат Modrinth): modrinth.index.json (files[] — записи
/// с url+sha1), overrides/ — конфиги, ресурспаки, шейдеры, локальные моды.
/// Сейвы и шум не включаются. Файлы из files[] в overrides не дублируются.
pub fn export_mrpack(paths: &Paths, inst: &Instance) -> Result<ArchiveResult> {
    let src = instance_dir(paths, &inst.id);
    std::fs::create_dir_all(crate::util::fs::long_path(&paths.exports_dir()))?;
    let name = format!(
        "{}.mrpack",
        crate::util::names::sanitize_user_name(&inst.name, "instance")
    );
    let dest = paths.exports_dir().join(name);

    let manifest = load_content_manifest(paths, &inst.id);
    let mut index_files = Vec::new();
    let mut in_index: Vec<String> = Vec::new();
    for e in manifest.iter().filter(|e| e.enabled) {
        if let (Some(url), Some(sha1)) = (&e.url, &e.sha1) {
            let abs = minecraft_dir(&src).join(disk_path(&e.file, true));
            let size = std::fs::metadata(crate::util::fs::long_path(&abs))
                .map(|m| m.len())
                .unwrap_or(0);
            index_files.push(serde_json::json!({
                "path": e.file,
                "hashes": { "sha1": sha1 },
                "downloads": [url],
                "fileSize": size,
            }));
            in_index.push(e.file.clone());
        }
    }

    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".into(), serde_json::json!(inst.mc_version));
    if let (Some(loader), Some(ver)) = (&inst.loader, &inst.loader_version) {
        let key = match loader.as_str() {
            "fabric" => "fabric-loader",
            "quilt" => "quilt-loader",
            "forge" => "forge",
            "neoforge" => "neoforge",
            other => other,
        };
        deps.insert(key.into(), serde_json::json!(ver));
    }
    let index = serde_json::json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": inst.mc_version,
        "files": index_files,
        "dependencies": deps,
    });

    let mc = minecraft_dir(&src);
    let file = std::fs::File::create(crate::util::fs::long_path(&dest))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions = Default::default();
    let index_bytes = serde_json::to_vec_pretty(&index)?;
    zip.start_file("modrinth.index.json", opts)
        .map_err(|e| LauncherError::Zip(format!("index: {e}")))?;
    zip.write_all(&index_bytes)?;
    let mut count = 1usize;
    if crate::util::fs::long_path(&mc).exists() {
        count += crate::util::zip::write_zip_dir(&mc, &mut zip, "overrides/", &|rel| {
            rel.split('/').next().is_some_and(|d| {
                EXPORT_SKIP_DIRS.contains(&d) || NOISE_DIRS.contains(&d)
            }) || in_index.iter().any(|f| *f == rel)
        })?;
    }
    zip.finish()
        .map_err(|e| LauncherError::Zip(format!("finish: {e}")))?;
    let bytes = std::fs::metadata(crate::util::fs::long_path(&dest))?.len();
    Ok(ArchiveResult {
        path: dest.to_string_lossy().into(),
        files: count,
        bytes,
    })
}

/// Смягчение A16: файл иконки обязан быть абсолютным путём внутри домашнего
/// каталога пользователя (оттуда его и отдаёт нативный диалог выбора файла).
/// Без этого рендер читал любой файл системы до 300 КБ (эксфильтрация) и
/// получал оракул существования файлов по разнице ошибок.
pub(crate) fn ensure_icon_source_allowed(path: &std::path::Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(LauncherError::InvalidInput(
            "путь к иконке должен быть абсолютным".into(),
        ));
    }
    let home = dirs::home_dir()
        .ok_or_else(|| LauncherError::Internal("не определён домашний каталог".into()))?;
    // canonicalize снимает `..` и симлинки: иначе `C:\Users\u\..\Windows\x`
    // формально проходил бы проверку starts_with.
    let real = std::fs::canonicalize(crate::util::fs::long_path(path))
        .map_err(|_| LauncherError::NotFound(format!("файл иконки: {}", path.display())))?;
    let home_real = std::fs::canonicalize(crate::util::fs::long_path(&home)).unwrap_or(home);
    if !real.starts_with(&home_real) {
        return Err(LauncherError::InvalidInput(format!(
            "иконку можно выбрать только из домашнего каталога: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Иконка инстанса: data-URL в instance.json (`icon`). Пользовательский файл
/// с диска: PNG/JPEG/WebP, до ~300 КБ (чтобы instance.json не раздувать).
/// Data-URL — фронт не зависит от asset-протокола и его областей доступа.
pub fn set_icon(paths: &Paths, id: &str, path: &str) -> Result<Instance> {
    let p = std::path::Path::new(path);
    ensure_icon_source_allowed(p)?;
    let data = std::fs::read(crate::util::fs::long_path(p))
        .map_err(|_| LauncherError::NotFound(format!("файл иконки: {path}")))?;
    set_icon_from_bytes(paths, id, &data)
}

/// Та же иконка, но из готовых байт — установка модпака (скачанный icon_url
/// проекта Modrinth, в т.ч. webp). Проверки как у set_icon: лимит ~300 КБ и
/// снифф формата по сигнатуре (не по расширению), результат — data-URL в
/// instance.json. Байты приходят из сети, а не из произвольного пути, поэтому
/// ensure_icon_source_allowed здесь не применяется.
pub fn set_icon_from_bytes(paths: &Paths, id: &str, data: &[u8]) -> Result<Instance> {
    const MAX_ICON_BYTES: usize = 300 * 1024;
    if data.len() > MAX_ICON_BYTES {
        return Err(LauncherError::InvalidInput(
            "иконка больше 300 КБ".into(),
        ));
    }
    let mime = sniff_image(data)
        .ok_or_else(|| LauncherError::InvalidInput("поддерживаются PNG, JPEG и WebP".into()))?;
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    let mut inst = crate::instances::load(paths, id)?;
    inst.icon = Some(format!("data:{mime};base64,{b64}"));
    crate::instances::save(paths, &inst)?;
    Ok(inst)
}

/// Убрать иконку инстанса.
pub fn clear_icon(paths: &Paths, id: &str) -> Result<Instance> {
    let mut inst = crate::instances::load(paths, id)?;
    inst.icon = None;
    crate::instances::save(paths, &inst)?;
    Ok(inst)
}

fn sniff_image(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.len() > 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{save_content_manifest, Instance};

    fn setup() -> (tempfile::TempDir, Paths, Instance) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let inst = Instance::new("Тест", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        std::fs::create_dir_all(long(&mc.join("mods"))).unwrap();
        std::fs::create_dir_all(long(&mc.join("saves/w1"))).unwrap();
        std::fs::create_dir_all(long(&mc.join("logs"))).unwrap();
        std::fs::write(long(&mc.join("mods/a.jar")), b"AAA").unwrap();
        std::fs::write(long(&mc.join("saves/w1/level.dat")), b"W").unwrap();
        std::fs::write(long(&mc.join("logs/latest.log")), b"L").unwrap();
        (dir, paths, inst)
    }

    fn long(p: &std::path::Path) -> PathBuf {
        crate::util::fs::long_path(p)
    }

    fn zip_names<R: std::io::Read + std::io::Seek>(z: &mut zip::ZipArchive<R>) -> Vec<String> {
        (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().to_string())
            .collect()
    }

    fn entry(file: &str, url: Option<&str>) -> ContentEntry {
        ContentEntry {
            kind: crate::instances::ContentKind::Mod,
            file: file.into(),
            source: crate::instances::ContentSource::Modrinth,
            project_id: Some("aaa".into()),
            version_id: Some("v1".into()),
            sha1: Some("deadbeef".into()),
            url: url.map(|s| s.into()),
            enabled: true,
        }
    }

    #[test]
    fn toggle_renames_file_and_flips_flag() {
        let (_d, paths, inst) = setup();
        save_content_manifest(&paths, &inst.id, &[entry("mods/a.jar", None)]).unwrap();
        let e = toggle(&paths, &inst.id, "mods/a.jar").unwrap();
        assert!(!e.enabled);
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        assert!(!long(&mc.join("mods/a.jar")).exists());
        assert!(long(&mc.join("mods/a.jar.disabled")).exists());
        // Обратно.
        let e = toggle(&paths, &inst.id, "mods/a.jar").unwrap();
        assert!(e.enabled);
        assert!(long(&mc.join("mods/a.jar")).exists());
    }

    /// (а) Запись выключена, но на диске только активный файл: включение —
    /// переворот записи, файл не трогаем (раньше здесь был ложный NotFound).
    #[test]
    fn toggle_enable_when_active_file_already_on_disk() {
        let (_d, paths, inst) = setup();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        let mut e = entry("mods/a.jar", None);
        e.enabled = false;
        save_content_manifest(&paths, &inst.id, &[e]).unwrap();
        let out = toggle(&paths, &inst.id, "mods/a.jar").unwrap();
        assert!(out.enabled);
        assert!(long(&mc.join("mods/a.jar")).exists());
        assert!(!long(&mc.join("mods/a.jar.disabled")).exists());
    }

    /// (б) Запись включена, активного нет, лежит .disabled: выключение сходится
    /// к целевому состоянию без ошибки, файлы не трогаем.
    #[test]
    fn toggle_disable_when_disabled_file_already_on_disk() {
        let (_d, paths, inst) = setup();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        std::fs::remove_file(long(&mc.join("mods/a.jar"))).unwrap();
        std::fs::write(long(&mc.join("mods/a.jar.disabled")), b"AAA").unwrap();
        save_content_manifest(&paths, &inst.id, &[entry("mods/a.jar", None)]).unwrap();
        let out = toggle(&paths, &inst.id, "mods/a.jar").unwrap();
        assert!(!out.enabled);
        assert!(long(&mc.join("mods/a.jar.disabled")).exists());
        assert!(!long(&mc.join("mods/a.jar")).exists());
    }

    /// (в) Ни одного файла на диске (ни активного, ни .disabled) — NotFound.
    #[test]
    fn toggle_without_any_file_on_disk_is_not_found() {
        let (_d, paths, inst) = setup();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        std::fs::remove_file(long(&mc.join("mods/a.jar"))).unwrap();
        save_content_manifest(&paths, &inst.id, &[entry("mods/a.jar", None)]).unwrap();
        let err = toggle(&paths, &inst.id, "mods/a.jar").unwrap_err();
        assert_eq!(err.code(), "not_found");
        // Запись не перевёрнута.
        assert!(load_content_manifest(&paths, &inst.id)[0].enabled);
    }

    #[test]
    fn toggle_unknown_entry_and_traversal_fail() {
        let (_d, paths, inst) = setup();
        assert!(toggle(&paths, &inst.id, "mods/ghost.jar").is_err());
        assert!(resolve_content_file(&paths, &inst.id, "../escape").is_err());
        assert!(resolve_content_file(&paths, &inst.id, "saves/w1/x").is_err());
        assert!(resolve_content_file(&paths, &inst.id, "mods/sub/../../x").is_err());
        assert!(resolve_content_file(&paths, &inst.id, "mods/C:/x").is_err());
    }

    #[test]
    fn remove_deletes_both_variants_and_manifest_entry() {
        let (_d, paths, inst) = setup();
        save_content_manifest(&paths, &inst.id, &[entry("mods/a.jar", None)]).unwrap();
        // Выключим (файл станет .disabled), затем удалим.
        toggle(&paths, &inst.id, "mods/a.jar").unwrap();
        remove(&paths, &inst.id, "mods/a.jar").unwrap();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        assert!(!long(&mc.join("mods/a.jar.disabled")).exists());
        assert!(load_content_manifest(&paths, &inst.id).is_empty());
        assert!(remove(&paths, &inst.id, "mods/a.jar").is_err());
    }

    #[test]
    fn backup_contains_data_but_not_noise() {
        let (_d, paths, inst) = setup();
        let res = backup(&paths, &inst).unwrap();
        assert!(res.files >= 3, "ожидаем instance.json, сейв, мод; {}", res.files);
        let f = std::fs::File::open(long(std::path::Path::new(&res.path))).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let names = zip_names(&mut z);
        assert!(names.iter().any(|n| n == "instance/instance.json"));
        assert!(names.iter().any(|n| n == "instance/minecraft/saves/w1/level.dat"));
        assert!(!names.iter().any(|n| n.contains("logs/")));
    }

    #[test]
    fn export_mrpack_index_and_overrides() {
        let (_d, paths, mut inst) = setup();
        inst.loader = Some("fabric".into());
        inst.loader_version = Some("0.16.0".into());
        crate::instances::save(&paths, &inst).unwrap();
        // Мод с url → files[]; локальный мод → overrides.
        let local = ContentEntry {
            source: crate::instances::ContentSource::Local,
            file: "mods/local.jar".into(),
            project_id: None,
            version_id: None,
            sha1: Some("cafe".into()),
            url: None,
            kind: crate::instances::ContentKind::Mod,
            enabled: true,
        };
        std::fs::write(long(&minecraft_dir(&instance_dir(&paths, &inst.id)).join("mods/local.jar")), b"LOC").unwrap();
        save_content_manifest(&paths, &inst.id, &[entry("mods/a.jar", Some("https://example/a.jar")), local]).unwrap();

        let res = export_mrpack(&paths, &inst).unwrap();
        assert!(res.path.ends_with(".mrpack"));
        let f = std::fs::File::open(long(std::path::Path::new(&res.path))).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let mut idx_raw = Vec::new();
        {
            use std::io::Read as _;
            z.by_name("modrinth.index.json").unwrap().read_to_end(&mut idx_raw).unwrap();
        }
        let idx: serde_json::Value = serde_json::from_slice(&idx_raw).unwrap();
        assert_eq!(idx["formatVersion"], 1);
        assert_eq!(idx["dependencies"]["minecraft"], "1.20.1");
        assert_eq!(idx["dependencies"]["fabric-loader"], "0.16.0");
        let files = idx["files"].as_array().unwrap();
        assert_eq!(files.len(), 1, "только запись с url");
        assert_eq!(files[0]["path"], "mods/a.jar");
        assert_eq!(files[0]["downloads"][0], "https://example/a.jar");
        // Локальный мод в overrides, дубль файла из files[] — нет, сейвы — нет.
        let names = zip_names(&mut z);
        assert!(names.iter().any(|n| n == "overrides/mods/local.jar"));
        assert!(!names.iter().any(|n| n == "overrides/mods/a.jar"));
        assert!(!names.iter().any(|n| n.contains("saves")));
    }

    #[test]
    fn icon_set_clear_and_sniff() {
        let (_d, paths, inst) = setup();
        let png = [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let icon_path = _d.path().join("icon.png");
        std::fs::write(&icon_path, png).unwrap();
        let updated = set_icon(&paths, &inst.id, icon_path.to_str().unwrap()).unwrap();
        let icon = updated.icon.expect("иконка установлена");
        assert!(icon.starts_with("data:image/png;base64,"));
        let cleared = clear_icon(&paths, &inst.id).unwrap();
        assert!(cleared.icon.is_none());
        // Не-изображение и отсутствующий файл — честные ошибки.
        let bad = _d.path().join("bad.png");
        std::fs::write(&bad, b"not an image at all").unwrap();
        assert!(set_icon(&paths, &inst.id, bad.to_str().unwrap()).is_err());
        assert!(set_icon(&paths, &inst.id, _d.path().join("nope.png").to_str().unwrap()).is_err());
    }

    /// Иконка из готовых байт (установка модпака: icon_url проекта Modrinth):
    /// data-URL как у set_icon, webp принимается; не-изображение и >300 КБ —
    /// честные ошибки.
    #[test]
    fn icon_from_bytes_sets_and_validates() {
        let (_d, paths, inst) = setup();
        let png = [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let updated = set_icon_from_bytes(&paths, &inst.id, &png).unwrap();
        assert!(
            updated.icon.expect("иконка установлена").starts_with("data:image/png;base64,"),
            "png → data-URL"
        );
        // WebP — частый формат иконок Modrinth (снифф по RIFF/WEBP, len > 12).
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&[0u8; 4]);
        webp.extend_from_slice(b"WEBPVP8X");
        let updated = set_icon_from_bytes(&paths, &inst.id, &webp).unwrap();
        assert!(
            updated.icon.expect("webp установлен").starts_with("data:image/webp;base64,"),
            "webp → data-URL"
        );
        // Не-изображение.
        let err = set_icon_from_bytes(&paths, &inst.id, b"not an image at all").unwrap_err();
        assert_eq!(err.code(), "invalid_input");
        // Больше 300 КБ — отказ, даже если сигнатура валидная.
        let mut big = png.to_vec();
        big.resize(300 * 1024 + 1, 0);
        let err = set_icon_from_bytes(&paths, &inst.id, &big).unwrap_err();
        assert_eq!(err.code(), "invalid_input", "{err}");
    }

    /// A16: относительный путь и путь вне домашнего каталога отклоняются до
    /// чтения файла (эксфильтрация/оракул файлов с диска).
    #[test]
    fn icon_rejects_path_outside_home() {
        let (_d, paths, inst) = setup();
        // Относительный путь — не абсолютный.
        let err = set_icon(&paths, &inst.id, "icon.png").unwrap_err();
        assert_eq!(err.code(), "invalid_input");
        // Файл вне home: системный файл (в home пользователя его нет).
        #[cfg(windows)]
        let outside = r"C:\Windows\System32\drivers\etc\hosts";
        #[cfg(not(windows))]
        let outside = "/etc/hosts";
        let err = set_icon(&paths, &inst.id, outside).unwrap_err();
        assert_eq!(err.code(), "invalid_input", "путь вне home отклонён: {outside}");
        // Traversal через `..` из home наружу тоже не проходит.
        let home = dirs::home_dir().expect("домашний каталог");
        let escape = home.join("../outside.png");
        assert!(set_icon(&paths, &inst.id, escape.to_str().unwrap()).is_err());
    }

    /// F10: копия мода между инстансами — файл на диске приёмника и запись
    /// в его манифесте (enabled=true, метаданные сохранены); повтор — «уже
    /// установлен»; чужой файл — InvalidInput; нет приёмника — NotFound.
    #[test]
    fn copy_entry_copies_file_and_manifest_record() {
        let (_d, paths, from) = setup();
        let to = Instance::new("Приёмник", "1.20.1");
        crate::instances::save(&paths, &to).unwrap();
        save_content_manifest(&paths, &from.id, &[entry("mods/a.jar", None)]).unwrap();

        let e = copy_entry(&paths, &from.id, &to.id, "mods/a.jar").unwrap();
        assert!(e.enabled, "копия включена");
        assert_eq!(e.project_id.as_deref(), Some("aaa"), "метаданные сохранены");
        assert_eq!(e.kind, crate::instances::ContentKind::Mod);
        let mc = minecraft_dir(&instance_dir(&paths, &to.id));
        assert_eq!(
            std::fs::read(long(&mc.join("mods/a.jar"))).unwrap(),
            b"AAA",
            "файл скопирован на диск"
        );
        let manifest = load_content_manifest(&paths, &to.id);
        assert_eq!(manifest.len(), 1);
        assert_eq!(manifest[0].file, "mods/a.jar");

        // Повторная копия того же файла — «уже установлен».
        let err = copy_entry(&paths, &from.id, &to.id, "mods/a.jar").unwrap_err();
        assert_eq!(err.code(), "invalid_input");
        // Нет записи в манифесте-источнике.
        let err = copy_entry(&paths, &from.id, &to.id, "mods/ghost.jar").unwrap_err();
        assert_eq!(err.code(), "invalid_input");
        // Приёмника не существует.
        let err = copy_entry(&paths, &from.id, "ghost", "mods/a.jar").unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    /// F10/B9: копирование при работающем инстансе (любом из двух) запрещено.
    #[test]
    fn copy_entry_rejects_running_instance() {
        let (_d, paths, from) = setup();
        let to = Instance::new("Приёмник", "1.20.1");
        crate::instances::save(&paths, &to).unwrap();
        save_content_manifest(&paths, &from.id, &[entry("mods/a.jar", None)]).unwrap();
        // Имитируем живой процесс источника: .lock с PID текущего теста.
        std::fs::write(instance_dir(&paths, &from.id).join(".lock"), std::process::id().to_string())
            .unwrap();
        let err = copy_entry(&paths, &from.id, &to.id, "mods/a.jar").unwrap_err();
        assert_eq!(err.code(), "instance_running");
    }

    /// F7: каталог datapacks — полноправный контент (путь проходит валидацию,
    /// вкл/выкл и удаление работают). kind в записи здесь Mod — валидация пути
    /// в content.rs строковая, вариант ContentKind::Datapack добавляет mod.rs.
    #[test]
    fn datapacks_dir_is_managed_content() {
        let (_d, paths, inst) = setup();
        assert!(resolve_content_file(&paths, &inst.id, "datapacks/w.zip").is_ok());
        assert!(resolve_content_file(&paths, &inst.id, "datapacks/../evil").is_err());
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        std::fs::create_dir_all(long(&mc.join("datapacks"))).unwrap();
        std::fs::write(long(&mc.join("datapacks/w.zip")), b"DP").unwrap();
        save_content_manifest(&paths, &inst.id, &[entry("datapacks/w.zip", None)]).unwrap();
        let e = toggle(&paths, &inst.id, "datapacks/w.zip").unwrap();
        assert!(!e.enabled);
        assert!(long(&mc.join("datapacks/w.zip.disabled")).exists());
        remove(&paths, &inst.id, "datapacks/w.zip").unwrap();
        assert!(load_content_manifest(&paths, &inst.id).is_empty());
    }
}
