//! Выделенные серверы (модуль servers): реестр `servers.json` в корне данных
//! (паттерн favorites.rs: атомарная запись tmp+rename, карантин `.bad`) и
//! каталоги `%ДАННЫЕ%\servers\<id>\`. Сервер ПРИНАДЛЕЖИТ инстансу по ссылке
//! (instance_id), но живёт отдельно: удаление инстанса сервер не трогает.
//!
//! Дочерние модули: install (серверное ПО), run (запуск/стоп),
//! mods_filter (фильтр клиент-модов), worlds (мир и шаблоны конфигов).
//!
//! `running`/`pid`/`eula_accepted` в реестре НЕ хранятся — вычисляются на
//! лету при выдаче ServerInfo (run.rs + worlds::read_eula_accepted).

pub mod install;
pub mod mods_filter;
pub mod run;
pub mod worlds;

use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use crate::util::fs::{atomic_write, long_path};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Имя файла реестра в корне данных.
const FILE_NAME: &str = "servers.json";

/// Подкаталог каталогов серверов.
const SERVERS_SUBDIR: &str = "servers";

/// Границы RAM сервера (МБ). pub(crate): супервизор клампит -Xmx теми же
/// значениями (P3-ревизии: раньше клампил инстансными 256..65536).
pub(crate) const MIN_RAM_MB: u32 = 512;
pub(crate) const MAX_RAM_MB: u32 = 32768;

/// Сериализация read-modify-write реестра (создание/удаление из UI).
static SERVERS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Сервер как его видит UI/IPC. Поля running/pid/eula_accepted заполняются
/// при выдаче (свежие на момент вызова), в реестре их нет.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub id: String,
    pub name: String,
    pub instance_id: String,
    pub mc_version: String,
    pub loader: Option<String>,
    pub loader_version: Option<String>,
    pub dir: String,
    pub port: u16,
    pub ram_mb: u32,
    pub online_mode: bool,
    pub eula_accepted: bool,
    pub world: Option<String>,
    pub created_at: u64,
    pub running: bool,
    pub pid: Option<u32>,
    pub java_major: u32,
}

/// Результат создания сервера: карточка + сводка фильтра модов.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCreateResult {
    pub server: ServerInfo,
    /// Скопировано серверных/двусторонних модов (проверенных по API).
    pub mods_server: u32,
    /// Пропущено клиент-модов.
    pub mods_client_skipped: u32,
    /// Скопировано непроверенных (офлайн, нет project_id).
    pub mods_unchecked: u32,
    /// Не скопировано из-за дубля имени (dst-коллизия вложенных путей).
    pub collisions: u32,
    /// Имена файлов пропущенных клиент-модов.
    pub skipped_projects: Vec<String>,
}

/// Персистентная запись реестра. serde(default): файл без новых полей
/// читается — реестр не должен целиком уезжать в карантин из-за эволюции
/// схемы (потерянное поле хуже потерянного списка серверов).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ServerRecord {
    id: String,
    name: String,
    instance_id: String,
    mc_version: String,
    loader: Option<String>,
    loader_version: Option<String>,
    port: u16,
    ram_mb: u32,
    online_mode: bool,
    /// Имя мира-источника, скопированного при создании (для UI).
    world: Option<String>,
    created_at: u64,
    /// Требуемый мажор Java из version JSON на момент создания
    /// (0 — не вычислили, супервизор тогда на эвристике; снапшоты 26.x
    /// эвристика занижает — приёмка ловила UnsupportedClassVersionError).
    #[serde(default)]
    java_major: u32,
}

impl Default for ServerRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            instance_id: String::new(),
            mc_version: String::new(),
            loader: None,
            loader_version: None,
            port: 25565,
            ram_mb: 2048,
            online_mode: true,
            world: None,
            created_at: 0,
                java_major: 0,
        }
    }
}

/// Реестр: id → запись (BTreeMap — стабильный порядок в файле и выдаче).
type Registry = BTreeMap<String, ServerRecord>;

/// Корень каталогов серверов: `%ДАННЫЕ%\servers\`.
pub fn servers_root(paths: &Paths) -> PathBuf {
    paths.root().join(SERVERS_SUBDIR)
}

