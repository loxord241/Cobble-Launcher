//! Инстансы (спека §6.12, §7): изоляция, instance.json атомарно,
//! удаление в корзину ОС, `.lock` на время работы.

pub mod content;
pub mod repair;
pub mod configs;
pub mod screens;
pub mod modmeta;
pub mod packart;
pub mod shortcut;
pub mod snapshots;
pub mod worlds;
pub mod run;
pub mod store;

use crate::errors::{LauncherError, Result};
use crate::util::fs::{atomic_write, long_path};
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Instance {
    #[serde(alias = "schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    #[serde(alias = "mc_version")]
    pub mc_version: String,
    pub loader: Option<String>,
    #[serde(alias = "loader_version")]
    pub loader_version: Option<String>,
    /// id version JSON, который запускаем (после установки загрузчика:
    /// `fabric-loader-…`, `neoforge-…`). None = чистый mc_version.
    #[serde(alias = "version_id")]
    pub version_id: Option<String>,
    #[serde(alias = "java_path")]
    pub java_path: Option<String>,
    #[serde(alias = "ram_mb")]
    pub ram_mb: u32,
    #[serde(alias = "jvm_flags")]
    pub jvm_flags: Vec<String>,
    #[serde(alias = "game_args_extra")]
    pub game_args_extra: Vec<String>,
    #[serde(alias = "created_at")]
    pub created_at: u64,
    #[serde(alias = "last_played")]
    pub last_played: Option<u64>,
    #[serde(alias = "play_seconds")]
    pub play_seconds: u64,
    #[serde(alias = "launch_count")]
    pub launch_count: u64,
    pub notes: String,
    /// Быстрый запуск (F1, MC 1.20+): имя мира в saves для
    /// `--quickPlaySingleplayer`. Одновременно с quick_play_server запрещено.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_play_world: Option<String>,
    /// Быстрый запуск (F1, MC 1.20+): host:port для `--quickPlayMultiplayer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_play_server: Option<String>,
    /// Профиль для запуска именно этого инстанса (F11); None — как в лаунчере.
    /// Хранится только ссылка (id аккаунта), не сам профиль.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Подряд упавших запусков (F30): !=0 инкремент, успех/0 — сброс.
    #[serde(default)]
    pub crash_count: u32,
}

impl Default for Instance {
    /// Для serde(default): файлы без новых полей читаются; валидность
    /// проверяется на уровне загрузчика (mc_version обязателен по факту).
    fn default() -> Self {
        Self::new("Инстанс", "")
    }
}

impl Instance {
    pub fn new(name: &str, mc_version: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            id: uuid::Uuid::new_v4().to_string(),
            name: crate::util::names::sanitize_user_name(name, "Инстанс"),
            icon: None,
            mc_version: mc_version.into(),
            loader: None,
            loader_version: None,
            version_id: None,
            java_path: None,
            ram_mb: 2048,
            jvm_flags: Vec::new(),
            game_args_extra: Vec::new(),
            created_at: now_secs(),
            last_played: None,
            play_seconds: 0,
            launch_count: 0,
            notes: String::new(),
            quick_play_world: None,
            quick_play_server: None,
            account_id: None,
            crash_count: 0,
        }
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Каталог конкретного инстанса.
pub fn instance_dir(paths: &crate::paths::Paths, id: &str) -> PathBuf {
    paths.instances_dir().join(id)
}

/// Каталог `versions/` инстанса — JSON загрузчиков живут здесь (спека §6.3).
pub fn instance_versions_dir(paths: &crate::paths::Paths, id: &str) -> PathBuf {
    instance_dir(paths, id).join("versions")
}

/// Тип установленного контента (спека §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Mod,
    ResourcePack,
    Shader,
    Datapack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentSource {
    Modrinth,
    CurseForge,
    Local,
}

/// Запись content-manifest.json (спека §7) — база обновлений.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentEntry {
    pub kind: ContentKind,
    /// Путь относительно minecraft/ (например, `mods/sodium.jar`).
    pub file: String,
    pub source: ContentSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    /// URL загрузки на Modrinth — для экспорта .mrpack (files[]). Записи
    /// без url (Local/старые манифесты) при экспорте уходят в overrides/.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub enabled: bool,
}

pub fn content_manifest_path(paths: &crate::paths::Paths, id: &str) -> PathBuf {
    instance_dir(paths, id).join("content-manifest.json")
}

/// Загрузить манифест контента (нет файла → пустой).
pub fn load_content_manifest(paths: &crate::paths::Paths, id: &str) -> Vec<ContentEntry> {
    let path = content_manifest_path(paths, id);
    let long = long_path(&path);
    if !long.exists() {
        return Vec::new();
    }
    match std::fs::read(&long)
        .map_err(LauncherError::from)
        .and_then(|d| serde_json::from_slice::<Vec<ContentEntry>>(&d).map_err(LauncherError::from))
    {
        Ok(list) => list,
        Err(e) => {
            tracing::warn!("битый content-manifest {id}: {e}");
            Vec::new()
        }
    }
}

/// Атомарно сохранить манифест контента.
pub fn save_content_manifest(
    paths: &crate::paths::Paths,
    id: &str,
    entries: &[ContentEntry],
) -> Result<()> {
    let data = serde_json::to_vec_pretty(entries)?;
    atomic_write(&content_manifest_path(paths, id), &data)
}

