//! Чужие лаунчеры (спека §6.11-смежная): найти установленные Prism/PolyMC,
//! ATLauncher, GDLauncher и официальный vanilla-каталог, показать их инстансы
//! и импортировать выбранный. Скан — только перечисление локальных каталогов:
//! без сети, без процессов, ошибки доступа пропускаются.
//!
//! ВАЖНО: копирование — ПРЯМАЯ копия байтами (не `copy_tree_mixed`!): тот
//! хардлинкует «неуникальные» каталоги, а чужой контент после импорта обязан
//! жить независимо от чужого лаунчера (удалили Prism — наш инстанс не сломался).

use crate::errors::{LauncherError, Result};
use crate::instances::{ContentEntry, ContentKind, ContentSource, Instance};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Инстанс чужого лаунчера — строка для UI (выбор → импорт).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignInstance {
    pub name: String,
    /// Полный путь к каталогу инстанса (входит в `import_foreign_instance`).
    pub dir: String,
    pub mc_version: Option<String>,
    /// Наши имена загрузчиков: fabric|forge|neoforge|quilt (None — vanilla).
    pub loader: Option<String>,
    pub loader_version: Option<String>,
    /// Приблизительный размер инстанса (для «≈1.2 ГБ» в UI).
    pub size_bytes: u64,
}

/// Найденный чужой лаунчер: вид, путь установки, его инстансы.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignSource {
    /// "prism" | "gdlauncher" | "atlauncher" | "vanilla".
    pub kind: String,
    /// Путь установки лаунчера (не каталог инстансов).
    pub root: String,
    pub instances: Vec<ForeignInstance>,
}

/// Псевдоним — длинные пути Windows должны проходить через long_path (спека §1).
fn lp(p: &Path) -> PathBuf {
    crate::util::fs::long_path(p)
}

// ---------- скан ----------

/// Стандартные места установки лаунчеров в Windows.
fn standard_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(roaming) = dirs::data_dir() {
        for name in ["PrismLauncher", "PolyMC", "gdlauncher", "gdlauncher_next"] {
            roots.push(roaming.join(name));
        }
        roots.push(roaming.join(".minecraft"));
    }
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("ATLauncher"));
    }
    roots
}

/// Найти лаунчеры в стандартных местах Windows. Каталог без прав/несуществующий
/// не валят скан — источник просто пропускается.
pub fn scan_foreign_launchers() -> Vec<ForeignSource> {
    scan_roots(standard_roots())
}

/// Скан по перечню корней (для тестов и будущих «своих путей»). Вид лаунчера
/// определяется по ИМЕНИ корневого каталога; неизвестные и отсутствующие
/// корни пропускаются; лаунчеры без инстансов в результат не попадают.
pub fn scan_roots(roots: Vec<PathBuf>) -> Vec<ForeignSource> {
    let mut out = Vec::new();
    for root in roots {
        let Some(kind) = kind_for_root(&root) else {
            continue;
        };
        if !root.is_dir() {
            continue;
        }
        let source = match kind {
            "vanilla" => vanilla_source(&root),
            other => ForeignSource {
                kind: other.to_string(),
                root: root.to_string_lossy().into_owned(),
                instances: subdirs(&root.join("instances"))
                    .into_iter()
                    .map(|dir| match other {
                        "prism" => prism_instance(&dir),
                        "atlauncher" => atl_instance(&dir),
                        _ => gd_instance(&dir),
                    })
                    .collect(),
            },
        };
        if !source.instances.is_empty() {
            out.push(source);
        }
    }
    out
}

/// Вид лаунчера по имени каталога установки.
fn kind_for_root(root: &Path) -> Option<&'static str> {
    let name = root.file_name()?.to_string_lossy().to_lowercase();
    match name.as_str() {
        "prismlauncher" | "polymc" => Some("prism"),
        "atlauncher" => Some("atlauncher"),
        "gdlauncher" | "gdlauncher_next" => Some("gdlauncher"),
        ".minecraft" => Some("vanilla"),
        _ => None,
    }
}

/// Подкаталоги (только каталоги, отсортированные — детерминированный список).
fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(lp(dir)) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out
}

/// Имя каталога-инстанса — универсальный fallback для имени.
fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Инстанс".into())
}

/// Приблизительный размер каталога (обход с капом: патологическое дерево
/// не должно виснуть скан; после капа размер считается неполным).
const MAX_SCAN_FILES: usize = 200_000;

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut seen = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(lp(&d)) else {
            continue;
        };
        for e in entries.flatten() {
            seen += 1;
            if seen > MAX_SCAN_FILES {
                return total;
            }
            let Ok(ft) = e.file_type() else { continue };
            // Символ-ссылки/junction не обходим: цикл в чужом каталоге не должен
            // зациклить скан.
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(e.path());
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    total
}