/// Каталог конкретного сервера. id валидируется вызывающими (valid_server_id).
fn server_dir(paths: &Paths, id: &str) -> PathBuf {
    servers_root(paths).join(id)
}

fn registry_path(paths: &Paths) -> PathBuf {
    paths.root().join(FILE_NAME)
}

/// Строгая проверка id сервера: ASCII-буквы/цифры, `-`, `_`. Барьер против
/// path traversal в IPC-командах, принимающих id (`..`, разделители — мимо).
fn valid_server_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(LauncherError::InvalidInput(format!(
            "некорректный id сервера: {id}"
        )));
    }
    Ok(())
}

/// Битый servers.json — в карантин: переименовать в `servers.json.bad`
/// (занятый слот не затираем — тогда `.bad.1`, `.bad.2`, ...). Иначе первая же
/// запись молча затёрла бы реестр всех серверов, а вторая порча — молча
/// затёрла бы прежний карантин: слоты оставляют данные для ручного
/// восстановления (та же схема слотов, что у settings.json).
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
                tracing::warn!("servers.json невалиден ({reason}) — перемещён в {}", candidate.display());
                return;
            }
            Err(e) => {
                tracing::warn!("servers.json невалиден ({reason}), карантин не удался: {e}");
                return;
            }
        }
    }
    tracing::warn!("servers.json невалиден ({reason}); слоты карантина заняты — файл оставлен как есть");
}

/// Читать реестр; файла нет → пустой, битый → карантин + пустой.
fn load_registry(paths: &Paths) -> Registry {
    let path = registry_path(paths);
    let long = long_path(&path);
    if !long.exists() {
        return BTreeMap::new();
    }
    match std::fs::read(&long) {
        Ok(data) => match serde_json::from_slice(&data) {
            Ok(reg) => reg,
            Err(e) => {
                quarantine_broken(&path, &e.to_string());
                BTreeMap::new()
            }
        },
        Err(e) => {
            tracing::warn!("servers.json не прочитан: {e}");
            BTreeMap::new()
        }
    }
}

/// Атомарно записать реестр (tmp + rename, как settings.json).
fn save_registry(paths: &Paths, reg: &Registry) -> Result<()> {
    let data = serde_json::to_vec_pretty(reg)?;
    atomic_write(&registry_path(paths), &data)
}

/// Каталоги-сироты: подкаталог в servers_root без записи в реестре (после
/// карантина servers.json, сбоя диска или ручной чистки файла). Подкаталог с
/// .server.lock или хотя бы одним файлом — warn в журнал: молча терять
/// установленный сервер нельзя, но записи не восстанавливаем и каталог не
/// трогаем (список сироты, как и раньше, не возвращает).
fn warn_orphan_server_dirs(paths: &Paths, reg: &Registry) {
    let Ok(entries) = std::fs::read_dir(long_path(&servers_root(paths))) else {
        return; // корня серверов ещё нет — сирот тоже нет
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if reg.contains_key(&id) {
            continue;
        }
        let has_content = path.join(".server.lock").exists()
            || std::fs::read_dir(&path)
                .map(|rd| rd.filter_map(|e| e.ok()).any(|e| e.path().is_file()))
                .unwrap_or(false);
        if has_content {
            tracing::warn!("каталог сервера без записи реестра: {}", path.display());
        }
    }
}

