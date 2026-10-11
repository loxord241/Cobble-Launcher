//! Экспорт инстанса в .mrpack (формат Modrinth v1) — обратная операция к
//! modrinth/mrpack.rs (импорт). files[] — контент с привязкой к Modrinth
//! (project_id + version_id + ссылка), overrides/ — ручные файлы, конфиги
//! (`config/`) и опционально мир. Логи, краши, скриншоты, `.trash` и кэши
//! не включаются.
//!
//! Отличие от простого `content::export_mrpack`: здесь файлы[0] получают
//! вычисленные с диска sha1+sha512 и честный fileSize, ссылка добирается из
//! API Modrinth, если её нет в манифесте, а всё, что не попало в files[],
//! возвращается в отчёте `skipped` с причиной (стабильный ключ для i18n).

use crate::errors::{LauncherError, Result};
use crate::instances::{minecraft_dir, ContentEntry, Instance};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use serde::Serialize;
use sha1::{Digest, Sha1};
use sha2::Sha512;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};

/// Каталоги `minecraft/`, целиком уходящие в overrides/ (контент без
/// привязки к Modrinth и конфиги; список = content::CONTENT_DIRS + config).
const OVERRIDES_DIRS: &[&str] = &["mods", "resourcepacks", "shaderpacks", "datapacks", "config"];

/// Причина пропуска: запись выключена (файл `*.disabled` в пак не берём).
const REASON_DISABLED: &str = "disabled";
/// Причина пропуска: файла нет на диске (удалён вручную мимо лаунчера).
const REASON_MISSING: &str = "missing";
/// Причина пропуска: привязка к Modrinth есть, но ссылку получить не удалось
/// (офлайн/ошибка API) — файл уехал в overrides/.
const REASON_NO_URL: &str = "no_url";
/// Причина пропуска: привязки к Modrinth нет (ручной jar) — файл в overrides/.
const REASON_UNLINKED: &str = "unlinked";
/// Причина пропуска: путь записи из манифеста небезопасен (`..`, абсолютный
/// путь, диск, пустой) — за пределы minecraft/ и staging не выпускаем
/// (защита в глубину, как A29 у импорта).
const REASON_TRAVERSAL: &str = "traversal";
/// Причина пропуска: в saves/ несколько миров, а quick_play_world не указан —
/// за пользователя не решаем, мир в пак не включается.
const REASON_WORLD_AMBIGUOUS: &str = "world_ambiguous";

/// Файл, пропущенный при экспорте: путь в паке + причина (стабильный ключ,
/// переводит фронт: `export.skip.<reason>`).
#[derive(Debug, Clone, Serialize)]
pub struct ExportedFileReport {
    pub path: String,
    pub reason: String,
}

/// Итог экспорта.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackExportResult {
    /// Итоговый файл .mrpack.
    pub path: String,
    /// Файлов в files[] (по данным Modrinth — качаются при установке пака).
    pub files_sourced: u32,
    /// Файлов скопировано в overrides/.
    pub files_overrides: u32,
    /// Что не попало в files[] и почему.
    pub skipped: Vec<ExportedFileReport>,
}

/// Запись files[] после вычисления хэшей с диска.
#[derive(Debug, Clone)]
struct SourcedFile {
    path: String,
    url: String,
    sha1: String,
    sha512: String,
    size: u64,
    project_id: Option<String>,
    version_id: Option<String>,
}

/// План экспорта: что идёт в files[], что — в overrides, что — пропущен.
#[derive(Debug, Default)]
struct ExportPlan {
    sourced: Vec<SourcedPlan>,
    /// Логические пути записей без привязки (для отчёта и точечной
    /// докопировки, если wholesale-обход их каталог не накрыл).
    unlinked: Vec<String>,
    skipped: Vec<ExportedFileReport>,
}

/// Кандидат в files[] до хэширования.
#[derive(Debug, Clone)]
struct SourcedPlan {
    path: String,
    url: String,
    project_id: Option<String>,
    version_id: Option<String>,
}

/// Собрать .mrpack из инстанса. `output_path` — путь файла (обычно выбран
/// системным диалогом), расширение обязано быть `.mrpack`.
pub async fn export_instance_mrpack(
    instance_id: &str,
    output_path: &Path,
    include_worlds: bool,
) -> Result<MrpackExportResult> {
    let paths = Paths::new(Paths::default_root());
    let settings = crate::settings::Settings::load(&paths.settings_file()).unwrap_or_default();
    let client = HttpClient::new(settings.proxy_url.as_deref())?;
    export_instance_mrpack_in(&paths, &client, instance_id, output_path, include_worlds).await
}

/// То же с внешними paths/client — для вызова из мест, где состояние уже
/// есть (IPC-команда, CLI), и для тестов (сеть в тесты не ходит).
pub async fn export_instance_mrpack_in(
    paths: &Paths,
    client: &HttpClient,
    instance_id: &str,
    output_path: &Path,
    include_worlds: bool,
) -> Result<MrpackExportResult> {
    crate::instances::valid_id(instance_id)?;
    if !output_path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mrpack"))
    {
        return Err(LauncherError::InvalidInput(format!(
            "файл экспорта должен иметь расширение .mrpack: {}",
            output_path.display()
        )));
    }

    // 1. Инстанс + манифест (дешёвые чтения — в блокирующем пуле).
    let paths_owned = paths.clone();
    let id = instance_id.to_string();
    let (inst, manifest) = tauri::async_runtime::spawn_blocking(move || {
        let inst = crate::instances::load(&paths_owned, &id)?;
        let dir = crate::instances::instance_dir(&paths_owned, &id);
        // B9: экспорт читает файлы игры — у работающего инстанса запрещён.
        if crate::instances::running_pid(&dir).is_some() {
            return Err(LauncherError::InstanceRunning(inst.id));
        }
        let manifest = crate::instances::load_content_manifest(&paths_owned, &id);
        Ok((inst, manifest))
    })
    .await
    .map_err(join_err)??;

    // 2. Ссылки для записей с привязкой, но без url в манифесте. Офлайн —
    //    без походов в сеть: такие записи уедут в overrides с отчётом.
    let urls = resolve_missing_urls(client, &manifest).await;

    // 3. Тяжёлая часть (хэши, копии, zip) — в блокирующем пуле.
    let paths_owned = paths.clone();
    let inst = inst.clone();
    let output = output_path.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        assemble(&paths_owned, &inst, &manifest, &urls, &output, include_worlds)
    })
    .await
    .map_err(join_err)?
}