/// PrisMLauncher/PolyMC: instance.cfg (ini, `name=`) + mmc-pack.json (uid/версии).
fn prism_instance(dir: &Path) -> ForeignInstance {
    let cfg = std::fs::read_to_string(lp(&dir.join("instance.cfg"))).ok();
    let name = cfg
        .as_deref()
        .and_then(|t| ini_get(t, "name"))
        .filter(|s| !s.is_empty());
    let (mc, loader) = read_json(&dir.join("mmc-pack.json"))
        .map(|v| mmc_versions(&v))
        .unwrap_or((None, None));
    ForeignInstance {
        name: name.unwrap_or_else(|| dir_name(dir)),
        dir: dir.to_string_lossy().into_owned(),
        mc_version: mc,
        loader: loader.as_ref().map(|(k, _)| k.clone()),
        loader_version: loader.map(|(_, v)| v),
        size_bytes: dir_size(dir),
    }
}

/// ATLauncher: instance.json (name/version/pack) + game-файлы в корне
/// инстанса (там же `versions/`) либо в `.minecraft/` — определим по факту.
fn atl_instance(dir: &Path) -> ForeignInstance {
    let meta = read_json(&dir.join("instance.json"));
    let name = meta
        .as_ref()
        .and_then(|m| jstr(m, &["name"]))
        .unwrap_or_else(|| dir_name(dir));
    let game = pick_game_dir(dir);
    let (mut mc, loader) = versions_scan(&game);
    // Fallback: `version` в instance.json — у vanilla-паков это версия MC,
    // у модпаков — версия пака (может выглядеть как «1.x»!) — берём только
    // если по versions/ версию определить не удалось (честная эвристика).
    if mc.is_none() {
        mc = meta
            .as_ref()
            .and_then(|m| jstr(m, &["version"]))
            .filter(|v| v.starts_with("1."));
    }
    ForeignInstance {
        name,
        dir: dir.to_string_lossy().into_owned(),
        mc_version: mc,
        loader: loader.as_ref().map(|(k, _)| k.clone()),
        loader_version: loader.map(|(_, v)| v),
        size_bytes: dir_size(dir),
    }
}

/// GDLauncher (carbon: config.json; старые сборки: instance.json) + versions/.
fn gd_instance(dir: &Path) -> ForeignInstance {
    let cfg = read_json(&dir.join("config.json"));
    let meta = read_json(&dir.join("instance.json"));
    let name = cfg
        .as_ref()
        .and_then(|c| jstr(c, &["name"]))
        .or_else(|| meta.as_ref().and_then(|m| jstr(m, &["name"])))
        .unwrap_or_else(|| dir_name(dir));
    let mut mc = None;
    let mut loader = None;
    if let Some(c) = &cfg {
        let ltype = c
            .pointer("/loader/type")
            .and_then(Value::as_str)
            .map(|s| s.to_ascii_lowercase());
        let lver = c
            .pointer("/loader/version")
            .and_then(Value::as_str)
            .map(str::to_string);
        match ltype.as_deref() {
            // «Vanilla» в GDLauncher: version = версия самой MC.
            Some("vanilla") => mc = lver,
            Some(kind) if matches!(kind, "fabric" | "forge" | "neoforge" | "quilt") => {
                loader = lver.map(|v| (kind.to_string(), v));
                mc = jstr(c, &["minecraftVersion", "mcVersion", "gameVersion"]);
            }
            _ => mc = jstr(c, &["minecraftVersion", "mcVersion", "gameVersion"]),
        }
    }
    if mc.is_none() {
        mc = meta
            .as_ref()
            .and_then(|m| jstr(m, &["minecraftVersion", "mcVersion", "version"]));
    }
    // Всё, что не выяснили из конфигов, — добираем из каталога versions/.
    if mc.is_none() || loader.is_none() {
        let (mc2, loader2) = versions_scan(&pick_game_dir(dir));
        mc = mc.or(mc2);
        loader = loader.or(loader2);
    }
    ForeignInstance {
        name,
        dir: dir.to_string_lossy().into_owned(),
        mc_version: mc,
        loader: loader.as_ref().map(|(k, _)| k.clone()),
        loader_version: loader.map(|(_, v)| v),
        size_bytes: dir_size(dir),
    }
}

/// Официальный лаунчер: `launcher_profiles.json` с кастомными версиями
/// (свой каталог в `versions/` + загрузчик) — по инстансу на профиль; без
/// таких профилей — один общий инстанс «Vanilla .minecraft».
fn vanilla_source(root: &Path) -> ForeignSource {
    let profiles = read_json(&root.join("launcher_profiles.json"));
    let mut pairs: Vec<(String, String)> = Vec::new(); // (имя профиля, lastVersionId)
    if let Some(p) = &profiles {
        if let Some(map) = p.get("profiles").and_then(Value::as_object) {
            for prof in map.values() {
                let Some(vid) = prof.get("lastVersionId").and_then(Value::as_str) else {
                    continue;
                };
                if vid.is_empty() {
                    continue;
                }
                let name = prof
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| vid.to_string());
                pairs.push((name, vid.to_string()));
            }
        }
    }
    pairs.sort();
    let root_size = dir_size(root);
    let mut instances = Vec::new();
    for (name, vid) in &pairs {
        // Кастомный профиль = свой каталог версии + распознанный загрузчик
        // (чистые релизы — общее достояние, отдельными инстансами их не плодим).
        let Some((kind, ver)) = parse_version_id(vid).1 else {
            continue;
        };
        if !root.join("versions").join(vid).is_dir() {
            continue;
        }
        instances.push(ForeignInstance {
            name: name.clone(),
            dir: root.to_string_lossy().into_owned(),
            mc_version: parse_version_id(vid).0,
            loader: Some(kind),
            loader_version: Some(ver),
            size_bytes: root_size,
        });
    }
    if instances.is_empty() && (profiles.is_some() || root.join("versions").is_dir()) {
        // Простая полезная версия: один общий инстанс vanilla.
        let mut mc = None;
        let mut loader = None;
        for (_, vid) in &pairs {
            if root.join("versions").join(vid).is_dir() {
                let (m, l) = parse_version_id(vid);
                mc = m.or(mc);
                loader = l.or(loader);
                break;
            }
        }
        if mc.is_none() {
            (mc, loader) = versions_scan(root);
        }
        instances.push(ForeignInstance {
            name: "Vanilla .minecraft".into(),
            dir: root.to_string_lossy().into_owned(),
            mc_version: mc,
            loader: loader.as_ref().map(|(k, _)| k.clone()),
            loader_version: loader.map(|(_, v)| v),
            size_bytes: root_size,
        });
    }
    ForeignSource {
        kind: "vanilla".into(),
        root: root.to_string_lossy().into_owned(),
        instances,
    }
}