/// Имя каталога сервера из имени: ASCII-буквы/цифры в нижний регистр,
/// остальное — в `-`, повторы схлопнуты, до 48 символов, края без `-`.
/// Пустой результат — fallback "server" (кириллическое имя не ломает id).
fn slugify(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let mut out = String::new();
    for c in mapped.chars() {
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    let trimmed = out.trim_matches('-');
    let cut: String = trimmed.chars().take(48).collect();
    let cut = cut.trim_matches('-');
    if cut.is_empty() {
        "server".to_string()
    } else {
        cut.to_string()
    }
}

/// Свободный id `<slug>-<8 hex>`: свежий uuid на каждую попытку, коллизии
/// с реестром/каталогом отсекает `taken` (FnMut — проверка может держать
/// внутреннее состояние).
fn unique_server_id(slug: &str, mut taken: impl FnMut(&str) -> bool) -> String {
    loop {
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let candidate = format!("{slug}-{}", &suffix[..8]);
        if !taken(&candidate) {
            return candidate;
        }
    }
}

/// Запись → карточка для UI: running/pid из run.rs, eula — из eula.txt.
fn to_info(paths: &Paths, rec: &ServerRecord) -> ServerInfo {
    let dir = server_dir(paths, &rec.id);
    ServerInfo {
        id: rec.id.clone(),
        name: rec.name.clone(),
        instance_id: rec.instance_id.clone(),
        mc_version: rec.mc_version.clone(),
        loader: rec.loader.clone(),
        loader_version: rec.loader_version.clone(),
        dir: dir.to_string_lossy().into_owned(),
        port: rec.port,
        ram_mb: rec.ram_mb,
        online_mode: rec.online_mode,
        eula_accepted: worlds::read_eula_accepted(&dir),
        world: rec.world.clone(),
        created_at: rec.created_at,
        running: run::is_running(paths, &rec.id),
        pid: run::running_pid(paths, &rec.id),
        java_major: rec.java_major,
    }
}

/// Параметры создания, прошедшие валидацию (разговорка create_inner).
struct CreateParams<'a> {
    id: &'a str,
    dir: &'a Path,
    name: &'a str,
    world: Option<&'a str>,
    port: u16,
    ram_mb: u32,
    online_mode: bool,
}

/// Создать выделенный сервер из инстанса: установить серверное ПО (наследует
/// версию и загрузчик инстанса), скопировать моды с фильтром клиент-модов,
/// опционально мир, записать server.properties/eula.txt и реестр. Любая
/// ошибка после создания каталога откатывает каталог целиком.
#[allow(clippy::too_many_arguments)]
pub async fn server_create(
    paths: &Paths,
    http: &HttpClient,
    instance_id: &str,
    name: &str,
    world: Option<&str>,
    port: u16,
    ram_mb: u32,
    online_mode: bool,
) -> Result<ServerCreateResult> {
    // 1. Инстанс и входные данные. Ошибки валидации — ДО создания каталога.
    let inst = crate::instances::load(paths, instance_id)?;
    if crate::instances::running_pid(&crate::instances::instance_dir(paths, instance_id)).is_some()
    {
        return Err(LauncherError::InstanceRunning(instance_id.to_string()));
    }
    let mc_version = inst.mc_version.trim().to_string();
    if mc_version.is_empty() {
        return Err(LauncherError::InvalidInput(
            "сначала выберите версию игры инстанса — сервер наследует её".into(),
        ));
    }
    if port == 0 {
        return Err(LauncherError::InvalidInput(
            "порт сервера обязателен (1..=65535)".into(),
        ));
    }
    if !(MIN_RAM_MB..=MAX_RAM_MB).contains(&ram_mb) {
        return Err(LauncherError::InvalidInput(format!(
            "RAM сервера должна быть от {MIN_RAM_MB} до {MAX_RAM_MB} МБ, получено {ram_mb}"
        )));
    }
    let name = crate::util::names::sanitize_user_name(name, "Сервер");
    let world = world.map(str::trim).filter(|w| !w.is_empty());
    if let Some(w) = world {
        // Ранний отказ до скачивания ПО: мира нет — ничего и не начинаем.
        worlds::ensure_world_copyable(&crate::instances::instance_dir(paths, instance_id), w)?;
    }

    // 2. Уникальный id и каталог (занятость — и в реестре, и на диске).
    let id = {
        let reg = load_registry(paths);
        unique_server_id(&slugify(&name), |c| {
            reg.contains_key(c) || servers_root(paths).join(c).exists()
        })
    };
    let dir = server_dir(paths, &id);
    std::fs::create_dir_all(long_path(&dir))?;

    let params = CreateParams {
        id: &id,
        dir: &dir,
        name: &name,
        world,
        port,
        ram_mb,
        online_mode,
    };
    match create_inner(paths, http, &inst, &mc_version, params).await {
        Ok(res) => Ok(res),
        Err(e) => {
            // Откат: недостроенный каталог сервера не оставляем. Первый
            // remove_dir_all может не смочь (антивирус держит свежий
            // server.jar) — один повтор через короткую паузу. Редкий путь,
            // короткий блокирующий сон в async-функции здесь допустим.
            match std::fs::remove_dir_all(long_path(&dir)) {
                Ok(()) => {
                    tracing::warn!("создание сервера {id} не удалось, каталог откачен: {e}")
                }
                Err(first) => {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    match std::fs::remove_dir_all(long_path(&dir)) {
                        Ok(()) => tracing::warn!(
                            "создание сервера {id} не удалось, каталог откачен со второй попытки: {e}"
                        ),
                        Err(retry) => tracing::warn!(
                            "создание сервера {id} не удалось, откат не удался: {first}; повтор: {retry}; причина отказа: {e}"
                        ),
                    }
                }
            }
            Err(e)
        }
    }
}