/// Ошибка фоновой задачи (как join_err в import): поток упал или запаниковал.
fn join_err(e: tauri::Error) -> LauncherError {
    LauncherError::Io(std::io::Error::other(format!("фоновая задача экспорта: {e}")))
}

/// SHA1+SHA512+размер файла за один проход (BufReader, чанки по 128 КБ):
/// оба хэшера обновляются на одном и том же чтении, файл не вычитывается
/// дважды (как было с sha1_file + sha512_file подряд). Размер — фактически
/// хэшированные байты, поэтому hash и fileSize всегда согласованы.
fn hash_file_once(path: &Path) -> Result<(String, String, u64)> {
    let file = std::fs::File::open(crate::util::fs::long_path(path))?;
    let mut reader = BufReader::with_capacity(128 * 1024, file);
    let mut sha1 = Sha1::new();
    let mut sha512 = Sha512::new();
    let mut total = 0u64;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            break;
        }
        let n = chunk.len();
        sha1.update(chunk);
        sha512.update(chunk);
        reader.consume(n);
        total += n as u64;
    }
    Ok((
        hex::encode(sha1.finalize()),
        hex::encode(sha512.finalize()),
        total,
    ))
}

/// Защита в глубину для путей записей из манифеста (зеркало
/// util::zip::safe_relative_path): только нормальные компоненты.
/// `..`, абсолютный путь, диск (`C:`), NUL и пустая строка — отбраковка:
/// `Some(причина)` для лога; `None` — путь безопасен.
fn traversal_reason(raw: &str) -> Option<&'static str> {
    if raw.contains('\0') {
        return Some("NUL-байт");
    }
    let normalized = raw.replace('\\', "/");
    let mut out = PathBuf::new();
    for comp in Path::new(&normalized).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            _ => return Some("`..`, абсолютный путь или диск"),
        }
    }
    if out.as_os_str().is_empty() {
        return Some("пустой путь");
    }
    None
}

/// Ключ dependencies для загрузчика инстанса (зеркало resolve_loader в
/// mrpack.rs: у Fabric/Quilt ключ с суффиксом `-loader`). Неизвестное
/// значение проходит как есть — импортёр пака разберётся.
fn dependency_key(loader: &str) -> String {
    match loader {
        "fabric" => "fabric-loader".into(),
        "quilt" => "quilt-loader".into(),
        other => other.to_string(),
    }
}

/// modrinth.index.json (формат v1, env не пишем): {formatVersion, game,
/// versionId, name, files[], dependencies}. Чистая функция — тестируется
/// без сети и диска.
fn build_index(inst: &Instance, sourced: &[SourcedFile]) -> Result<Vec<u8>> {
    let files: Vec<serde_json::Value> = sourced
        .iter()
        .map(|f| {
            let mut hashes = BTreeMap::new();
            hashes.insert("sha1", f.sha1.clone());
            hashes.insert("sha512", f.sha512.clone());
            serde_json::json!({
                "path": f.path,
                "hashes": hashes,
                "downloads": [f.url],
                "fileSize": f.size,
                "projectId": f.project_id,
                "versionId": f.version_id,
            })
        })
        .collect();
    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".into(), serde_json::json!(inst.mc_version));
    if let Some(loader) = &inst.loader {
        let version = inst
            .loader_version
            .clone()
            .unwrap_or_else(|| "*".to_string());
        deps.insert(dependency_key(loader), serde_json::json!(version));
    }
    let index = serde_json::json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": "1.0.0",
        "name": inst.name,
        "files": files,
        "dependencies": deps,
    });
    Ok(serde_json::to_vec_pretty(&index)?)
}

/// Достать ссылки из API Modrinth для записей с project_id+version_id, у
/// которых url в манифесте нет (старые манифесты). Офлайн — пусто: записи
/// попадут в overrides с отчётом no_url. Запросы кэшируются на вызов по
/// паре (project_id, version_id).
async fn resolve_missing_urls(client: &HttpClient, entries: &[ContentEntry]) -> HashMap<String, String> {
    if client.offline() {
        return HashMap::new();
    }
    let mut cache: HashMap<(String, String), Option<String>> = HashMap::new();
    let mut out = HashMap::new();
    for e in entries {
        let (Some(project), Some(version)) = (&e.project_id, &e.version_id) else {
            continue;
        };
        if e.url.as_deref().is_some_and(|u| !u.is_empty()) {
            continue; // ссылка уже в манифесте — сеть не нужна
        }
        let key = (project.clone(), version.clone());
        let url = match cache.get(&key) {
            Some(cached) => cached.clone(),
            None => {
                let fetched = fetch_version_url(client, project, version, &e.file).await;
                cache.insert(key.clone(), fetched.clone());
                fetched
            }
        };
        if let Some(url) = url {
            out.insert(e.file.clone(), url);
        }
    }
    out
}