// ---------- парсеры (общие для скана и импорта) ----------

/// Значение `key=value` из java-properties (instance.cfg): первый совпавший.
fn ini_get(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

/// Прочитать JSON-файл целиком; любой сбой — None (скан не должен падать).
fn read_json(path: &Path) -> Option<Value> {
    let data = std::fs::read(lp(path)).ok()?;
    serde_json::from_slice(&data).ok()
}

/// Первая непустая строка по любому из ключей объекта.
fn jstr(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(Value::as_str) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// Загрузчик из uid компонента mmc-pack.json — те же имена, что у
/// import/mod.rs (`loader_from_uid` там приватный, здесь свой дубль).
fn loader_from_uid(uid: &str, version: Option<&str>) -> Option<(String, String)> {
    let kind = match uid {
        "net.fabricmc.fabric-loader" => "fabric",
        "org.quiltmc.quilt-loader" => "quilt",
        "net.neoforged.neoforge" => "neoforge",
        "net.minecraftforge" => "forge",
        _ => return None,
    };
    Some((kind.to_string(), version?.to_string()))
}

/// (mc, loader) из components[] mmc-pack.json. `cachedVersion` — fallback,
/// когда `version` не записан (pinned-компоненты Prism).
fn mmc_versions(pack: &Value) -> (Option<String>, Option<(String, String)>) {
    let mut mc = None;
    let mut loader = None;
    for c in pack
        .get("components")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let uid = c.get("uid").and_then(Value::as_str).unwrap_or("");
        let ver = c
            .get("version")
            .and_then(Value::as_str)
            .or_else(|| c.get("cachedVersion").and_then(Value::as_str));
        if uid == "net.minecraft" {
            mc = ver.map(str::to_string);
        } else {
            loader = loader.or_else(|| loader_from_uid(uid, ver));
        }
    }
    (mc, loader)
}

/// Каталог игры внутри инстанса: `.minecraft/` или `minecraft/`, если есть
/// (свежий Prism), иначе — сам каталог инстанса (ATLauncher, vanilla).
fn pick_game_dir(dir: &Path) -> PathBuf {
    for sub in [".minecraft", "minecraft"] {
        let p = dir.join(sub);
        if p.is_dir() {
            return p;
        }
    }
    dir.to_path_buf()
}

/// (mc, loader) по именам каталогов в `versions/` игрового каталога:
/// предпочитаем каталог с загрузчиком (forge/fabric/…), иначе — чистую версию.
fn versions_scan(base: &Path) -> (Option<String>, Option<(String, String)>) {
    let mut mc = None;
    let mut loader = None;
    for dir in subdirs(&base.join("versions")) {
        let Some(name) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let (m, l) = parse_version_id(&name);
        if loader.is_none() {
            loader = l;
            mc = m.or(mc);
        } else if mc.is_none() {
            mc = m;
        }
    }
    (mc, loader)
}

/// Разбор id версии (`1.20.1-forge-47.2.0`, `fabric-loader-0.15.11-1.20.1`,
/// `quilt-loader-0.21.0-beta.2-1.20.1`, `neoforge-20.4.237`) →
/// (версия MC, загрузчик+версия). Регистр не важен — чужие лаунчеры пишут
/// по-разному; версии загрузчиков и MC — это цифры/точки, lowercase безвреден.
fn parse_version_id(id: &str) -> (Option<String>, Option<(String, String)>) {
    let low = id.to_ascii_lowercase();
    for (prefix, kind) in [("fabric-loader-", "fabric"), ("quilt-loader-", "quilt")] {
        if let Some(rest) = low.strip_prefix(prefix) {
            // Версия загрузчика — до ПОСЛЕДНЕГО дефиса (беты quilt содержат
            // дефисы), версия MC — хвост.
            return match rest.rsplit_once('-') {
                Some((lv, mc)) if !mc.is_empty() && !lv.is_empty() => {
                    (Some(mc.to_string()), Some((kind.to_string(), lv.to_string())))
                }
                _ => (None, None),
            };
        }
    }
    // forge/neoforge без версии MC в имени id (neoforge-21.1.77): MC неизвестна.
    for (prefix, kind) in [("forge-", "forge"), ("neoforge-", "neoforge")] {
        if let Some(ver) = low.strip_prefix(prefix) {
            if !ver.is_empty() {
                return (None, Some((kind.to_string(), ver.to_string())));
            }
        }
    }
    for (marker, kind) in [("-forge-", "forge"), ("-neoforge-", "neoforge")] {
        if let Some((mc, ver)) = low.split_once(marker) {
            if !mc.is_empty() && !ver.is_empty() {
                return (Some(mc.to_string()), Some((kind.to_string(), ver.to_string())));
            }
        }
    }
    (Some(id.to_string()), None)
}

// ---------- импорт ----------

/// Результат разбора каталога инстанса чужого лаунчера перед импортом.
struct ParsedForeign {
    name: Option<String>,
    mc_version: Option<String>,
    loader: Option<(String, String)>,
    game_dir: PathBuf,
}

/// Разобрать каталог чужого инстанса (определить вид по маркерам).
fn parse_foreign_dir(dir: &Path) -> Result<ParsedForeign> {
    if !dir.is_dir() {
        return Err(LauncherError::NotFound(format!(
            "каталог инстанса {} не найден",
            dir.display()
        )));
    }
    let fallback = dir_name(dir);
    // 1. Prism/PolyMC: instance.cfg + mmc-pack.json.
    if dir.join("instance.cfg").exists() || dir.join("mmc-pack.json").exists() {
        let cfg = std::fs::read_to_string(lp(&dir.join("instance.cfg"))).ok();
        let name = cfg
            .as_deref()
            .and_then(|t| ini_get(t, "name"))
            .filter(|s| !s.is_empty());
        let (mc, loader) = read_json(&dir.join("mmc-pack.json"))
            .map(|v| mmc_versions(&v))
            .unwrap_or((None, None));
        return Ok(ParsedForeign {
            name: Some(name.unwrap_or(fallback)),
            mc_version: mc,
            loader,
            game_dir: pick_game_dir(dir),
        });
    }
    // 2. GDLauncher (config.json) — разбор переиспользует скан-логику.
    if dir.join("config.json").exists() {
        let inst = gd_instance(dir);
        return Ok(ParsedForeign {
            name: Some(inst.name),
            mc_version: inst.mc_version,
            loader: inst.loader.zip(inst.loader_version),
            game_dir: pick_game_dir(dir),
        });
    }
    if dir.join("instance.json").exists() {
        let inst = atl_instance(dir);
        return Ok(ParsedForeign {
            name: Some(inst.name),
            mc_version: inst.mc_version,
            loader: inst.loader.zip(inst.loader_version),
            game_dir: pick_game_dir(dir),
        });
    }
    // 4. Общий случай: vanilla-каталог или инстанс без метаданных — только
    // скан versions/.
    let game_dir = pick_game_dir(dir);
    let (mc, loader) = versions_scan(&game_dir);
    Ok(ParsedForeign {
        name: Some(fallback),
        mc_version: mc,
        loader,
        game_dir,
    })
}

/// Откат несостоявшегося импорта — локальный аналог import/mod.rs: на момент
/// сбоя инстанс уже сохранён, сбой оставил бы «призрак». Каталог удаляем
/// целиком; ошибку удаления только логируем — наружу идёт исходная причина.
fn rollback_instance(paths: &crate::paths::Paths, id: &str) {
    let dir = crate::instances::instance_dir(paths, id);
    if let Err(e) = std::fs::remove_dir_all(lp(&dir)) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("откат инстанса {id}: {} не удалён: {e}", dir.display());
        }
    }
}