/// Каталог игры инстанса (всё записываемое — внутри, спека §6.12).
pub fn minecraft_dir(instance_dir: &Path) -> PathBuf {
    instance_dir.join("minecraft")
}

fn instance_file(dir: &Path) -> PathBuf {
    dir.join("instance.json")
}

/// Строгая проверка id инстанса: только ASCII-буквы/цифры, `-`, `_`.
/// Единственный барьер против path traversal в IPC-командах, принимающих id
/// (`..`, разделители, `:` не проходят) — A1/A23.
pub(crate) fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(LauncherError::InvalidInput(format!("некорректный id инстанса: {id}")));
    }
    Ok(())
}

pub fn load(paths: &crate::paths::Paths, id: &str) -> Result<Instance> {
    valid_id(id)?;
    let path = instance_file(&instance_dir(paths, id));
    if !path.exists() {
        return Err(LauncherError::NotFound(format!("инстанс {id}")));
    }
    let data = std::fs::read(long_path(&path))?;
    Ok(serde_json::from_slice(&data)?)
}

/// Мьютекс записи `instance.json`: супервизор игры (run.rs) и настройки из UI
/// (instance_settings_set) пишут один файл из разных потоков — без сериализации
/// последний писатель затирает свежие поля другого (A28/D19).
static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn save(paths: &crate::paths::Paths, instance: &Instance) -> Result<()> {
    valid_id(&instance.id)?;
    // Отравленный мьютекс не должен каскадно ронять все последующие записи.
    let _guard = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    save_locked(paths, instance)
}

/// Внутренняя запись БЕЗ захвата SAVE_LOCK (вызывается из save/update).
fn save_locked(paths: &crate::paths::Paths, instance: &Instance) -> Result<()> {
    let dir = instance_dir(paths, &instance.id);
    std::fs::create_dir_all(long_path(&dir))?;
    let data = serde_json::to_vec_pretty(instance)?;
    atomic_write(&instance_file(&dir), &data)
}

/// Атомарное обновление инстанса (A28): load → modify → save под SAVE_LOCK
/// одним куском. Супервизор игры обязан писать статистику только через него,
/// иначе затирает свежие настройки из UI (и наоборот).
pub fn update<F: FnOnce(&mut Instance)>(
    paths: &crate::paths::Paths,
    id: &str,
    f: F,
) -> Result<()> {
    let _guard = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut inst = load(paths, id)?;
    f(&mut inst);
    save_locked(paths, &inst)
}

/// Пределы RAM инстанса (МБ): ниже игра не стартует, выше — `-Xmx` съедает
/// память машины (спека §6.9).
pub const MIN_RAM_MB: u32 = 256;
pub const MAX_RAM_MB: u32 = 65536;

/// Подстроки JVM-флагов, запрещённые в настройках инстанса (A3). Агенты и
/// колбэки JVM исполняют произвольный код в ОС при старте игры:
/// `-javaagent:evil.jar`, `-agentlib:…`, `-Xrun…`, `-XX:OnError="cmd /c …"`.
const FORBIDDEN_JVM_SUBSTRINGS: &[&str] = &[
    "-javaagent",
    "-agentlib",
    "-agentpath",
    "-xrun",
    // Колбэк-формы запрещаем только с "=" — безобидный
    // -XX:+HeapDumpOnOutOfMemoryError без "=" остаётся разрешённым.
    "onerror=",
    "onoutofmemoryerror=",
];

/// Валидация настроек запуска на IPC-границе (A3): опасные JVM-флаги и
/// аргументы игры, RAM вне диапазона, относительный `java_path` — отклоняются.
/// Легитимный `-javaagent` для ely.by собирает сам лаунчер (auth::ely), а не
/// это поле, поэтому белый список не нужен — достаточно чёрного.
pub fn validate_instance_settings(inst: &Instance) -> Result<()> {
    if !(MIN_RAM_MB..=MAX_RAM_MB).contains(&inst.ram_mb) {
        return Err(LauncherError::InvalidInput(format!(
            "RAM инстанса {0} МБ вне диапазона {MIN_RAM_MB}–{MAX_RAM_MB} МБ",
            inst.ram_mb
        )));
    }
    for arg in inst.jvm_flags.iter().chain(inst.game_args_extra.iter()) {
        let low = arg.to_ascii_lowercase();
        if let Some(bad) = FORBIDDEN_JVM_SUBSTRINGS.iter().find(|b| low.contains(*b)) {
            return Err(LauncherError::InvalidInput(format!(
                "недопустимый аргумент запуска «{arg}» (совпадение с {bad})"
            )));
        }
    }
    if let Some(java) = &inst.java_path {
        let trimmed = java.trim();
        if !trimmed.is_empty() && !Path::new(trimmed).is_absolute() {
            return Err(LauncherError::InvalidInput(
                "путь к Java должен быть абсолютным".into(),
            ));
        }
    }
    // F1: quick play. Обе цели сразу запрещены — игра выбрала бы одну,
    // а пользователь не понял бы, какая.
    if inst.quick_play_world.is_some() && inst.quick_play_server.is_some() {
        return Err(LauncherError::InvalidInput(
            "быстрый запуск: нельзя задать одновременно и мир, и сервер".into(),
        ));
    }
    if let Some(world) = &inst.quick_play_world {
        if !valid_quick_play_world(world) {
            return Err(LauncherError::InvalidInput(format!(
                "некорректное имя мира для быстрого запуска: {world:?} (латиница/цифры/_/-, до 64)"
            )));
        }
    }
    if let Some(server) = &inst.quick_play_server {
        if !valid_quick_play_server(server) {
            return Err(LauncherError::InvalidInput(format!(
                "некорректный адрес сервера для быстрого запуска: {server:?} (host[:port])"
            )));
        }
    }
    // F11: ссылка на профиль — только безопасные символы id (UUID аккаунтов
    // вписывается). Существование проверять здесь нечего: удалённый профиль
    // обрабатывает UI (показывает глобальный и не хранит висячую ссылку).
    if let Some(acc) = &inst.account_id {
        if acc.is_empty()
            || acc.len() > 64
            || !acc
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(LauncherError::InvalidInput(format!(
                "некорректный id профиля в настройках инстанса: {acc}"
            )));
        }
    }
    Ok(())
}