/// Файл конкретной версии: GET /v2/version/{version_id} (документированный
/// эндпоинт одной версии). Из файлов версии берём совпадающий по имени,
/// иначе primary, иначе первый. None — версия недоступна (офлайн/404/битый
/// ответ) — вызывающий отправляет файл в overrides, экспорт не роняется.
async fn fetch_version_url(
    client: &HttpClient,
    project_id: &str,
    version_id: &str,
    want_file: &str,
) -> Option<String> {
    // id уходит в URL: мусор отсекаем до сети (инъекция пути запроса).
    if let Err(e) = crate::modrinth::api::validate_id_or_slug(project_id)
        .and_then(|_| crate::modrinth::api::validate_id_or_slug(version_id))
    {
        tracing::warn!("экспорт: некорректный id {project_id}/{version_id}: {e}");
        return None;
    }
    let url = format!(
        "{}/version/{}",
        crate::modrinth::api::API_BASE,
        version_id
    );
    let version: crate::modrinth::api::Version = match client.get_json_retry(&url).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("экспорт: версия {version_id} не получена ({e}) — файл в overrides");
            return None;
        }
    };
    let want_name = Path::new(want_file)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = version
        .files
        .iter()
        .find(|f| f.filename == want_name)
        .or_else(|| version.files.iter().find(|f| f.primary))
        .or_else(|| version.files.first());
    let _ = project_id; // project_id только для валидации/логики кэша
    file.map(|f| f.url.clone())
}

/// План экспорта по манифесту: файл с привязкой + ссылка → files[], всё
/// остальное — overrides/отчёт. Существование файла проверяется сразу
/// (дёшево), хэширование — позже, только для files[].
fn plan_export(
    entries: &[ContentEntry],
    urls: &HashMap<String, String>,
    mc_dir: &Path,
) -> ExportPlan {
    let mut plan = ExportPlan::default();
    for e in entries {
        // Небезопасный путь из манифеста не доходит ни до files[] (утечка
        // содержимого мимо minecraft/), ни до overrides (запись мимо
        // staging) — отбраковка в отчёт, экспорт не падает.
        if let Some(why) = traversal_reason(&e.file) {
            tracing::warn!("экспорт: путь {:?} отбракован ({why}) — в пак не попадёт", e.file);
            plan.skipped.push(ExportedFileReport {
                path: e.file.clone(),
                reason: REASON_TRAVERSAL.into(),
            });
            continue;
        }
        if !e.enabled {
            plan.skipped.push(ExportedFileReport {
                path: e.file.clone(),
                reason: REASON_DISABLED.into(),
            });
            continue;
        }
        let disk = crate::instances::content::disk_path(&e.file, true);
        if !crate::util::fs::long_path(&mc_dir.join(&disk)).is_file() {
            plan.skipped.push(ExportedFileReport {
                path: e.file.clone(),
                reason: REASON_MISSING.into(),
            });
            continue;
        }
        let url = e
            .url
            .clone()
            .filter(|u| !u.is_empty())
            .or_else(|| urls.get(&e.file).cloned());
        match (&e.project_id, &e.version_id, url) {
            (Some(_), Some(_), Some(url)) => plan.sourced.push(SourcedPlan {
                path: e.file.clone(),
                url,
                project_id: e.project_id.clone(),
                version_id: e.version_id.clone(),
            }),
            (Some(_), Some(_), None) => plan.skipped.push(ExportedFileReport {
                path: e.file.clone(),
                reason: REASON_NO_URL.into(),
            }),
            _ => {
                plan.skipped.push(ExportedFileReport {
                    path: e.file.clone(),
                    reason: REASON_UNLINKED.into(),
                });
                plan.unlinked.push(e.file.clone());
            }
        }
    }
    plan
}

/// Результат выбора мира для overrides/world.
#[derive(Debug, PartialEq, Eq)]
enum WorldPick {
    /// Мир определён — каталог для копирования.
    Dir(PathBuf),
    /// Мира нет (saves пуст, quick_play_world не указывает на мир).
    None,
    /// Несколько миров без указания — за пользователя не решаем,
    /// в отчёт уходит world_ambiguous.
    Ambiguous,
}

/// Выбрать мир для include_worlds: quick_play_world, иначе `world`, иначе
/// единственный мир в saves/. Несколько миров без указания — Ambiguous:
/// вслепую брать первый нельзя.
fn pick_world(mc_dir: &Path, inst: &Instance) -> WorldPick {
    let saves = mc_dir.join("saves");
    let is_world = |d: &Path| d.is_dir() && d.join("level.dat").is_file();
    if let Some(qw) = &inst.quick_play_world {
        let d = saves.join(qw);
        if is_world(&d) {
            return WorldPick::Dir(d);
        }
    }
    let default_world = saves.join("world");
    if is_world(&default_world) {
        return WorldPick::Dir(default_world);
    }
    let worlds: Vec<PathBuf> = std::fs::read_dir(crate::util::fs::long_path(&saves))
        .map(|it| {
            it.flatten()
                .map(|e| e.path())
                .filter(|p| is_world(p))
                .collect()
        })
        .unwrap_or_default();
    match worlds.as_slice() {
        [one] => WorldPick::Dir(one.clone()),
        [] => WorldPick::None,
        _ => WorldPick::Ambiguous,
    }
}

/// Да/нет-обёртка над pick_world: каталог мира или None (неоднозначность
/// тоже None — различать через pick_world).
fn pick_world_dir(mc_dir: &Path, inst: &Instance) -> Option<PathBuf> {
    match pick_world(mc_dir, inst) {
        WorldPick::Dir(d) => Some(d),
        WorldPick::None | WorldPick::Ambiguous => None,
    }
}

/// Копировать дерево `src` → `dst` (только обычные файлы, отсортированно —
/// детерминированный порядок записей архива). `skip` получает путь
/// относительно `src`, слэши; true — пропустить. Нет src — Ok(0).
fn copy_tree(src: &Path, dst: &Path, skip: &dyn Fn(&str) -> bool) -> Result<usize> {
    let it = match std::fs::read_dir(crate::util::fs::long_path(src)) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    let mut count = 0usize;
    let mut entries: Vec<_> = it.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let rel = path
            .strip_prefix(src)
            .map_err(|_| LauncherError::InvalidInput("copy_tree: префикс".into()))?
            .to_string_lossy()
            .replace('\\', "/");
        let ft = e.file_type()?;
        if ft.is_dir() {
            if skip(&rel) {
                continue;
            }
            count += copy_tree(&path, &dst.join(&rel), skip)?;
        } else if ft.is_file() && !skip(&rel) {
            if let Some(parent) = dst.join(&rel).parent() {
                std::fs::create_dir_all(crate::util::fs::long_path(parent))?;
            }
            std::fs::copy(
                crate::util::fs::long_path(&path),
                crate::util::fs::long_path(&dst.join(&rel)),
            )?;
            count += 1;
        }
    }
    Ok(count)
}