/// Все шаги ПОСЛЕ `instances::save` под тем же откатом, что и import_archive (A29).
fn rollback_on_err<T>(paths: &crate::paths::Paths, inst: &Instance, r: Result<T>) -> Result<T> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => {
            tracing::warn!("импорт чужого инстанса не удался ({e}) — откатываю");
            rollback_instance(paths, &inst.id);
            Err(e)
        }
    }
}

/// Каталоги верхнего уровня игрового каталога, которые НЕ копируем: мусор,
/// кэши и общесистемный контент — клиент/библиотеки/ассеты наш лаунчер ставит
/// в инстанс сам (стор + install_loader), тащить чужие гигабайты незачем.
const SKIP_TOP_DIRS: &[&str] = &[
    ".trash",
    "logs",
    "crash-reports",
    "crashreports",
    ".qmlcache", // кэш Qt-интерфейса Prism
    ".cache",
    "cache",
    "versions",
    "libraries",
    "assets",
    "bin",
    "natives",
];

/// Метаданные чужого лаунчера на верхнем уровне: к нашему инстансу не относятся
/// (наш instance.json живёт на уровень выше, в корне каталога инстанса).
const SKIP_TOP_FILES: &[&str] = &[
    "instance.cfg",
    "mmc-pack.json",
    "instance.json",
    "launcher_profiles.json",
    "launcher_log.txt",
];

/// Глубина рекурсии копирования: сейвы/конфиги глубже не строят; дальше —
/// цикл или мусор, обрываем.
const MAX_COPY_DEPTH: u32 = 32;