/// Имя мира для quick play: имя каталога в saves — латиница/цифры/`_`/`-`, 1–64.
/// Регекс по образцу settings::is_hex_color (крейт regex уже в зависимостях).
fn valid_quick_play_world(world: &str) -> bool {
    regex::Regex::new(r"^[A-Za-z0-9_\-]{1,64}$")
        .map(|re| re.is_match(world))
        .unwrap_or(false)
}

/// Адрес сервера для quick play: host ([A-Za-z0-9.-]) с опциональным портом
/// 1–5 цифр, вся строка ≤255 (лимит на host:port в аргументе игры).
fn valid_quick_play_server(addr: &str) -> bool {
    if addr.len() > 255 {
        return false;
    }
    regex::Regex::new(r"^[A-Za-z0-9.\-]+(:[0-9]{1,5})?$")
        .map(|re| re.is_match(addr))
        .unwrap_or(false)
}

/// Все инстансы, отсортированные по имени. Битые instance.json пропускаются
/// с предупреждением (не роняем весь список).
pub fn list(paths: &crate::paths::Paths) -> Vec<Instance> {
    let root = paths.instances_dir();
    let Ok(entries) = std::fs::read_dir(long_path(&root)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let f = e.path().join("instance.json");
        if !f.exists() {
            continue;
        }
        match std::fs::read(long_path(&f))
            .map_err(LauncherError::from)
            .and_then(|d| serde_json::from_slice::<Instance>(&d).map_err(LauncherError::from))
        {
            Ok(inst) => out.push(inst),
            Err(e) => tracing::warn!("битый instance.json в {f:?}: {e}"),
        }
    }
    out.sort_by_key(|i| i.name.to_lowercase());
    out
}

pub fn rename(paths: &crate::paths::Paths, id: &str, new_name: &str) -> Result<Instance> {
    let dir = instance_dir(paths, id);
    if running_pid(&dir).is_some() {
        return Err(LauncherError::InvalidInput(
            "нельзя переименовать работающий инстанс: сначала остановите его".into(),
        ));
    }
    let mut inst = load(paths, id)?;
    inst.name = crate::util::names::sanitize_user_name(new_name, &inst.name);
    save(paths, &inst)?;
    Ok(inst)
}

/// Куда нельзя жёстко линковать при дублировании: уникальные данные инстанса.
pub const UNIQUE_SUBDIRS: &[&str] = &[
    "saves",
    "mods",
    "resourcepacks",
    "shaderpacks",
    "datapacks",
    "snapshots",
    "screenshots",
    "config",
    "logs",
    ".trash",
];

pub const UNIQUE_FILES: &[&str] = &["options.txt", "servers.dat", "usercache.json"];

/// Дублирование: общее (клиент, библиотеки, нативы) — hardlink из исходника,
/// уникальное (saves/mods/config…) — копия (спека §6.12). Запись в hardlink
/// через одну из копий меняла бы обе — поэтому уникальное копируется байтами.
pub fn duplicate(paths: &crate::paths::Paths, id: &str) -> Result<Instance> {
    let src_dir = instance_dir(paths, id);
    if running_pid(&src_dir).is_some() {
        return Err(LauncherError::InvalidInput(
            "нельзя дублировать работающий инстанс: сначала остановите его".into(),
        ));
    }
    let src = load(paths, id)?;
    let mut inst = Instance::new(&format!("{} (копия)", src.name), &src.mc_version);
    inst.loader = src.loader.clone();
    inst.loader_version = src.loader_version.clone();
    inst.ram_mb = src.ram_mb;
    inst.jvm_flags = src.jvm_flags.clone();
    inst.game_args_extra = src.game_args_extra.clone();
    inst.notes = src.notes.clone();
    let dst_dir = instance_dir(paths, &inst.id);
    std::fs::create_dir_all(long_path(&dst_dir))?;
    store::copy_tree_mixed(&minecraft_dir(&src_dir), &minecraft_dir(&dst_dir))?;
    save(paths, &inst)?;
    Ok(inst)
}