/// Шаги 3-7 создания. Каждый шаг при ошибке уводит наверх — откат делает
/// server_create (remove_dir_all созданного каталога).
async fn create_inner(
    paths: &Paths,
    http: &HttpClient,
    inst: &crate::instances::Instance,
    mc_version: &str,
    p: CreateParams<'_>,
) -> Result<ServerCreateResult> {
    // Требуемый мажор Java фиксируем из version JSON (источник истины, а не
    // эвристика fallback_major — снапшоты 26.x ей незнакомы).
    let java_major = install::required_java_major(paths, http, mc_version).await;
    // 3. Серверное ПО (vanilla/fabric/forge/neoforge) — модуль install;
    // тяжёлое блокирует сам инсталлятор, прогресс играет в журнал.
    let progress = |msg: String| tracing::info!(target: "servers::create", "{msg}");
    install::install_server_software(
        paths,
        http,
        p.dir,
        mc_version,
        inst.loader.as_deref(),
        inst.loader_version.as_deref(),
        &progress,
    )
    .await?;

    // 4. Моды инстанса с фильтрацией клиент-модов.
    let mods = mods_filter::copy_server_mods(paths, http, &inst.id, p.dir).await?;

    // 5. Мир (побайтово, без session.lock) — тяжёлое копирование вне executor.
    if let Some(w) = p.world {
        let world_name = w.to_string();
        let inst_dir = crate::instances::instance_dir(paths, &inst.id);
        let dir = p.dir.to_path_buf();
        tokio::task::spawn_blocking(move || worlds::copy_world(&inst_dir, &world_name, &dir))
            .await
            .map_err(|e| LauncherError::internal(format!("копирование мира: {e}")))??;
    }

    // 6. server.properties (level-name = world) и заглушка eula.txt.
    worlds::write_properties(p.dir, p.port, p.online_mode, worlds::WORLD_DIR_NAME, p.name)?;
    worlds::write_eula_stub(p.dir)?;

    // 7. Реестр: id уникализирован заранее, коллизии здесь не бывает.
    let rec = ServerRecord {
        id: p.id.to_string(),
        name: p.name.to_string(),
        instance_id: inst.id.clone(),
        mc_version: mc_version.to_string(),
        loader: inst.loader.clone(),
        loader_version: inst.loader_version.clone(),
        port: p.port,
        ram_mb: p.ram_mb,
        online_mode: p.online_mode,
        world: p.world.map(str::to_string),
        created_at: crate::instances::now_secs(),
        java_major,
    };
    {
        let _guard = SERVERS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut reg = load_registry(paths);
        // P3-ревизии: уникальность id проверялась до лока — перепроверяем под
        // локом, чтобы параллельный create не затёр чужую запись.
        if reg.contains_key(&rec.id) {
            return Err(LauncherError::InvalidInput(format!(
                "сервер с id {} уже существует — попробуйте другое имя",
                rec.id
            )));
        }
        reg.insert(rec.id.clone(), rec.clone());
        save_registry(paths, &reg)?;
    }

    let server = to_info(paths, &rec);
    Ok(ServerCreateResult {
        server,
        mods_server: mods.copied,
        mods_client_skipped: mods.skipped_client,
        mods_unchecked: mods.unchecked,
        collisions: mods.collisions,
        skipped_projects: mods.skipped_projects,
    })
}