/// Импортировать инстанс чужого лаунчера: скопировать игровое содержимое в наш
/// каталог инстансов, создать наш Instance (+ content-manifest из Local-записей,
/// загрузчик ставится как в import_archive — той же транзакцией с откатом A29).
/// Тяжёлое копирование уезжает в blocking-пул (D14).
///
/// Сигнатура шире контракта задачи на `paths`/`settings`/`client`: создание
/// `Instance` без `Paths` невозможно, а установка загрузчика (как в
/// import_archive) требует settings/client — IPC-обёртка передаёт их из AppState.
pub async fn import_foreign_instance(
    paths: &crate::paths::Paths,
    settings: &crate::settings::Settings,
    client: std::sync::Arc<crate::net::http::HttpClient>,
    dir: &Path,
    target_name: &str,
) -> Result<String> {
    let parsed = parse_foreign_dir(dir)?;
    // Санитизация через общий хелпер имён: fallback — родное имя инстанса.
    let fallback = parsed
        .name
        .clone()
        .unwrap_or_else(|| "Импорт".to_string());
    let name = crate::util::names::sanitize_user_name(target_name, &fallback);
    let mut inst = Instance::new(&name, parsed.mc_version.as_deref().unwrap_or(""));
    if let Some((kind, ver)) = &parsed.loader {
        inst.loader = Some(kind.clone());
        inst.loader_version = Some(ver.clone());
    }
    crate::instances::save(paths, &inst)?;

    // Копирование игрового дерева — в blocking-пул: тысячи файлов фризили бы
    // рантайм IPC (D14).
    let copy_paths = paths.clone();
    let inst_id = inst.id.clone();
    let src_game = parsed.game_dir.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let dst = crate::instances::minecraft_dir(&crate::instances::instance_dir(
            &copy_paths,
            &inst_id,
        ));
        copy_game_tree(&src_game, &dst)
    })
    .await
    .map_err(|e| LauncherError::Io(std::io::Error::other(format!("фоновая задача импорта: {e}"))));
    let entries = rollback_on_err(paths, &inst, joined.and_then(|r| r))?;

    // Контент-манифест: скопированные моды/рп/шейдеры — Local-записи без
    // Modrinth-источника (аналог ручных модов, project_id=None).
    if !entries.is_empty() {
        let saved = crate::instances::save_content_manifest(paths, &inst.id, &entries);
        rollback_on_err(paths, &inst, saved)?;
    }

    // Загрузчик ставится под тем же откатом (D62/D64-паритет с import_archive):
    // офлайн + известный загрузчик — честный отказ, а не «частичный успех».
    if let Some((kind, ver)) = &parsed.loader {
        if client.offline() {
            let err = LauncherError::OfflineMode(format!(
                "для импорта «{name}» нужен установчик {kind} — включи сеть и повтори импорт"
            ));
            return rollback_on_err(paths, &inst, Err(err));
        }
        let installed = crate::instances::run::install_loader(
            paths,
            settings,
            client.clone(),
            &inst,
            kind,
            Some(ver.as_str()),
        )
        .await;
        inst = rollback_on_err(paths, &inst, installed)?;
    }
    Ok(inst.id)
}

/// Скопировать игровое дерево чужого инстанса в наш `minecraft/`, попутно
/// собрав content-записи (mods/resourcepacks/shaderpacks).
fn copy_game_tree(src: &Path, dst: &Path) -> Result<Vec<ContentEntry>> {
    std::fs::create_dir_all(lp(dst))?;
    let mut entries = Vec::new();
    copy_recursive(src, dst, "", 0, &mut entries)?;
    Ok(entries)
}

fn copy_recursive(
    src: &Path,
    dst: &Path,
    rel: &str,
    depth: u32,
    entries: &mut Vec<ContentEntry>,
) -> Result<()> {
    if depth > MAX_COPY_DEPTH {
        tracing::warn!("импорт: глубже {MAX_COPY_DEPTH} уровней не копирую: {}", src.display());
        return Ok(());
    }
    let Ok(rd) = std::fs::read_dir(lp(src)) else {
        return Ok(()); // нет прав на подкаталог — не роняем весь импорт
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(ft) = e.file_type() else { continue };
        // Символ-ссылки/junction: циклы и файлы чужих томов не тащим.
        if ft.is_symlink() {
            continue;
        }
        // Корзина обновлений — мусор на любой глубине.
        if name == ".trash" {
            continue;
        }
        if rel.is_empty() {
            let skipped = if ft.is_dir() {
                SKIP_TOP_DIRS.contains(&name.as_str())
            } else {
                SKIP_TOP_FILES.contains(&name.as_str())
            };
            if skipped {
                continue;
            }
        }
        let child_rel = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let child_dst = dst.join(&name);
        if ft.is_dir() {
            std::fs::create_dir_all(lp(&child_dst))?;
            copy_recursive(&e.path(), &child_dst, &child_rel, depth + 1, entries)?;
        } else {
            if let Some(parent) = lp(&child_dst).parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(lp(&e.path()), lp(&child_dst))?;
            if let Some(entry) = content_entry(&child_rel) {
                entries.push(entry);
            }
        }
    }
    Ok(())
}