/// Удаление: в корзину ОС (восстановимо) или насовсем (спека §6.12).
/// Store-контент (libraries/versions/bin — hardlinks из стора) удаляем
/// напрямую: данные остаются в сторе, а корзина на таких деревьях падает.
/// В корзину идёт уникальное: saves/mods/config/instance.json.
pub fn delete(paths: &crate::paths::Paths, id: &str, wipe: bool) -> Result<()> {
    // Без валидации id `..\backups` с wipe:true удалял бы каталог бэкапов,
    // а `..\..` — что угодно в данных/домашнем каталоге (A1).
    valid_id(id)?;
    let dir = instance_dir(paths, id);
    if !dir.exists() {
        return Err(LauncherError::NotFound(format!("инстанс {id}")));
    }
    if running_pid(&dir).is_some() {
        return Err(LauncherError::InvalidInput(
            "нельзя удалить работающий инстанс: сначала остановите его".into(),
        ));
    }
    let mc = minecraft_dir(&dir);
    for shared in ["libraries", "versions", "bin"] {
        let p = mc.join(shared);
        if p.exists() {
            std::fs::remove_dir_all(long_path(&p))?;
        }
    }
    if wipe {
        std::fs::remove_dir_all(long_path(&dir))?;
    } else {
        recycle_dir(&dir)?;
    }
    Ok(())
}