/// Список серверов (всех или конкретного инстанса), порядок — по id.
pub fn server_list(paths: &Paths, instance_id: Option<&str>) -> Vec<ServerInfo> {
    let reg = load_registry(paths);
    warn_orphan_server_dirs(paths, &reg);
    reg.into_values()
        .filter(|r| match instance_id {
            Some(need) => r.instance_id == need,
            None => true,
        })
        .map(|r| to_info(paths, &r))
        .collect()
}

/// Статус одного сервера (свежие running/pid/eula).
pub fn server_status(paths: &Paths, id: &str) -> Result<ServerInfo> {
    valid_server_id(id)?;
    let rec = load_registry(paths)
        .get(id)
        .cloned()
        .ok_or_else(|| LauncherError::NotFound(format!("сервер {id}")))?;
    Ok(to_info(paths, &rec))
}

/// Удалить сервер: только остановленный. Каталог — best-effort (застрявший
/// файл не должен оставить запись-сироту), запись из реестра убирает всегда.
pub fn server_delete(paths: &Paths, id: &str) -> Result<()> {
    valid_server_id(id)?;
    {
        let reg = load_registry(paths);
        if !reg.contains_key(id) {
            return Err(LauncherError::NotFound(format!("сервер {id}")));
        }
    }
    // Гвард старта (тот же, что сериализует server_start) берём ДО финальной
    // проверки is_running и держим до конца удаления каталога: без него старт
    // успел бы просочиться между проверкой и remove_dir_all (гонка delete vs
    // start — каталог исчез бы под ногами у свежеспавненной java).
    let _start = run::start_guard();
    if run::is_running(paths, id) {
        return Err(LauncherError::InvalidInput(
            "нельзя удалить работающий сервер: сначала остановите его".into(),
        ));
    }
    let dir = server_dir(paths, id);
    if let Err(e) = std::fs::remove_dir_all(long_path(&dir)) {
        tracing::warn!("каталог сервера {id} удалён не полностью: {e}");
    }
    let _guard = SERVERS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut reg = load_registry(paths);
    reg.remove(id);
    save_registry(paths, &reg)
}