/// Guard временного каталога сборки: удаляет его на любом выходе (успех,
/// ошибка, паника) — лучше-effort, ошибка только в лог.
struct StagingCleanup {
    path: PathBuf,
}

impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(crate::util::fs::long_path(&self.path)) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("staging {} не удалён: {e}", self.path.display());
            }
        }
    }
}

/// Тяжёлая часть экспорта: план → хэши → staging → zip. Выполняется в
/// блокирующем пуле.
fn assemble(
    paths: &Paths,
    inst: &Instance,
    manifest: &[ContentEntry],
    urls: &HashMap<String, String>,
    output_path: &Path,
    include_worlds: bool,
) -> Result<MrpackExportResult> {
    let dir = crate::instances::instance_dir(paths, &inst.id);
    let mc = minecraft_dir(&dir);

    let plan = plan_export(manifest, urls, &mc);

    // files[]: хэши и размер — строго с диска (не из манифеста), один
    // проход на файл (sha1+sha512+размер).
    let mut sourced: Vec<SourcedFile> = Vec::new();
    let mut skipped = plan.skipped;
    for s in &plan.sourced {
        let abs =
            crate::util::fs::long_path(&mc.join(crate::instances::content::disk_path(&s.path, true)));
        let (sha1, sha512, size) = match hash_file_once(&abs) {
            Ok(h) => h,
            Err(e) => {
                skipped.push(ExportedFileReport {
                    path: s.path.clone(),
                    reason: REASON_MISSING.into(),
                });
                tracing::warn!("экспорт: {} не прочитан ({e}) — пропущен", s.path);
                continue;
            }
        };
        sourced.push(SourcedFile {
            path: s.path.clone(),
            url: s.url.clone(),
            sha1,
            sha512,
            size,
            project_id: s.project_id.clone(),
            version_id: s.version_id.clone(),
        });
    }

    // Staging: temp/mc-launcher-v2/export-<uuid>/{modrinth.index.json, overrides/**}.
    let staging = std::env::temp_dir()
        .join("mc-launcher-v2")
        .join(format!("export-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(crate::util::fs::long_path(&staging.join("overrides")))?;
    let _cleanup = StagingCleanup {
        path: staging.clone(),
    };

    let index_bytes = build_index(inst, &sourced)?;
    crate::util::fs::atomic_write(&staging.join("modrinth.index.json"), &index_bytes)?;

    // overrides: контентные каталоги + config; файлы из files[] и
    // `*.disabled` не дублируются.
    let in_index: HashSet<String> = sourced.iter().map(|f| f.path.clone()).collect();
    let mut overrides_count = 0usize;
    for subdir in OVERRIDES_DIRS {
        let sourced_dir: HashSet<String> = in_index
            .iter()
            .filter(|p| {
                p.starts_with(&format!("{subdir}/"))
            })
            .cloned()
            .collect();
        overrides_count += copy_tree(
            &mc.join(subdir),
            &staging.join("overrides").join(subdir),
            &|rel| {
                rel.ends_with(".disabled")
                    || sourced_dir.contains(&format!("{subdir}/{rel}"))
            },
        )?;
    }
    // Ручные записи вне контентных каталогов копируем точечно (wholesale их
    // не накрыл); внутри каталогов они уже на месте.
    for rel in &plan.unlinked {
        // Глубинная проверка: plan_export уже отбраковал мусор, но точечная
        // копия — последний рубеж, мимо staging писать нельзя.
        if let Some(why) = traversal_reason(rel) {
            tracing::warn!("экспорт: точечная копия {rel:?} отменена ({why})");
            continue;
        }
        let in_staging = staging.join("overrides").join(rel);
        if crate::util::fs::long_path(&in_staging).exists() {
            continue;
        }
        let src = mc.join(crate::instances::content::disk_path(rel, true));
        if !crate::util::fs::long_path(&src).is_file() {
            continue; // missing уже в отчёте/плане не был, но диск мог измениться
        }
        if let Some(parent) = in_staging.parent() {
            std::fs::create_dir_all(crate::util::fs::long_path(parent))?;
        }
        std::fs::copy(
            crate::util::fs::long_path(&src),
            crate::util::fs::long_path(&in_staging),
        )?;
        overrides_count += 1;
    }
    // Мир (по флагу): overrides/world, один мир. Несколько миров без
    // quick_play_world — видимый отказ в отчёте, а не молчаливый пропуск.
    if include_worlds {
        if let Some(world_dir) = pick_world_dir(&mc, inst) {
            overrides_count += copy_tree(&world_dir, &staging.join("overrides").join("world"), &|_| {
                false
            })?;
        } else if matches!(pick_world(&mc, inst), WorldPick::Ambiguous) {
            skipped.push(ExportedFileReport {
                path: "world".into(),
                reason: REASON_WORLD_AMBIGUOUS.into(),
            });
            tracing::info!(
                "экспорт: в saves/ несколько миров, quick_play_world не указан — мир в пак не включён"
            );
        } else {
            tracing::debug!("экспорт: мира для включения не нашлось — overrides/world пуст");
        }
    }

    // zip: staging целиком в корень архива (write_zip_dir с пустым префиксом
    // кладёт и файл индекса, и overrides/ на верхний уровень).
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(crate::util::fs::long_path(parent))?;
    }
    let file = std::fs::File::create(crate::util::fs::long_path(output_path))?;
    let mut zip = zip::ZipWriter::new(file);
    let written = crate::util::zip::write_zip_dir(&staging, &mut zip, "", &|_| false)?;
    zip.finish()
        .map_err(|e| LauncherError::Zip(format!("finish: {e}")))?;
    tracing::info!(
        "экспорт {}: {} в files[], {} в overrides (записей архива: {written})",
        output_path.display(),
        sourced.len(),
        overrides_count
    );
    Ok(MrpackExportResult {
        path: output_path.to_string_lossy().into_owned(),
        files_sourced: sourced.len() as u32,
        files_overrides: overrides_count as u32,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{instance_dir, save_content_manifest, ContentKind, ContentSource};
    use crate::util::fs::long_path;

    fn test_paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().join("data"));
        paths.ensure_dirs().unwrap();
        (dir, paths)
    }

    fn entry(file: &str, source: ContentSource, project: Option<&str>, url: Option<&str>) -> ContentEntry {
        ContentEntry {
            kind: ContentKind::Mod,
            file: file.into(),
            source,
            project_id: project.map(|p| p.into()),
            version_id: project.map(|p| p.into()),
            sha1: None,
            url: url.map(|u| u.into()),
            enabled: true,
        }
    }

    fn write_mc_file(mc: &Path, rel: &str, data: &[u8]) {
        let p = mc.join(rel);
        std::fs::create_dir_all(long_path(p.parent().unwrap())).unwrap();
        std::fs::write(long_path(&p), data).unwrap();
    }

    /// dependencies: fabric → ключ `fabric-loader`; без версии загрузчика — `*`.
    #[test]
    fn index_dependencies_and_fields() {
        let mut inst = Instance::new("Мой пак", "1.20.1");
        inst.loader = Some("fabric".into());
        let sourced = vec![SourcedFile {
            path: "mods/a.jar".into(),
            url: "https://cdn.modrinth.com/a.jar".into(),
            sha1: "aa".into(),
            sha512: "bb".into(),
            size: 7,
            project_id: Some("A".into()),
            version_id: Some("v1".into()),
        }];
        let bytes = build_index(&inst, &sourced).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["formatVersion"], 1);
        assert_eq!(v["game"], "minecraft");
        assert_eq!(v["versionId"], "1.0.0");
        assert_eq!(v["name"], "Мой пак");
        assert_eq!(v["dependencies"]["minecraft"], "1.20.1");
        assert_eq!(v["dependencies"]["fabric-loader"], serde_json::json!("*"));
        let f = &v["files"][0];
        assert_eq!(f["path"], "mods/a.jar");
        assert_eq!(f["hashes"]["sha1"], "aa");
        assert_eq!(f["hashes"]["sha512"], "bb");
        assert_eq!(f["fileSize"], 7);
        assert_eq!(f["downloads"][0], "https://cdn.modrinth.com/a.jar");
        assert_eq!(f["projectId"], "A");
        assert_eq!(f["versionId"], "v1");
        assert!(v.get("env").is_none(), "env в v1 не пишем");
    }

    /// Vanilla-инстанс: dependencies только minecraft; forge/neoforge —
    /// ключ без суффикса; загрузчик без версии — `*`.
    #[test]
    fn index_loader_keys_and_wildcard() {
        let mut inst = Instance::new("x", "1.21.1");
        let v: serde_json::Value =
            serde_json::from_slice(&build_index(&inst, &[]).unwrap()).unwrap();
        assert_eq!(v["dependencies"]["minecraft"], "1.21.1");
        assert!(v["dependencies"].get("fabric-loader").is_none());

        inst.loader = Some("forge".into());
        let v: serde_json::Value =
            serde_json::from_slice(&build_index(&inst, &[]).unwrap()).unwrap();
        assert_eq!(v["dependencies"]["forge"], "*");

        inst.loader = Some("neoforge".into());
        inst.loader_version = Some("20.4.237".into());
        let v: serde_json::Value =
            serde_json::from_slice(&build_index(&inst, &[]).unwrap()).unwrap();
        assert_eq!(v["dependencies"]["neoforge"], "20.4.237");
    }

    /// Классификация манифеста: выключен/нет файла/нет ссылки/нет привязки.
    #[test]
    fn plan_classifies_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mc = dir.path().to_path_buf();
        write_mc_file(&mc, "mods/sourced.jar", b"aaa");
        write_mc_file(&mc, "mods/manual.jar", b"bbb");
        write_mc_file(&mc, "mods/no-url.jar", b"ddd");
        // gone.jar на диск НЕ пишем — проверяем класс «файл отсутствует».
        let mut disabled = entry("mods/disabled.jar", ContentSource::Local, None, None);
        disabled.enabled = false;
        let entries = vec![
            entry(
                "mods/sourced.jar",
                ContentSource::Modrinth,
                Some("AABB"),
                Some("https://cdn.modrinth.com/sourced.jar"),
            ),
            entry("mods/manual.jar", ContentSource::Local, None, None),
            // привязка есть, ссылки нет (офлайн/ошибка API)
            entry("mods/no-url.jar", ContentSource::Modrinth, Some("CCDD"), None),
            disabled,
            // файла нет на диске
            entry("mods/gone.jar", ContentSource::Local, None, None),
        ];
        let plan = plan_export(&entries, &HashMap::new(), &mc);
        assert_eq!(plan.sourced.len(), 1);
        assert_eq!(plan.sourced[0].path, "mods/sourced.jar");
        assert_eq!(plan.sourced[0].url, "https://cdn.modrinth.com/sourced.jar");
        assert_eq!(plan.unlinked, vec!["mods/manual.jar".to_string()]);
        let reasons: HashMap<&str, &str> = plan
            .skipped
            .iter()
            .map(|r| (r.path.as_str(), r.reason.as_str()))
            .collect();
        assert_eq!(reasons.get("mods/no-url.jar"), Some(&"no_url"));
        assert_eq!(reasons.get("mods/disabled.jar"), Some(&"disabled"));
        assert_eq!(reasons.get("mods/gone.jar"), Some(&"missing"));
        assert_eq!(reasons.get("mods/manual.jar"), Some(&"unlinked"));
        assert_eq!(reasons.len(), 4, "всё, что не в files[], в отчёте");
    }

    /// Полный путь (офлайн): files[] по url из манифеста, ручные файлы и
    /// конфиги в overrides, мир по флагу, логи и чужие миры — мимо.
    #[tokio::test]
    async fn full_export_offline_with_world() {
        let (dir, paths) = test_paths();
        let mut inst = Instance::new("Пак для теста", "1.20.1");
        inst.loader = Some("fabric".into());
        inst.loader_version = Some("0.16.0".into());
        inst.quick_play_world = Some("MyWorld".into());
        crate::instances::save(&paths, &inst).unwrap();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));

        write_mc_file(&mc, "mods/sodium.jar", b"sodium bytes");
        write_mc_file(&mc, "mods/manual.jar", b"manual bytes");
        write_mc_file(&mc, "mods/old.jar.disabled", b"disabled bytes");
        write_mc_file(&mc, "config/opt.cfg", b"key=value");
        write_mc_file(&mc, "saves/MyWorld/level.dat", b"my world");
        write_mc_file(&mc, "saves/OtherWorld/level.dat", b"other world");
        write_mc_file(&mc, "logs/latest.log", b"log");

        let sha1 = crate::util::fs::sha1_file(&long_path(&mc.join("mods/sodium.jar"))).unwrap();
        let sha512 = crate::util::fs::sha512_file(&long_path(&mc.join("mods/sodium.jar"))).unwrap();
        save_content_manifest(
            &paths,
            &inst.id,
            &[
                entry(
                    "mods/sodium.jar",
                    ContentSource::Modrinth,
                    Some("AANobbMI"),
                    Some("https://cdn.modrinth.com/data/AANobbMI/sodium.jar"),
                ),
                entry("mods/manual.jar", ContentSource::Local, None, None),
            ],
        )
        .unwrap();

        let client = HttpClient::new(None).unwrap();
        client.set_offline(true); // тесты без сети: url берём из манифеста
        let out = dir.path().join("pack.mrpack");
        let res = export_instance_mrpack_in(&paths, &client, &inst.id, &out, true)
            .await
            .unwrap();

        assert_eq!(res.files_sourced, 1);
        assert_eq!(res.path, out.to_string_lossy());
        let skipped: HashMap<&str, &str> = res
            .skipped
            .iter()
            .map(|r| (r.path.as_str(), r.reason.as_str()))
            .collect();
        assert_eq!(skipped.get("mods/manual.jar"), Some(&"unlinked"));
        assert_eq!(skipped.len(), 1, "прочих пропусков нет: {skipped:?}");

        // Архив: индекс + overrides, ничего лишнего.
        let file = std::fs::File::open(long_path(&out)).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        for expected in [
            "modrinth.index.json",
            "overrides/mods/manual.jar",
            "overrides/config/opt.cfg",
            "overrides/world/level.dat",
        ] {
            assert!(names.iter().any(|n| n == expected), "нет {expected}: {names:?}");
        }
        for forbidden in [
            "overrides/mods/sodium.jar", // уже в files[]
            "overrides/mods/old.jar.disabled",
            "overrides/saves/MyWorld/level.dat",
            "overrides/world/OtherWorld/level.dat", // чужой мир не включён
            "overrides/logs/latest.log",
        ] {
            assert!(!names.iter().any(|n| n == forbidden), "лишнее {forbidden}: {names:?}");
        }
        let index: serde_json::Value =
            serde_json::from_reader(zip.by_name("modrinth.index.json").unwrap()).unwrap();
        assert_eq!(index["name"], "Пак для теста");
        assert_eq!(index["dependencies"]["minecraft"], "1.20.1");
        assert_eq!(index["dependencies"]["fabric-loader"], "0.16.0");
        let f = &index["files"][0];
        assert_eq!(f["path"], "mods/sodium.jar");
        assert_eq!(f["hashes"]["sha1"], sha1);
        assert_eq!(f["hashes"]["sha512"], sha512);
        assert_eq!(f["fileSize"], "sodium bytes".len() as u64);
        assert_eq!(f["downloads"][0], "https://cdn.modrinth.com/data/AANobbMI/sodium.jar");
        assert!(f.get("env").is_none(), "env не пишем");
        assert!(res.files_overrides >= 3, "manual.jar + opt.cfg + мир: {res:?}");

        // Мир и overrides в одном инстансе не задвоены; второй экспорт без мира.
        drop(zip);
        let out2 = dir.path().join("no-world.mrpack");
        let res2 = export_instance_mrpack_in(&paths, &client, &inst.id, &out2, false)
            .await
            .unwrap();
        assert_eq!(res2.files_sourced, 1, "files[] и без мира на месте");
        let file2 = std::fs::File::open(long_path(&out2)).unwrap();
        let mut zip2 = zip::ZipArchive::new(file2).unwrap();
        let names2: Vec<String> = (0..zip2.len())
            .map(|i| zip2.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(
            !names2.iter().any(|n| n.starts_with("overrides/world")),
            "без include_worlds мира нет: {names2:?}"
        );
    }

    /// Привязка к Modrinth без ссылки + офлайн: файл уезжает в overrides,
    /// в отчёте — no_url, files[] пуст.
    #[tokio::test]
    async fn offline_without_url_goes_to_overrides() {
        let (dir, paths) = test_paths();
        let inst = Instance::new("Офлайн-пак", "1.21.1");
        crate::instances::save(&paths, &inst).unwrap();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        write_mc_file(&mc, "mods/handmade.jar", b"jar");

        save_content_manifest(
            &paths,
            &inst.id,
            &[entry(
                "mods/handmade.jar",
                ContentSource::Modrinth,
                Some("PROJ1"),
                None,
            )],
        )
        .unwrap();

        let client = HttpClient::new(None).unwrap();
        client.set_offline(true);
        let out = dir.path().join("offline.mrpack");
        let res = export_instance_mrpack_in(&paths, &client, &inst.id, &out, false)
            .await
            .unwrap();
        assert_eq!(res.files_sourced, 0);
        assert_eq!(res.skipped.len(), 1);
        assert_eq!(res.skipped[0].reason, "no_url");
        assert_eq!(res.skipped[0].path, "mods/handmade.jar");
        assert_eq!(res.files_overrides, 1, "файл уехал в overrides");

        let file = std::fs::File::open(long_path(&out)).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let mut buf = Vec::new();
        {
            let mut e = zip.by_name("overrides/mods/handmade.jar").unwrap();
            std::io::Read::read_to_end(&mut e, &mut buf).unwrap();
        }
        assert_eq!(buf, b"jar");
    }

    /// Путь не .mrpack — InvalidInput до всякой работы.
    #[tokio::test]
    async fn rejects_non_mrpack_output() {
        let (_d, paths) = test_paths();
        let inst = Instance::new("x", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        let client = HttpClient::new(None).unwrap();
        let out = tempfile::tempdir().unwrap().path().join("pack.zip");
        let err = export_instance_mrpack_in(&paths, &client, &inst.id, &out, false)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "invalid_input", "{err}");
    }

    /// Работающий инстанс (живой .lock) экспортировать нельзя — B9, как у
    /// backup/экспорта в content.rs.
    #[tokio::test]
    async fn rejects_running_instance() {
        let (dir, paths) = test_paths();
        let inst = Instance::new("busy", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        let lock = instance_dir(&paths, &inst.id).join(".lock");
        std::fs::write(long_path(&lock), std::process::id().to_string()).unwrap();
        let client = HttpClient::new(None).unwrap();
        let out = dir.path().join("busy.mrpack");
        let err = export_instance_mrpack_in(&paths, &client, &inst.id, &out, false)
            .await
            .unwrap_err();
        assert!(matches!(err, LauncherError::InstanceRunning(_)), "{err}");
    }

    /// Несуществующий инстанс — NotFound; traversal-id — InvalidInput.
    #[tokio::test]
    async fn rejects_missing_instance_and_bad_id() {
        let (_d, paths) = test_paths();
        let client = HttpClient::new(None).unwrap();
        let out = std::env::temp_dir().join("mc-launcher-v2-export-test.mrpack");
        let err = export_instance_mrpack_in(&paths, &client, "no-such-id", &out, false)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "not_found", "{err}");
        for id in ["../evil", "a/b", ""] {
            let err = export_instance_mrpack_in(&paths, &client, id, &out, false)
                .await
                .unwrap_err();
            assert_eq!(err.code(), "invalid_input", "id {id:?}: {err}");
        }
        let _ = std::fs::remove_file(long_path(&out));
    }

    /// Мир выбирается в порядке: quick_play_world → `world` → единственный.
    #[test]
    fn world_picking_order() {
        let dir = tempfile::tempdir().unwrap();
        let mc = dir.path().to_path_buf();
        let mut inst = Instance::new("w", "1.20.1");
        assert!(pick_world_dir(&mc, &inst).is_none(), "saves нет — None");
        write_mc_file(&mc, "saves/Alpha/level.dat", b"a");
        assert_eq!(
            pick_world_dir(&mc, &inst).unwrap(),
            mc.join("saves/Alpha"),
            "единственный мир берётся"
        );
        write_mc_file(&mc, "saves/Beta/level.dat", b"b");
        assert!(pick_world_dir(&mc, &inst).is_none(), "два мира без quick_play — None");
        write_mc_file(&mc, "saves/world/level.dat", b"w");
        assert_eq!(pick_world_dir(&mc, &inst).unwrap(), mc.join("saves/world"));
        inst.quick_play_world = Some("Beta".into());
        assert_eq!(pick_world_dir(&mc, &inst).unwrap(), mc.join("saves/Beta"));
    }

    /// Неоднозначный выбор — отдельный статус: два мира без quick_play —
    /// Ambiguous (в отчёт уходит world_ambiguous), с указанием — Dir.
    #[test]
    fn world_pick_distinguishes_ambiguity() {
        let dir = tempfile::tempdir().unwrap();
        let mc = dir.path().to_path_buf();
        let mut inst = Instance::new("w", "1.20.1");
        write_mc_file(&mc, "saves/Alpha/level.dat", b"a");
        write_mc_file(&mc, "saves/Beta/level.dat", b"b");
        assert!(matches!(pick_world(&mc, &inst), WorldPick::Ambiguous));
        inst.quick_play_world = Some("Alpha".into());
        assert_eq!(
            pick_world(&mc, &inst),
            WorldPick::Dir(mc.join("saves/Alpha"))
        );
    }

    /// Однопроходный хэш: пара (sha1, sha512) и размер совпадают с эталоном
    /// через прежние sha1_file/sha512_file — на многочанковом (>128 КБ) и
    /// пустом файлах; нечитаемый путь — ошибка.
    #[test]
    fn single_pass_hash_matches_reference() {
        let dir = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..300 * 1024u32).map(|i| (i % 251) as u8).collect();
        let big = dir.path().join("big.bin");
        std::fs::write(long_path(&big), &data).unwrap();
        let (sha1, sha512, size) = hash_file_once(&long_path(&big)).unwrap();
        assert_eq!(sha1, crate::util::fs::sha1_file(&long_path(&big)).unwrap());
        assert_eq!(
            sha512,
            crate::util::fs::sha512_file(&long_path(&big)).unwrap()
        );
        assert_eq!(size, data.len() as u64);

        let empty = dir.path().join("empty.bin");
        std::fs::write(long_path(&empty), b"").unwrap();
        let (s1, s2, sz) = hash_file_once(&long_path(&empty)).unwrap();
        assert_eq!(s1, crate::util::fs::sha1_file(&long_path(&empty)).unwrap());
        assert_eq!(
            s2,
            crate::util::fs::sha512_file(&long_path(&empty)).unwrap()
        );
        assert_eq!(sz, 0);
        assert!(hash_file_once(&long_path(&dir.path().join("nope.bin"))).is_err());
    }

    /// Фильтр путей манифеста: нормальные относительные проходят,
    /// `..`/абсолютные/диск/пустые/NUL — нет.
    #[test]
    fn manifest_path_filter() {
        for ok in ["mods/a.jar", "resourcepacks/pack.zip", "a/b/c.dat"] {
            assert!(traversal_reason(ok).is_none(), "{ok}");
        }
        for bad in [
            "",
            ".",
            "..",
            "../evil.jar",
            "mods/../evil.jar",
            "C:/abs.jar",
            "/abs.jar",
            "a\\..\\b.jar",
            "mod\0.jar",
        ] {
            assert!(traversal_reason(bad).is_some(), "{bad:?}");
        }
    }

    /// Traversal-пути манифеста отбраковываются в плане: и sourced-ветка
    /// (привязка+ссылка), и unlinked-ветка уходят в skipped с traversal.
    #[test]
    fn plan_rejects_traversal_paths() {
        let dir = tempfile::tempdir().unwrap();
        let mc = dir.path().to_path_buf();
        write_mc_file(&mc, "mods/ok.jar", b"ok");
        let entries = vec![
            entry("../evil.jar", ContentSource::Local, None, None),
            entry(
                "C:/abs.jar",
                ContentSource::Modrinth,
                Some("PROJ"),
                Some("https://cdn.modrinth.com/abs.jar"),
            ),
            entry("mods/ok.jar", ContentSource::Local, None, None),
        ];
        let plan = plan_export(&entries, &HashMap::new(), &mc);
        assert!(plan.sourced.is_empty(), "traversal не проходит в files[]");
        assert_eq!(
            plan.unlinked,
            vec!["mods/ok.jar".to_string()],
            "до копий доходит только безопасный путь"
        );
        let reasons: HashMap<&str, &str> = plan
            .skipped
            .iter()
            .map(|r| (r.path.as_str(), r.reason.as_str()))
            .collect();
        assert_eq!(reasons.get("../evil.jar"), Some(&"traversal"));
        assert_eq!(reasons.get("C:/abs.jar"), Some(&"traversal"));
        assert_eq!(reasons.get("mods/ok.jar"), Some(&"unlinked"));
    }

    /// Traversal в манифесте (полный прогон, офлайн): ../evil.jar (файл
    /// существует и существует за пределами minecraft/) и C:/abs.jar не
    /// выходят за staging — в отчёте traversal, в архиве их нет.
    #[tokio::test]
    async fn traversal_paths_never_reach_the_pack() {
        let (dir, paths) = test_paths();
        let inst = Instance::new("trav", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        let inst_root = instance_dir(&paths, &inst.id);
        let mc = minecraft_dir(&inst_root);
        write_mc_file(&mc, "mods/manual.jar", b"manual bytes");
        // «жертва» ровно там, куда целился бы ../evil.jar из minecraft/.
        std::fs::write(long_path(&inst_root.join("evil.jar")), b"secret").unwrap();

        save_content_manifest(
            &paths,
            &inst.id,
            &[
                entry("../evil.jar", ContentSource::Local, None, None),
                entry(
                    "C:/abs.jar",
                    ContentSource::Modrinth,
                    Some("PROJ"),
                    Some("https://cdn.modrinth.com/abs.jar"),
                ),
            ],
        )
        .unwrap();

        let client = HttpClient::new(None).unwrap();
        client.set_offline(true);
        let out = dir.path().join("trav.mrpack");
        let res = export_instance_mrpack_in(&paths, &client, &inst.id, &out, false)
            .await
            .unwrap();
        assert_eq!(res.files_sourced, 0);
        let skipped: HashMap<&str, &str> = res
            .skipped
            .iter()
            .map(|r| (r.path.as_str(), r.reason.as_str()))
            .collect();
        assert_eq!(skipped.get("../evil.jar"), Some(&"traversal"));
        assert_eq!(skipped.get("C:/abs.jar"), Some(&"traversal"));

        let file = std::fs::File::open(long_path(&out)).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.iter().all(|n| !n.contains("evil.jar")), "{names:?}");
        assert!(names.iter().all(|n| !n.contains("abs.jar")), "{names:?}");
        assert!(
            names.iter().any(|n| n == "overrides/mods/manual.jar"),
            "обычный файл на месте: {names:?}"
        );
    }

    /// Несколько миров без quick_play_world — видимый отказ: skipped
    /// { path: "world", reason: "world_ambiguous" }, мира в архиве нет.
    #[tokio::test]
    async fn ambiguous_world_reported_not_silently_skipped() {
        let (dir, paths) = test_paths();
        let inst = Instance::new("ambiguous", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        write_mc_file(&mc, "saves/Alpha/level.dat", b"a");
        write_mc_file(&mc, "saves/Beta/level.dat", b"b");

        let client = HttpClient::new(None).unwrap();
        client.set_offline(true);
        let out = dir.path().join("amb.mrpack");
        let res = export_instance_mrpack_in(&paths, &client, &inst.id, &out, true)
            .await
            .unwrap();
        assert!(
            res.skipped
                .iter()
                .any(|r| r.path == "world" && r.reason == "world_ambiguous"),
            "{:?}",
            res.skipped
        );

        let file = std::fs::File::open(long_path(&out)).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(
            !names.iter().any(|n| n.starts_with("overrides/world")),
            "мир не выбран вслепую: {names:?}"
        );
    }
}