/// Экранирование значения для PowerShell-литерала в одинарных кавычках:
/// внутри `'…'` спецсимвол только один — сама кавычка, и она удваивается.
/// Без этого апостроф в пути (например `C:\Users\O'Brien\…`) разрывал бы
/// строку и позволял исполнить произвольный скрипт (A2).
#[cfg(windows)]
fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// Корзина ОС. Крейт `trash` на части систем падает с «operations aborted»,
/// поэтому — PowerShell + VisualBasic.FileIO (документированный путь Windows).
/// Скрипт передаётся отдельным аргументом `Command` (массив `args`, без
/// строковой интерполяции шелла), а путь внутри скрипта — экранированным
/// PS-литералом: инъекция через имя каталога невозможна (A2).
#[cfg(windows)]
fn recycle_dir(dir: &Path) -> Result<()> {
    let ps = format!(
        "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory('{}', 'OnlyErrorDialogs', 'SendToRecycleBin')",
        ps_quote(&dir.display().to_string())
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps]);
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = cmd.output()?;
    if out.status.success() {
        return Ok(());
    }
    Err(LauncherError::Internal(format!(
        "корзина: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    )))
}

#[cfg(not(windows))]
fn recycle_dir(dir: &Path) -> Result<()> {
    // Вне Windows корзину эмулируем переносом в .trash рядом с инстансами.
    let root = dir.parent().unwrap_or(Path::new(".")).join(".trash");
    std::fs::create_dir_all(long_path(&root))?;
    let name = format!(
        "{}-{}",
        dir.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        now_secs()
    );
    std::fs::rename(long_path(dir), long_path(&root.join(name)))?;
    Ok(())
}

/// PID запущенного инстанса из `.lock` (None = не запущен / протух).
/// Жив ли процесс по PID (для Discord-RPC-потока F26; не трогает .lock).
pub(crate) fn running_pid_alive(pid: u32) -> bool {
    pid_alive(pid)
}

pub fn running_pid(instance_dir: &Path) -> Option<u32> {
    let lock = instance_dir.join(".lock");
    let text = std::fs::read_to_string(long_path(&lock)).ok()?;
    let pid = text.trim().parse::<u32>().ok()?;
    if pid_alive(pid) {
        Some(pid)
    } else {
        None
    }
}

/// Kill: завершить дерево процессов игры (спека §6.10) и снять lock.
pub fn kill(instance_dir: &Path) -> Result<()> {
    let Some(pid) = running_pid(instance_dir) else {
        let _ = std::fs::remove_file(long_path(&instance_dir.join(".lock")));
        return Ok(());
    };
    #[cfg(windows)]
    {
        let mut c = std::process::Command::new("taskkill");
        c.args(["/F", "/T", "/PID", &pid.to_string()]);
        c.creation_flags(0x0800_0000);
        let out = c.output()?;
        if !out.status.success() {
            tracing::warn!(
                "taskkill PID {pid}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).output();
    }
    let _ = std::fs::remove_file(long_path(&instance_dir.join(".lock")));
    Ok(())
}

/// Запущенные инстансы (id, PID) — для process_status в UI.
pub fn running_instances(paths: &crate::paths::Paths) -> Vec<(String, u32)> {
    list(paths)
        .iter()
        .filter_map(|i| running_pid(&instance_dir(paths, &i.id)).map(|pid| (i.id.clone(), pid)))
        .collect()
}

/// `.lock` на время работы инстанса (защита от двойного запуска).
pub struct InstanceLock {
    path: PathBuf,
    expected_pid: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

/// Создать `.lock` атомарно: `create_new` отдаёт AlreadyExists, если файл уже
/// есть, — проверка «файла нет» и запись становятся одной операцией (D24).
fn create_lock_file(long: &Path, pid: u32) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(long)?;
    f.write_all(pid.to_string().as_bytes())?;
    f.sync_all()?;
    Ok(())
}

/// PID из `.lock`, если он там корректно записан. Пустой или битый файл → None:
/// такой lock нельзя ни снять (PID неизвестен — вдруг его пишет живой процесс
/// прямо сейчас), ни объявить своим.
fn lock_file_pid(long: &Path) -> Option<u32> {
    std::fs::read_to_string(long).ok()?.trim().parse::<u32>().ok()
}

/// Сообщение «занято» — одно на все ветки acquire, чтобы UI/i18n видели
/// тот же текст, что и до D24.
fn lock_busy() -> LauncherError {
    LauncherError::InvalidInput("инстанс уже запущен (.lock)".into())
}

impl InstanceLock {
    pub fn acquire(instance_dir: &Path) -> Result<Self> {
        let path = instance_dir.join(".lock");
        let long = long_path(&path);
        let my_pid = std::process::id();
        // Раньше было «проверил exists → записал» — межпроцессная гонка: два
        // запуска проходили проверку и оба писали свой PID. Теперь владелец
        // один: у кого create_new удался (D24).
        if let Err(e) = create_lock_file(&long, my_pid) {
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(e.into());
            }
            // Чужой .lock можно снять только протухший: в файле валидный PID,
            // а процесса с ним уже нет. Иначе — «занято», файл не трогаем.
            let Some(stale_pid) = lock_file_pid(&long).filter(|pid| !pid_alive(*pid)) else {
                return Err(lock_busy());
            };
            tracing::warn!("протухший .lock (PID {stale_pid} мёртв), снимаю: {long:?}");
            let _ = std::fs::remove_file(&long);
            // Гонка может повториться (lock перехватил третий процесс) —
            // тогда он и владелец.
            create_lock_file(&long, my_pid).map_err(|e| match e.kind() {
                std::io::ErrorKind::AlreadyExists => lock_busy(),
                _ => e.into(),
            })?;
        }
        Ok(Self {
            path,
            expected_pid: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(my_pid)),
        })
    }

    pub fn set_child_pid(&self, pid: u32) {
        self.expected_pid.store(pid, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let long = long_path(&self.path);
        if let Ok(text) = std::fs::read_to_string(&long) {
            let file_pid = text.trim().parse::<u32>().unwrap_or(0);
            let exp = self.expected_pid.load(std::sync::atomic::Ordering::SeqCst);
            if file_pid == exp || file_pid == std::process::id() {
                let _ = std::fs::remove_file(&long);
            }
        }
    }
}

/// Жив ли процесс с таким PID. Раньше здесь на каждый чек спавнился `tasklist`
/// (создание процесса и консоли на каждый инстанс в списке) — теперь один
/// снапшот списка процессов через sysinfo, без новых процессов (D16).
/// Семантика та же: «жив по PID»; несуществующий PID (в т.ч. отсутствующий в
/// снапшоте) — мёртв.
fn pid_alive(pid: u32) -> bool {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut sys = sysinfo::System::new();
    // Обновляем только запрошенный PID: остальные процессы не открываются.
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).is_some()
}

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths() -> (tempfile::TempDir, crate::paths::Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        (dir, paths)
    }

    #[test]
    fn create_save_load_roundtrip() {
        let (_d, paths) = test_paths();
        let mut inst = Instance::new("Мой <тест>: инстанс?", "1.20.1");
        inst.ram_mb = 4096;
        save(&paths, &inst).unwrap();
        let loaded = load(&paths, &inst.id).unwrap();
        assert_eq!(loaded.name, "Мой тест инстанс");
        assert_eq!(loaded.ram_mb, 4096);
        assert_eq!(loaded.mc_version, "1.20.1");
    }

    #[test]
    fn list_sorted_and_bad_skipped() {
        let (_d, paths) = test_paths();
        let mut a = Instance::new("b", "1.20.1");
        let mut b = Instance::new("A", "1.20.1");
        b.mc_version = "1.12.2".into();
        save(&paths, &a).unwrap();
        save(&paths, &b).unwrap();
        // Битый инстанс
        let dir = instance_dir(&paths, "broken-id");
        std::fs::create_dir_all(long_path(&dir)).unwrap();
        std::fs::write(dir.join("instance.json"), b"{ not json").unwrap();
        let all = list(&paths);
        assert_eq!(all.len(), 2, "битый пропущен");
        assert_eq!(all[0].name, "A");
        assert_eq!(all[1].name, "b");
        a.id.clone_from(&all[1].id);
    }

    #[test]
    fn invalid_id_rejected() {
        let (d, _paths) = test_paths();
        let p = crate::paths::Paths::new(d.path().to_path_buf());
        assert!(load(&p, "../etc").is_err());
        assert!(load(&p, "a/b").is_err());
    }

    #[test]
    fn lock_prevents_double_run_and_cleans_stale() {
        let (_d, paths) = test_paths();
        let inst = Instance::new("x", "1.20.1");
        save(&paths, &inst).unwrap();
        let dir = instance_dir(&paths, &inst.id);
        {
            let _lock = InstanceLock::acquire(&dir).unwrap();
            assert!(InstanceLock::acquire(&dir).is_err(), "второй запуск запрещён");
        }
        // После drop — свободен
        let _lock2 = InstanceLock::acquire(&dir).unwrap();
        // Протухший lock с заведомо невозможным PID (u32::MAX)
        std::fs::write(dir.join(".lock"), "4294967295").unwrap();
        let _lock3 = InstanceLock::acquire(&dir).unwrap();
    }

    #[test]
    fn duplicate_copies_unique_hardlinks_shared() {
        let (_d, paths) = test_paths();
        let inst = Instance::new("src", "1.20.1");
        save(&paths, &inst).unwrap();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        // shared: библиотека (store-подобная)
        let lib = mc.join("libraries/a/b.jar");
        std::fs::create_dir_all(long_path(lib.parent().unwrap())).unwrap();
        std::fs::write(&lib, b"shared bytes").unwrap();
        // unique: сейв
        let save = mc.join("saves/world/level.dat");
        std::fs::create_dir_all(long_path(save.parent().unwrap())).unwrap();
        std::fs::write(&save, b"unique world").unwrap();

        let copy = duplicate(&paths, &inst.id).unwrap();
        let mc2 = minecraft_dir(&instance_dir(&paths, &copy.id));
        let lib2 = mc2.join("libraries/a/b.jar");
        let save2 = mc2.join("saves/world/level.dat");
        assert_eq!(std::fs::read(&lib2).unwrap(), b"shared bytes");
        assert_eq!(std::fs::read(&save2).unwrap(), b"unique world");

        // Проверяем hardlink у shared (FileIndexId совпадает) — упрощённо:
        // запись в оригинал НЕ должна менять копию сейва, но должна — библиотеку.
        std::fs::write(&lib, b"CHANGED").unwrap();
        assert_eq!(std::fs::read(&lib2).unwrap(), b"CHANGED", "hardlink shared");
        std::fs::write(&save, b"EDITED").unwrap();
        assert_eq!(std::fs::read(&save2).unwrap(), b"unique world", "копия unique");
    }

    #[test]
    fn camelcase_serialization_and_snake_alias() {
        let inst = Instance::new("Test", "1.20.1");
        let json = serde_json::to_string(&inst).unwrap();
        assert!(json.contains("\"mcVersion\":\"1.20.1\""), "сериализуется в camelCase");
        assert!(json.contains("\"schemaVersion\":1"), "schemaVersion в camelCase");

        // Чтение старого формата со snake_case
        let old_json = r#"{
            "schema_version": 1,
            "id": "old-id",
            "name": "Old",
            "mc_version": "1.19.2",
            "ram_mb": 4096,
            "jvm_flags": [],
            "game_args_extra": [],
            "created_at": 12345,
            "play_seconds": 0,
            "launch_count": 0,
            "notes": ""
        }"#;
        let loaded: Instance = serde_json::from_str(old_json).unwrap();
        assert_eq!(loaded.mc_version, "1.19.2", "читает snake_case через alias");
        assert_eq!(loaded.ram_mb, 4096);
    }

    /// D16: живость PID по снапшоту sysinfo (без спавна `tasklist`).
    #[test]
    fn pid_alive_sees_own_process_and_dead_pid() {
        assert!(pid_alive(std::process::id()), "свой процесс жив");
        assert!(!pid_alive(u32::MAX), "несуществующий PID мёртв");
    }

    /// D24: `.lock` создаётся атомарно (create_new), поэтому чужой файл не
    /// перетирается. Пустой файл — это чужой lock в окне между созданием и
    /// записью PID: считаем «занято» и НЕ удаляем (иначе два процесса решили
    /// бы, что владеют инстансом). Снять можно только протухший lock.
    #[test]
    fn lock_create_new_keeps_foreign_file() {
        let (_d, paths) = test_paths();
        let inst = Instance::new("x", "1.20.1");
        save(&paths, &inst).unwrap();
        let dir = instance_dir(&paths, &inst.id);

        std::fs::write(dir.join(".lock"), "").unwrap();
        let Err(err) = InstanceLock::acquire(&dir) else {
            panic!("пустой чужой .lock должен считаться занятостью");
        };
        assert_eq!(err.code(), "invalid_input");
        assert!(dir.join(".lock").exists(), "чужой lock не удаляем");

        // Протухший lock (PID мёртв) снимаем и создаём свой: иначе после
        // падения игры инстанс остался бы «занятым» навсегда.
        std::fs::write(dir.join(".lock"), u32::MAX.to_string()).unwrap();
        let lock = InstanceLock::acquire(&dir).unwrap();
        let text = std::fs::read_to_string(dir.join(".lock")).unwrap();
        assert_eq!(text.trim(), std::process::id().to_string());
        drop(lock);
        assert!(!dir.join(".lock").exists(), "свой lock снимаем на выходе");
    }

    #[test]
    fn lock_no_cross_delete() {
        let (_d, paths) = test_paths();
        let inst = Instance::new("x", "1.20.1");
        save(&paths, &inst).unwrap();
        let dir = instance_dir(&paths, &inst.id);

        let lock1 = InstanceLock::acquire(&dir).unwrap();
        lock1.set_child_pid(1111);
        std::fs::write(dir.join(".lock"), "1111").unwrap();

        // Предположим, lock был перезаписан другим запуском с PID 2222
        std::fs::write(dir.join(".lock"), "2222").unwrap();

        // Когда первый lock завершается и дропается, он НЕ должен стереть файл с PID 2222
        drop(lock1);
        assert!(dir.join(".lock").exists(), "чужой lock не должен удаляться");
        assert_eq!(std::fs::read_to_string(dir.join(".lock")).unwrap().trim(), "2222");

        let _ = std::fs::remove_file(dir.join(".lock"));
    }

    /// A1: `instance_delete` не должен выходить за каталог инстансов
    /// (`..\backups` с wipe:true удалял каталог бэкапов).
    #[test]
    fn delete_rejects_traversal_id() {
        let (_d, paths) = test_paths();
        let victim = paths.root().join("backups");
        std::fs::create_dir_all(long_path(&victim)).unwrap();
        std::fs::write(victim.join("world.zip"), b"backup").unwrap();

        for id in ["..\\backups", "../backups", "..", ".", "a/b", ""] {
            let err = delete(&paths, id, true).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "id {id:?} отклонён");
        }
        assert!(long_path(&victim).exists(), "посторонний каталог не тронут");
        assert!(victim.join("world.zip").exists(), "бэкап на месте");
    }

    /// A3: опасные флаги JVM и аргументы игры отклоняются, нормальные проходят.
    #[test]
    fn validate_settings_rejects_dangerous_flags() {
        let mut inst = Instance::new("x", "1.20.1");
        for flag in [
            "-XX:OnError=calc.exe",
            "-XX:OnOutOfMemoryError=\"cmd /c powershell\"",
            "-javaagent:C:\\evil.jar",
            "-agentlib:jdwp=transport=dt_socket",
            "-agentpath:C:\\evil.dll",
            "-Xrunjdwp:transport=dt_socket",
        ] {
            inst.jvm_flags = vec![flag.to_string()];
            let err = validate_instance_settings(&inst).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "флаг {flag:?} отклонён");
        }
        // Аргументы игры тоже приходят по IPC — та же проверка.
        inst.jvm_flags.clear();
        inst.game_args_extra = vec!["--demo -javaagent:evil.jar".into()];
        assert!(validate_instance_settings(&inst).is_err(), "флаг в game_args_extra");
        // Нормальные настройки не задеты.
        inst.game_args_extra.clear();
        inst.jvm_flags = vec![
            "-XX:+UseG1GC".into(),
            "-XX:MaxGCPauseMillis=50".into(),
            "-Dfile.encoding=UTF-8".into(),
        ];
        assert!(validate_instance_settings(&inst).is_ok());
    }

    /// A3: RAM клампится, относительный путь к Java отклоняется.
    #[test]
    fn validate_settings_clamps_ram_and_java_path() {
        let mut inst = Instance::new("x", "1.20.1");
        inst.ram_mb = MIN_RAM_MB - 1;
        assert!(validate_instance_settings(&inst).is_err(), "мало RAM");
        inst.ram_mb = MAX_RAM_MB + 1;
        assert!(validate_instance_settings(&inst).is_err(), "много RAM");
        inst.ram_mb = 4096;
        assert!(validate_instance_settings(&inst).is_ok());
        inst.java_path = Some("java.exe".into());
        assert!(validate_instance_settings(&inst).is_err(), "относительный путь");
        inst.java_path = Some(String::new());
        assert!(validate_instance_settings(&inst).is_ok(), "пустой путь = авто");
        inst.java_path = Some(r"C:\Program Files\Java\bin\java.exe".into());
        assert!(validate_instance_settings(&inst).is_ok());
    }

    /// A2: удвоение одинарных кавычек — единственный спецсимвол PS-литерала.
    #[cfg(windows)]
    #[test]
    fn ps_quote_doubles_apostrophes() {
        assert_eq!(ps_quote(r"C:\Users\O'Brien\inst"), r"C:\Users\O''Brien\inst");
        assert_eq!(ps_quote("a'; calc; 'b"), "a''; calc; ''b");
        assert_eq!(ps_quote("без кавычек"), "без кавычек");
    }

    /// A2: каталог с апострофом и «payload» уходит в корзину, а сам payload
    /// остаётся данными. Без экранирования PowerShell разорвал бы литерал:
    /// `exit 42` исполнился бы (или скрипт не распарсился) — и вызов упал бы.
    #[cfg(windows)]
    #[test]
    fn recycle_dir_does_not_execute_injected_code() {
        let (_d, paths) = test_paths();
        let target = paths.root().join("Steve's; exit 42; '");
        std::fs::create_dir_all(long_path(&target)).unwrap();
        std::fs::write(target.join("data.txt"), b"x").unwrap();

        recycle_dir(&target).expect("каталог уходит в корзину, инъекция не исполнилась");
        assert!(!long_path(&target).exists(), "каталог перемещён в корзину");
    }

    /// A28: запись `instance.json` сериализуется глобальным мьютексом —
    /// супервизор игры и настройки из UI не пишут файл одновременно.
    #[test]
    fn save_waits_for_global_lock() {
        let (_d, paths) = test_paths();
        let inst = Instance::new("x", "1.20.1");
        let guard = SAVE_LOCK.lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (thread_paths, thread_inst) = (paths.clone(), inst.clone());
        let handle = std::thread::spawn(move || {
            let _ = tx.send(save(&thread_paths, &thread_inst).is_ok());
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(200)).is_err(),
            "save обязан ждать мьютекс, пока его держит другой поток"
        );
        drop(guard);
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap(),
            "после снятия лока запись проходит"
        );
        handle.join().unwrap();
    }

    #[test]
    fn delete_duplicate_rename_reject_running() {
        let (_d, paths) = test_paths();
        let inst = Instance::new("running-test", "1.20.1");
        save(&paths, &inst).unwrap();
        let dir = instance_dir(&paths, &inst.id);

        // Имитируем живой процесс текущего теста
        let current_pid = std::process::id();
        std::fs::write(dir.join(".lock"), current_pid.to_string()).unwrap();

        assert!(rename(&paths, &inst.id, "new-name").is_err());
        assert!(duplicate(&paths, &inst.id).is_err());
        assert!(delete(&paths, &inst.id, false).is_err());

        // Снимаем lock
        let _ = std::fs::remove_file(dir.join(".lock"));
        assert!(rename(&paths, &inst.id, "new-name").is_ok());
    }

    /// F1: quick play — имя мира и адрес сервера валидируются на IPC-границе,
    /// обе цели одновременно запрещены.
    #[test]
    fn validate_settings_quick_play_fields() {
        let mut inst = Instance::new("x", "1.20.1");
        // Мир: допустимые имена (каталог в saves).
        for world in ["NewWorld", "world_1", "a", "hardcore-", "A-b_C9"] {
            inst.quick_play_world = Some(world.to_string());
            assert!(validate_instance_settings(&inst).is_ok(), "мир {world:?} допустим");
        }
        // Мир: path traversal, кириллица, пустое, слишком длинное — отклоняются.
        let too_long_world = "x".repeat(65);
        let too_long_host = "h".repeat(256);
        for world in ["../evil", "a/b", "мир", "", too_long_world.as_str()] {
            inst.quick_play_world = Some(world.to_string());
            let err = validate_instance_settings(&inst).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "мир {world:?} отклонён");
        }
        // Сервер: host и host:port.
        for server in ["mc.example.com", "127.0.0.1", "localhost:25565", "a-b.c.d:1"] {
            inst.quick_play_world = None;
            inst.quick_play_server = Some(server.to_string());
            assert!(validate_instance_settings(&inst).is_ok(), "адрес {server:?} допустим");
        }
        // Сервер: путь, нецифровой порт, хвост без цифр, пустое, >255 — отклоняются.
        for server in ["host/path", "host:abc", "host:", "", "host:12a34", too_long_host.as_str()] {
            inst.quick_play_server = Some(server.to_string());
            let err = validate_instance_settings(&inst).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "адрес {server:?} отклонён");
        }
        // Обе цели сразу — InvalidInput.
        inst.quick_play_world = Some("world".into());
        inst.quick_play_server = Some("mc.example.com".into());
        let err = validate_instance_settings(&inst).unwrap_err();
        assert_eq!(err.code(), "invalid_input", "мир+сервер вместе запрещены");
        // Граница длины адреса: ровно 255 — допустимо (мир сброшен).
        inst.quick_play_world = None;
        inst.quick_play_server = Some(format!("{}:25565", "h".repeat(249)));
        assert_eq!(inst.quick_play_server.as_ref().unwrap().len(), 255);
        assert!(validate_instance_settings(&inst).is_ok(), "адрес длиной 255 допустим");
    }

    /// F11: ссылка на профиль — только безопасные символы id (UUID вписывается).
    #[test]
    fn validate_settings_account_id_charset() {
        let mut inst = Instance::new("x", "1.20.1");
        inst.account_id = Some("550e8400-e29b-41d4-a716-446655440000".into());
        assert!(validate_instance_settings(&inst).is_ok(), "UUID профиля допустим");
        let too_long_acc = "x".repeat(65);
        for acc in ["../evil", "a/b", "", too_long_acc.as_str()] {
            inst.account_id = Some(acc.to_string());
            let err = validate_instance_settings(&inst).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "id профиля {acc:?} отклонён");
        }
        inst.account_id = None;
        assert!(validate_instance_settings(&inst).is_ok(), "None = глобальный профиль");
    }

    /// F1/F11: новые поля сериализуются в camelCase, отсутствуют при None
    /// (skip_serializing_if) и читаются из старых файлов без них (serde default).
    #[test]
    fn quick_play_and_account_fields_serde() {
        let mut inst = Instance::new("Test", "1.20.1");
        inst.quick_play_world = Some("World_1".into());
        inst.account_id = Some("acc-1".into());
        let json = serde_json::to_string(&inst).unwrap();
        assert!(json.contains("\"quickPlayWorld\":\"World_1\""), "camelCase мир: {json}");
        assert!(json.contains("\"accountId\":\"acc-1\""), "camelCase профиль: {json}");
        assert!(!json.contains("quickPlayServer"), "None-сервер не сериализуется: {json}");

        // Читаем обратно; старый файл без новых полей даёт None.
        let reloaded: Instance = serde_json::from_str(&json).unwrap();
        assert_eq!(reloaded.quick_play_world.as_deref(), Some("World_1"));
        assert_eq!(reloaded.quick_play_server, None);
        assert_eq!(reloaded.account_id.as_deref(), Some("acc-1"));

        let old_json = r#"{"id":"old","name":"Old","mc_version":"1.19.2"}"#;
        let legacy: Instance = serde_json::from_str(old_json).unwrap();
        assert_eq!(legacy.quick_play_world, None, "старый файл: мир не задан");
        assert_eq!(legacy.quick_play_server, None, "старый файл: сервер не задан");
        assert_eq!(legacy.account_id, None, "старый файл: профиль глобальный");
    }
}