/// Подтвердить EULA: переписать eula.txt с `eula=true` (комментарии файла
/// сохраняются, при отсутствии — ванильный шаблон). Ok — можно стартовать.
pub fn server_accept_eula(paths: &Paths, id: &str) -> Result<()> {
    valid_server_id(id)?;
    let reg = load_registry(paths);
    if !reg.contains_key(id) {
        return Err(LauncherError::NotFound(format!("сервер {id}")));
    }
    worlds::write_eula(&server_dir(paths, id), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paths в tempdir без сети.
    fn setup() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        (dir, paths)
    }

    fn record(id: &str, instance_id: &str) -> ServerRecord {
        ServerRecord {
            id: id.into(),
            name: "Сервер".into(),
            instance_id: instance_id.into(),
            mc_version: "1.20.1".into(),
            loader: None,
            loader_version: None,
            port: 25565,
            ram_mb: 2048,
            online_mode: false,
            world: Some("Новый мир".into()),
            created_at: 1700000000,
            java_major: 21,
        }
    }

    #[test]
    fn registry_roundtrip_preserves_fields() {
        let (_d, paths) = setup();
        let mut reg = Registry::new();
        reg.insert("srv-aaa".into(), record("srv-aaa", "inst-1"));
        reg.insert("srv-bbb".into(), record("srv-bbb", "inst-2"));
        save_registry(&paths, &reg).unwrap();

        let loaded = load_registry(&paths);
        assert_eq!(loaded.len(), 2);
        let r = &loaded["srv-aaa"];
        assert_eq!(r.instance_id, "inst-1");
        assert_eq!(r.mc_version, "1.20.1");
        assert_eq!(r.port, 25565);
        assert_eq!(r.ram_mb, 2048);
        assert!(!r.online_mode);
        assert_eq!(r.world.as_deref(), Some("Новый мир"));
        // BTreeMap: порядок ключей в файле стабильный.
        let first = serde_json::to_value(&loaded).unwrap();
        let keys: Vec<&String> = first.as_object().unwrap().keys().collect();
        assert_eq!(keys, [&"srv-aaa", &"srv-bbb"]);
    }

    #[test]
    fn broken_registry_quarantined_and_starts_fresh() {
        let (_d, paths) = setup();
        let path = registry_path(&paths);
        std::fs::write(&path, b"{ not a map").unwrap();
        assert!(load_registry(&paths).is_empty(), "битый реестр читается пустым");
        assert!(
            path.with_file_name("servers.json.bad").exists(),
            "битый файл в карантине"
        );
        // Запись после карантина начинает реестр заново.
        let mut reg = Registry::new();
        reg.insert("srv-x".into(), record("srv-x", "i"));
        save_registry(&paths, &reg).unwrap();
        assert_eq!(load_registry(&paths).len(), 1);
    }

    #[test]
    fn missing_registry_file_is_empty() {
        let (_d, paths) = setup();
        assert!(load_registry(&paths).is_empty());
    }

    /// Вторая порча подряд не затирает первый карантин — новый файл уезжает
    /// в свободный слот `.bad.1` (схема settings.json).
    #[test]
    fn quarantine_does_not_overwrite_previous_bad() {
        let (_d, paths) = setup();
        let path = registry_path(&paths);
        std::fs::write(&path, b"{ first broken").unwrap();
        assert!(load_registry(&paths).is_empty());
        assert!(path.with_file_name("servers.json.bad").exists());

        std::fs::write(&path, b"{ second broken").unwrap();
        assert!(load_registry(&paths).is_empty());
        let first = std::fs::read_to_string(path.with_file_name("servers.json.bad")).unwrap();
        assert_eq!(first, "{ first broken", "прежний карантин не затёрт");
        assert!(path.with_file_name("servers.json.bad.1").exists(), "вторая порча в соседнем слоте");
    }

    /// Каталог-сирота (есть на диске, записи в реестре нет): в список не
    /// попадает, запись молча не восстанавливается, каталог не удаляется —
    /// server_list только предупреждает в журнал.
    #[test]
    fn orphan_dir_not_in_list_and_left_untouched() {
        let (_d, paths) = setup();
        let root = servers_root(&paths);
        let orphan = root.join("srv-orphan");
        std::fs::create_dir_all(long_path(&orphan)).unwrap();
        std::fs::write(orphan.join(".server.lock"), b"pid: 123\n").unwrap();
        // Пустой подкаталог без файлов — не сирота, но в список тоже не попадает.
        std::fs::create_dir_all(long_path(&root.join("srv-empty"))).unwrap();

        assert!(server_list(&paths, None).is_empty(), "сирота в список не попадает");
        assert!(orphan.exists(), "каталог-сирота не удаляется");
        assert!(!registry_path(&paths).exists(), "запись не восстанавливается молча");
        // Повторный list — та же картина (не копит побочных эффектов).
        assert!(server_list(&paths, None).is_empty());
        assert!(orphan.exists());
    }

    #[test]
    fn slugify_rules() {
        assert_eq!(slugify("My Server!"), "my-server");
        assert_eq!(slugify("  a__b  "), "a-b");
        assert_eq!(slugify("Мой сервер"), "server", "кириллица → fallback");
        assert_eq!(slugify("---"), "server");
        let long = slugify(&"x".repeat(100));
        assert_eq!(long.len(), 48);
        assert!(!long.starts_with('-') && !long.ends_with('-'));
    }

    #[test]
    fn unique_server_id_avoids_taken_and_keeps_slug() {
        // Первый кандидат «занят» — функция обязана дать другой суффикс.
        let first = std::cell::Cell::new(true);
        let id = unique_server_id("my-server", |c| {
            assert!(c.starts_with("my-server-"), "кандидат {c:?} от slug");
            if first.get() {
                first.set(false);
                return true;
            }
            false
        });
        assert!(id.starts_with("my-server-"), "{id}");
        // Формат: slug + ровно 8 hex.
        let suffix = id.trim_start_matches("my-server-");
        assert_eq!(suffix.len(), 8);
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn two_servers_same_name_get_different_ids() {
        let mut reg = Registry::new();
        let id1 = unique_server_id(&slugify("Сервер"), |c| reg.contains_key(c));
        reg.insert(id1.clone(), record(&id1, "i"));
        let id2 = unique_server_id(&slugify("Сервер"), |c| reg.contains_key(c));
        assert_ne!(id1, id2, "второй сервер с тем же именем — другой id");
        assert!(reg.insert(id2.clone(), record(&id2, "i")).is_none());
    }

    /// Валидация server_create: все отказы — ДО создания каталога сервера
    /// и ДО обращения к сети (instaлятор не вызывается).
    #[tokio::test]
    async fn create_validates_before_touching_disk() {
        let (_d, paths) = setup();
        let http = HttpClient::new(None).unwrap();

        // Инстанса нет.
        let err = server_create(&paths, &http, "ghost", "s", None, 25565, 2048, true)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "not_found");

        // Инстанс с пустой версией.
        let inst = crate::instances::Instance::new("Тест", "");
        crate::instances::save(&paths, &inst).unwrap();
        let err = server_create(&paths, &http, &inst.id, "s", None, 25565, 2048, true)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "invalid_input", "пустая mc_version");

        // Инстанс запущен (легаси .lock с живым PID теста).
        let inst = crate::instances::Instance::new("Тест", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        std::fs::write(
            crate::instances::instance_dir(&paths, &inst.id).join(".lock"),
            std::process::id().to_string(),
        )
        .unwrap();
        let err = server_create(&paths, &http, &inst.id, "s", None, 25565, 2048, true)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "instance_running");
        let _ = std::fs::remove_file(crate::instances::instance_dir(&paths, &inst.id).join(".lock"));

        // Порт 0 и RAM вне диапазона.
        for (port, ram) in [(0, 2048), (25565, 100), (25565, 40000)] {
            let err = server_create(&paths, &http, &inst.id, "s", None, port, ram, true)
                .await
                .unwrap_err();
            assert_eq!(err.code(), "invalid_input", "port={port}, ram={ram}");
        }
        // Мир не существует — ранний отказ.
        let err = server_create(&paths, &http, &inst.id, "s", Some("ghost"), 25565, 2048, true)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "not_found");

        // Ни каталог серверов, ни реестр не созданы отказами.
        assert!(!servers_root(&paths).exists());
        assert!(!registry_path(&paths).exists());
    }

    /// Удаление остановленного сервера: каталог и запись уходят вместе;
    /// чужой id — ошибки; реестр остаётся консистентным.
    #[tokio::test]
    async fn delete_stopped_server_removes_dir_and_record() {
        let (_d, paths) = setup();
        let id = "srv-del";
        let dir = server_dir(&paths, id);
        std::fs::create_dir_all(long_path(&dir.join("mods"))).unwrap();
        std::fs::write(dir.join("server.properties"), b"server-port=25565").unwrap();
        {
            let _guard = SERVERS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut reg = Registry::new();
            reg.insert(id.into(), record(id, "inst"));
            reg.insert("srv-keep".into(), record("srv-keep", "inst"));
            save_registry(&paths, &reg).unwrap();
        }

        // Чужой/битый id — не трогаем ничего.
        assert_eq!(server_delete(&paths, "../escape").unwrap_err().code(), "invalid_input");
        assert_eq!(server_delete(&paths, "srv-ghost").unwrap_err().code(), "not_found");
        assert!(dir.exists(), "отказы не удаляют каталог");

        server_delete(&paths, id).unwrap();
        assert!(!dir.exists(), "каталог удалён");
        let reg = load_registry(&paths);
        assert!(!reg.contains_key(id));
        assert!(reg.contains_key("srv-keep"), "соседние записи не тронуты");

        // Повторное удаление — NotFound.
        assert_eq!(server_delete(&paths, id).unwrap_err().code(), "not_found");
    }

    /// Подтверждение EULA: для известной записи — true в eula.txt; для
    /// несуществующей — NotFound; битый id — InvalidInput.
    #[tokio::test]
    async fn accept_eula_writes_true() {
        let (_d, paths) = setup();
        let id = "srv-eula";
        let dir = server_dir(&paths, id);
        std::fs::create_dir_all(&dir).unwrap();
        {
            let _guard = SERVERS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut reg = Registry::new();
            reg.insert(id.into(), record(id, "inst"));
            save_registry(&paths, &reg).unwrap();
        }

        assert_eq!(server_accept_eula(&paths, "ghost").unwrap_err().code(), "not_found");
        assert_eq!(server_accept_eula(&paths, "..").unwrap_err().code(), "invalid_input");

        worlds::write_eula_stub(&dir).unwrap();
        assert!(!worlds::read_eula_accepted(&dir));
        server_accept_eula(&paths, id).unwrap();
        assert!(worlds::read_eula_accepted(&dir), "eula=true записан");
    }

    /// server_status/server_list: поля карточки, вычисляемые флаги
    /// (eula/running/pid) и фильтр по инстансу.
    #[tokio::test]
    async fn status_and_list_compute_flags_on_the_fly() {
        let (_d, paths) = setup();
        let id = "srv-info";
        let dir = server_dir(&paths, id);
        std::fs::create_dir_all(&dir).unwrap();
        {
            let _guard = SERVERS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut reg = Registry::new();
            reg.insert(id.into(), record(id, "inst-a"));
            let mut other = record("srv-b", "inst-b");
            other.port = 25566;
            reg.insert("srv-b".into(), other);
            save_registry(&paths, &reg).unwrap();
        }

        let info = server_status(&paths, id).unwrap();
        assert_eq!(info.id, id);
        assert_eq!(info.instance_id, "inst-a");
        assert_eq!(info.dir, dir.to_string_lossy());
        assert_eq!(info.port, 25565);
        assert_eq!(info.ram_mb, 2048);
        assert!(!info.online_mode);
        assert_eq!(info.world.as_deref(), Some("Новый мир"));
        // Вычисляются на лету, не из реестра: eula-файла нет, сервер не запущен.
        assert!(!info.eula_accepted);
        assert!(!info.running);
        assert_eq!(info.pid, None);

        worlds::write_eula(&dir, true).unwrap();
        let info = server_status(&paths, id).unwrap();
        assert!(info.eula_accepted, "eula перечитывается с диска");

        // Несуществующий — NotFound; фильтр списка по инстансу.
        assert_eq!(server_status(&paths, "ghost").unwrap_err().code(), "not_found");
        assert_eq!(server_list(&paths, None).len(), 2);
        assert_eq!(server_list(&paths, Some("inst-a")).len(), 1);
        assert!(server_list(&paths, Some("inst-a"))[0].id == id);
        assert!(server_list(&paths, Some("inst-x")).is_empty());
    }

    /// Контракт IPC: ServerInfo сериализуется camelCase.
    #[test]
    fn server_info_serializes_camel_case() {
        let info = ServerInfo {
            id: "s".into(),
            name: "S".into(),
            instance_id: "i".into(),
            mc_version: "1.20.1".into(),
            loader: Some("fabric".into()),
            loader_version: None,
            dir: r"C:\x".into(),
            port: 25565,
            ram_mb: 2048,
            online_mode: true,
            eula_accepted: false,
            world: None,
            created_at: 1,
            running: false,
            pid: Some(42),
            java_major: 21,
        };
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(v["instanceId"], "i");
        assert_eq!(v["mcVersion"], "1.20.1");
        assert_eq!(v["loaderVersion"], serde_json::Value::Null);
        assert_eq!(v["ramMb"], 2048);
        assert_eq!(v["onlineMode"], true);
        assert_eq!(v["eulaAccepted"], false);
        assert_eq!(v["createdAt"], 1);
        assert_eq!(v["pid"], 42);
        assert!(v.get("instance_id").is_none(), "snake_case не сериализуется");
    }

    /// Идентификатор сервера проходит ту же проверку, что ждёт IPC.
    #[test]
    fn valid_server_id_rules() {
        assert!(valid_server_id("srv-1_a").is_ok());
        for bad in ["", "../x", "a/b", "a b", "a:b"] {
            assert_eq!(valid_server_id(bad).unwrap_err().code(), "invalid_input");
        }
    }
}