/// Content-запись для скопированного файла: только mods/resourcepacks/
/// shaderpacks. Prism-суффикс `.disabled` (наш content.rs понимает тот же
/// паттерн) превращается в enabled=false с логическим именем без суффикса.
fn content_entry(rel: &str) -> Option<ContentEntry> {
    let (top, rest) = rel.split_once('/')?;
    let kind = match top {
        "mods" => ContentKind::Mod,
        "resourcepacks" => ContentKind::ResourcePack,
        "shaderpacks" => ContentKind::Shader,
        _ => return None,
    };
    let (file, enabled) = match rest.strip_suffix(".disabled") {
        Some(base) => (base, false),
        None => (rest, true),
    };
    // Мод — только .jar; прочие файлы в mods/ копируются, но в манифест не идут.
    if kind == ContentKind::Mod && !file.ends_with(".jar") {
        return None;
    }
    Some(ContentEntry {
        kind,
        file: format!("{top}/{file}"),
        source: ContentSource::Local,
        project_id: None,
        version_id: None,
        sha1: None,
        url: None,
        enabled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Записать файл, создав родительские каталоги.
    fn w(path: &Path, data: &[u8]) {
        std::fs::create_dir_all(lp(path.parent().unwrap())).unwrap();
        std::fs::write(lp(path), data).unwrap();
    }

    fn dir_of(p: &Path) -> PathBuf {
        p.to_path_buf()
    }

    // ---------- парсеры id версий ----------

    /// Все известные формы имён версий — fabric/quilt (включая беты с дефисами),
    /// forge/neoforge с версией MC в id и без неё, чистый релиз.
    #[test]
    fn version_ids_parse_to_known_loaders() {
        for (id, mc, kind, ver) in [
            ("fabric-loader-0.15.11-1.20.1", "1.20.1", "fabric", "0.15.11"),
            (
                "quilt-loader-0.21.0-beta.2-1.20.1",
                "1.20.1",
                "quilt",
                "0.21.0-beta.2",
            ),
            ("1.20.1-forge-47.2.0", "1.20.1", "forge", "47.2.0"),
            (
                "1.20.1-neoforge-47.1.106",
                "1.20.1",
                "neoforge",
                "47.1.106",
            ),
        ] {
            let (m, l) = parse_version_id(id);
            assert_eq!(m.as_deref(), Some(mc), "{id}");
            assert_eq!(l, Some((kind.to_string(), ver.to_string())), "{id}");
        }
        // neoforge-21.1.77: версия MC в id не присутствует — честный None.
        let (m, l) = parse_version_id("neoforge-21.1.77");
        assert_eq!(m, None);
        assert_eq!(l, Some(("neoforge".into(), "21.1.77".into())));
        // Чистый релиз — без загрузчика.
        let (m, l) = parse_version_id("1.21.4");
        assert_eq!(m.as_deref(), Some("1.21.4"));
        assert_eq!(l, None);
    }

    // ---------- скан ----------

    /// Синтетический Prism: instance.cfg + mmc-pack.json + .minecraft/mods.
    #[test]
    fn prism_scan_parses_name_versions_and_loaders() {
        let tmp = tempfile::tempdir().unwrap();
        let root = dir_of(tmp.path()).join("PrismLauncher");
        let inst = root.join("instances").join("fabulous");
        w(&inst.join("instance.cfg"), b"name=Fabulous Pack\nInstanceType=OneSix\n");
        w(
            &inst.join("mmc-pack.json"),
            br#"{"components":[{"uid":"net.minecraft","version":"1.20.1"},
                {"uid":"net.fabricmc.fabric-loader","version":"0.15.11"}]}"#,
        );
        w(&inst.join(".minecraft").join("mods").join("x.jar"), b"JAR");

        let found = scan_roots(vec![root]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "prism");
        assert_eq!(found[0].instances.len(), 1);
        let i = &found[0].instances[0];
        assert_eq!(i.name, "Fabulous Pack", "имя из instance.cfg");
        assert_eq!(i.mc_version.as_deref(), Some("1.20.1"));
        assert_eq!(i.loader.as_deref(), Some("fabric"));
        assert_eq!(i.loader_version.as_deref(), Some("0.15.11"));
        assert!(i.size_bytes >= 3, "размер считается по файлам");
        assert!(i.dir.ends_with("fabulous"));
    }

    /// PolyMC — тот же вид «prism»; без mmc-pack — инстанс без версий, но в списке.
    #[test]
    fn polymc_root_is_prism_kind_and_bare_instance_survives() {
        let tmp = tempfile::tempdir().unwrap();
        let root = dir_of(tmp.path()).join("PolyMC");
        let inst = root.join("instances").join("bare");
        w(&inst.join(".minecraft").join("mods").join("y.jar"), b"JAR");
        let found = scan_roots(vec![root]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "prism");
        assert_eq!(found[0].instances[0].name, "bare");
        assert_eq!(found[0].instances[0].mc_version, None);
        assert_eq!(found[0].instances[0].loader, None);
    }

    /// ATLauncher: game-файлы в корне инстанса (versions/ на месте), версия MC и
    /// forge — из имени каталога версии.
    #[test]
    fn atlauncher_scan_reads_root_layout_versions() {
        let tmp = tempfile::tempdir().unwrap();
        let root = dir_of(tmp.path()).join("ATLauncher");
        let inst = root.join("instances").join("atm9");
        w(
            &inst.join("instance.json"),
            br#"{"name":"All the Mods 9","pack":"All the Mods 9","version":"1.0.7"}"#,
        );
        w(
            &inst
                .join("versions")
                .join("1.20.1-forge-47.2.0")
                .join("1.20.1-forge-47.2.0.json"),
            br#"{"id":"1.20.1-forge-47.2.0"}"#,
        );
        w(&inst.join("mods").join("a.jar"), b"JAR");

        let found = scan_roots(vec![root]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "atlauncher");
        let i = &found[0].instances[0];
        assert_eq!(i.name, "All the Mods 9");
        assert_eq!(i.mc_version.as_deref(), Some("1.20.1"));
        assert_eq!(i.loader.as_deref(), Some("forge"), "forge из versions/");
        assert_eq!(i.loader_version.as_deref(), Some("47.2.0"));
    }

    /// GDLauncher: config.json carbon-формата; vanilla-тип → version = версия MC.
    #[test]
    fn gdlauncher_scan_reads_config_json() {
        let tmp = tempfile::tempdir().unwrap();
        let root = dir_of(tmp.path()).join("gdlauncher_next");
        let inst = root.join("instances").join("gd-fab");
        w(
            &inst.join("config.json"),
            br#"{"loader":{"type":"Fabric","version":"0.15.11"},"minecraftVersion":"1.20.1"}"#,
        );
        let inst2 = root.join("instances").join("gd-vanilla");
        w(
            &inst2.join("config.json"),
            br#"{"loader":{"type":"Vanilla","version":"1.19.2"}}"#,
        );

        let found = scan_roots(vec![root]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "gdlauncher");
        assert_eq!(found[0].instances.len(), 2);
        let fab = found[0]
            .instances
            .iter()
            .find(|i| i.name == "gd-fab")
            .unwrap();
        assert_eq!(fab.mc_version.as_deref(), Some("1.20.1"));
        assert_eq!(fab.loader.as_deref(), Some("fabric"));
        assert_eq!(fab.loader_version.as_deref(), Some("0.15.11"));
        let vanilla = found[0]
            .instances
            .iter()
            .find(|i| i.name == "gd-vanilla")
            .unwrap();
        assert_eq!(vanilla.mc_version.as_deref(), Some("1.19.2"));
        assert_eq!(vanilla.loader, None, "Vanilla-тип — без загрузчика");
    }

    /// Vanilla: профили с кастомными версиями → по инстансу на профиль; без
    /// них (или без profiles) — один общий «Vanilla .minecraft».
    #[test]
    fn vanilla_scan_custom_profiles_and_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let root = dir_of(tmp.path()).join(".minecraft");
        w(
            &root.join("launcher_profiles.json"),
            br#"{"profiles":{
                "p1":{"name":"Forge 1.20.1","lastVersionId":"1.20.1-forge-47.2.0"},
                "p2":{"name":"Fabric","lastVersionId":"fabric-loader-0.15.11-1.20.1"},
                "stock":{"type":"latest-release","lastVersionId":"1.21.4"}}}"#,
        );
        for vid in ["1.20.1-forge-47.2.0", "fabric-loader-0.15.11-1.20.1", "1.21.4"] {
            w(
                &root.join("versions").join(vid).join(format!("{vid}.json")),
                br#"{}"#,
            );
        }
        let found = scan_roots(vec![root.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "vanilla");
        assert_eq!(found[0].instances.len(), 2, "только кастомные профили");
        let forge = found[0]
            .instances
            .iter()
            .find(|i| i.name == "Forge 1.20.1")
            .unwrap();
        assert_eq!(forge.mc_version.as_deref(), Some("1.20.1"));
        assert_eq!(forge.loader.as_deref(), Some("forge"));

        // Fallback: без profiles — один общий инстанс.
        let tmp2 = tempfile::tempdir().unwrap();
        let root2 = dir_of(tmp2.path()).join(".minecraft");
        w(
            &root2
                .join("versions")
                .join("1.20.1")
                .join("1.20.1.json"),
            br#"{}"#,
        );
        let found2 = scan_roots(vec![root2]);
        assert_eq!(found2[0].instances.len(), 1);
        assert_eq!(found2[0].instances[0].name, "Vanilla .minecraft");
        assert_eq!(found2[0].instances[0].mc_version.as_deref(), Some("1.20.1"));
        assert_eq!(found2[0].instances[0].loader, None);
    }

    /// Неизвестные и несуществующие корни пропускаются, скан не падает.
    #[test]
    fn scan_skips_unknown_and_missing_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let found = scan_roots(vec![
            dir_of(tmp.path()).join("Nope"),
            dir_of(tmp.path()).join("UnknownLauncher"),
        ]);
        assert!(found.is_empty());
    }

    // ---------- импорт ----------

    fn test_paths(dir: &tempfile::TempDir) -> crate::paths::Paths {
        let paths = crate::paths::Paths::new(dir.path().join("launcher"));
        paths.ensure_dirs().unwrap();
        paths
    }

    /// Синтетический Prism-инстанс (без загрузчика — офлайн-тест) → импорт:
    /// наш каталог создан, mods/config/saves скопированы, mmc-метаданные,
    /// logs и .trash — нет; манифест контента — Local-записи (.disabled →
    /// enabled=false).
    #[tokio::test]
    async fn import_prism_copies_game_and_skips_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let inst = dir.path().join("PrismLauncher").join("instances").join("fab");
        w(&inst.join("instance.cfg"), b"name=Fab\n");
        w(
            &inst.join("mmc-pack.json"),
            br#"{"components":[{"uid":"net.minecraft","version":"1.20.1"}]}"#,
        );
        w(&inst.join(".minecraft").join("mods").join("a.jar"), b"AAA");
        w(
            &inst
                .join(".minecraft")
                .join("mods")
                .join("b.jar.disabled"),
            b"BBB",
        );
        w(&inst.join(".minecraft").join("config").join("x.cfg"), b"X");
        w(
            &inst.join(".minecraft").join("saves").join("w").join("level.dat"),
            b"W",
        );
        w(
            &inst.join(".minecraft").join("logs").join("latest.log"),
            b"LOG",
        );
        w(&inst.join(".minecraft").join(".trash").join("junk.jar"), b"J");

        let client = std::sync::Arc::new(crate::net::http::HttpClient::new(None).unwrap());
        let id = import_foreign_instance(
            &paths,
            &crate::settings::Settings::default(),
            client,
            &inst,
            "Мой импорт",
        )
        .await
        .unwrap();

        let loaded = crate::instances::load(&paths, &id).unwrap();
        assert_eq!(loaded.name, "Мой импорт");
        assert_eq!(loaded.mc_version, "1.20.1");
        assert_eq!(loaded.loader, None, "без загрузчика сеть не нужна");

        let ours = crate::instances::instance_dir(&paths, &id);
        let mc = crate::instances::minecraft_dir(&ours);
        assert_eq!(std::fs::read(mc.join("mods/a.jar")).unwrap(), b"AAA");
        assert_eq!(
            std::fs::read(mc.join("mods/b.jar.disabled")).unwrap(),
            b"BBB"
        );
        assert_eq!(std::fs::read(mc.join("config/x.cfg")).unwrap(), b"X");
        assert_eq!(std::fs::read(mc.join("saves/w/level.dat")).unwrap(), b"W");
        // Мусор и метаданные чужого лаунчера не приехали.
        assert!(!mc.join("logs").exists(), "logs не копируется");
        assert!(!mc.join(".trash").exists(), ".trash не копируется");
        assert!(!ours.join("instance.cfg").exists(), "mmc-метаданные не копируются");
        assert!(!ours.join("mmc-pack.json").exists());
        assert!(!mc.join("mmc-pack.json").exists());
        assert!(!mc.join("instance.cfg").exists());

        // Манифест контента: a.jar включён, b.jar выключен, источник Local.
        let manifest = crate::instances::load_content_manifest(&paths, &id);
        assert_eq!(manifest.len(), 2, "{manifest:?}");
        let a = manifest.iter().find(|e| e.file == "mods/a.jar").unwrap();
        assert!(a.enabled && matches!(a.kind, ContentKind::Mod));
        assert_eq!(a.source, ContentSource::Local);
        assert_eq!(a.project_id, None);
        let b = manifest.iter().find(|e| e.file == "mods/b.jar").unwrap();
        assert!(!b.enabled, ".disabled → выключенный");
    }

    /// Несуществующий каталог — честный NotFound, инстанс-«призрак» не остаётся.
    #[tokio::test]
    async fn import_missing_dir_fails_without_ghost() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let client = std::sync::Arc::new(crate::net::http::HttpClient::new(None).unwrap());
        let err = import_foreign_instance(
            &paths,
            &crate::settings::Settings::default(),
            client,
            &dir.path().join("no-such-instance"),
            "X",
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), "not_found", "{err}");
        assert!(crate::instances::list(&paths).is_empty());
        assert_eq!(
            std::fs::read_dir(lp(&paths.instances_dir())).unwrap().count(),
            0,
            "каталог-призрак не создан"
        );
    }

    /// Vanilla-каталог (.minecraft с forge-версией, без метаданных): импорт
    /// вычисляет mc+loader из versions/, мусор лаунчера не тащится.
    #[tokio::test]
    async fn import_vanilla_generic_dir_parses_versions() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let root = dir.path().join(".minecraft");
        w(
            &root
                .join("versions")
                .join("1.20.1-forge-47.2.0")
                .join("1.20.1-forge-47.2.0.json"),
            br#"{"id":"1.20.1-forge-47.2.0"}"#,
        );
        w(&root.join("saves").join("w").join("level.dat"), b"W");
        w(&root.join("launcher_profiles.json"), br#"{"profiles":{}}"#);

        // Импорт с загрузчиком forge при выключенной сети — честный OfflineMode
        // с полным откатом (паритет D64 import_archive).
        let client = std::sync::Arc::new(crate::net::http::HttpClient::new(None).unwrap());
        client.set_offline(true);
        let err = import_foreign_instance(
            &paths,
            &crate::settings::Settings::default(),
            client,
            &root,
            "Из ванилы",
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), "offline_mode", "{err}");
        assert!(
            crate::instances::list(&paths).is_empty(),
            "инстанс-«призрак» откатан"
        );
        assert_eq!(
            std::fs::read_dir(lp(&paths.instances_dir())).unwrap().count(),
            0
        );
    }
}
