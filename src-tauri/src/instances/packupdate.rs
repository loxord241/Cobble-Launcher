//! Обновление модпака целиком (F1): инстанс, поставленный из .mrpack или
//! каталога Modrinth, узнаёт о свежей версии пака и обновляется на месте.
//!
//! Порядок применения — честная защита пользователя: бэкап инстанса целиком →
//! скачивание .mrpack новой версии движком загрузок (dl-группа видна в
//! «Загрузках») со сверкой sha1 → дифф манифестов (новые файлы докачать,
//! выбывшие из пака удалить, пользовательские не трогать) → overrides нового
//! пака поверх → новая ссылка пака в instance.json. Любая ошибка после бэкапа
//! оставляет возможность откатиться руками; манифест и instance.json пишутся
//! последними, чтобы повторная попытка пересчитала тот же дифф.

use crate::errors::{LauncherError, Result};
use crate::instances::{ContentEntry, ContentKind, ContentSource, Instance, ModpackRef};
use crate::modrinth::api::{self, Version};
use crate::modrinth::mrpack::{ensure_allowed_mrpack_url, ids_from_cdn_url, MrpackIndex};
use crate::net::download::{DownloadEngine, DownloadTask};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Потолок ченджлога в PackUpdateInfo: UI показывает пару строк, а ченджлоги
/// паков бывают на экраны текста. Режем по СИМВОЛАМ (не байтам) — кириллица.
const CHANGELOG_MAX_CHARS: usize = 600;

/// D70: реестр per-instance локов критической секции read-modify-write
/// манифеста контента (content-manifest.json). `pack_update_apply` (здесь) и
/// мутирующие контентные операции (content::toggle/remove/install_local/
/// copy_entry, install::download_and_record, updates::update_all) пишут ОДИН
/// файл: без лока параллельные вызовы теряли бы обновление (кто последний
/// записал — тот и прав). Лок на каждый инстанс свой: операции разных
/// инстансов не блокируют друг друга. Arc — лок живёт, пока его держит хоть
/// одна секция, даже если реестр уже выкинул запись (записи не выкидываем:
/// их столько, сколько инстансов). Лок через сетевые await НЕ держится:
/// скачивания — вне критических секций.
static CONTENT_MANIFEST_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    OnceLock::new();

/// D70: лок критической секции read-modify-write манифеста контента
/// инстанса `instance_id`. Берётся ТОЛЬКО на время локальных дисковых
/// операций (перечитать → изменить → записать), никогда — через сетевые
/// await. Отравленный реестр (паника предыдущего владельца) не мешает:
/// внутри просто Arc'ы, забираем их как есть.
pub(crate) fn content_manifest_lock(instance_id: &str) -> Arc<Mutex<()>> {
    let registry = CONTENT_MANIFEST_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = registry.lock().unwrap_or_else(|p| p.into_inner());
    map.entry(instance_id.to_string()).or_default().clone()
}

/// D70: проект пака снят с Modrinth (404/410) — тот же предикат, что у
/// поштучной проверки контента (updates.rs::project_gone): тихую проверку
/// это не должно ронять.
fn gone(err: &LauncherError) -> bool {
    matches!(err, LauncherError::HttpStatus { status: 404 | 410, .. })
}

/// Найденное обновление модпака (контракт фронту, camelCase).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackUpdateInfo {
    pub version_id: String,
    pub version_number: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changelog: Option<String>,
}

/// Базовый URL Modrinth API: в прод-сборке — константа, в тестах — локальный
/// mock-сервер (внешней сети в тестах нет). Как в updates.rs/mrpack.rs.
fn api_base() -> String {
    #[cfg(test)]
    if let Some(base) = TEST_API_BASE.get() {
        return base.clone();
    }
    api::API_BASE.to_string()
}

#[cfg(test)]
static TEST_API_BASE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Версии проекта: тот же эндпоинт и `get_json_retry`, что api::project_versions,
/// но через api_base() — иначе тестовый mock этот путь не перехватывает.
async fn project_versions(client: &HttpClient, project_id: &str) -> Result<Vec<Version>> {
    // id уходит в URL запроса: мусорный id из instance.json отсекаем до сети.
    api::validate_id_or_slug(project_id)?;
    let url = format!("{}/project/{project_id}/version", api_base());
    client.get_json_retry(&url).await
}

/// Обрезать ченджлог до CHANGELOG_MAX_CHARS символов (чистая функция).
fn truncate_changelog(changelog: Option<&str>) -> Option<String> {
    changelog.map(|c| c.chars().take(CHANGELOG_MAX_CHARS).collect())
}

/// Свежая версия пака из ответа `/project/{id}/version`: Modrinth отдаёт
/// версии от новых к старым, поэтому «новее» — ПЕРВАЯ запись списка, и только
/// если её id не совпадает с установленным (совпадение = мы уже на свежей).
/// Буквальное «первая запись с другим id» предлагало бы СТАРЫЕ версии, когда
/// инстанс уже на последней — остальные записи списка всегда «с другим id».
fn newest_update(versions: &[Version], current_version_id: &str) -> Option<PackUpdateInfo> {
    let first = versions.first()?;
    if first.id == current_version_id {
        return None;
    }
    Some(PackUpdateInfo {
        version_id: first.id.clone(),
        version_number: first.version_number.clone(),
        changelog: truncate_changelog(first.changelog.as_deref()),
    })
}

/// Тихая проверка: есть ли у пака инстанса свежая версия. Только для
/// инстансов с `modpack` (остальным — Ok(None), сети нет). У импорта .mrpack
/// project_id пуст — источник обновлений неизвестен, тоже Ok(None).
pub async fn check(
    // Зарезервировано: проверка пока читает только сеть и instance.json.
    _paths: &Paths,
    http: &HttpClient,
    inst: &Instance,
) -> Result<Option<PackUpdateInfo>> {
    let Some(pack) = &inst.modpack else {
        return Ok(None);
    };
    if pack.project_id.is_empty() {
        return Ok(None);
    }
    // F16: офлайн — мгновенный честный отказ вместо сетевого таймаута (UI
    // молча скрывает баннер).
    if http.offline() {
        return Err(LauncherError::OfflineMode(
            "проверка обновлений модпака".into(),
        ));
    }
    // D70: если установленная версия пака удалена с Modrinth, список версий
    // её больше не содержит — newest_update честно предложит последнюю
    // ОСТАВШУЮСЯ версию (её номер может быть МЕНЬШЕ установленного — это
    // валидный даунгрейд, apply применяет его штатно: бэкап + дифф).
    // Снятый целиком проект (404/410) — не ошибка тихой проверки: оставшихся
    // версий список не отдал, предложить нечего — Ok(None), баннер исчезает.
    let versions = match project_versions(http, &pack.project_id).await {
        Ok(v) => v,
        Err(e) if gone(&e) => {
            tracing::warn!(
                "проверка обновлений пака: проект {} недоступен ({e})",
                pack.project_id
            );
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    Ok(newest_update(&versions, &pack.version_id))
}

/// Файл нового пака, разобранный до задач загрузок: логический путь для
/// манифеста, хэши и ids конкретного файла (kind = None → конфиг/прочее,
/// в манифест контента не попадает — как в install_mrpack).
#[derive(Clone)]
struct PlannedFile {
    /// Путь в манифесте, например `mods/sodium.jar` (символы `/`).
    file: String,
    url: String,
    sha1: Option<String>,
    sha512: Option<String>,
    size: u64,
    kind: Option<ContentKind>,
    project_id: Option<String>,
    version_id: Option<String>,
}

/// План файлов из манифеста пака: те же барьеры, что при установке (A24 —
/// allowlist хостов ДО сети, санитизация пути чужого архива, «хэш на каждый
/// байт»), env-фильтр client. Всё ДО изменений на диске: битый пак нового
/// пака не должен успеть испортить старый инстанс.
fn plan_files(index: &MrpackIndex) -> Result<Vec<PlannedFile>> {
    let mut out = Vec::new();
    for f in index.client_files() {
        if f.downloads.is_empty() {
            return Err(LauncherError::InvalidInput(format!(
                "у файла {} нет ссылок для скачивания",
                f.path
            )));
        }
        for url in &f.downloads {
            ensure_allowed_mrpack_url(url)?;
        }
        let rel = crate::util::zip::safe_relative_path(&f.path)?;
        let sha1 = f.hashes.get("sha1").cloned();
        let sha512 = f.hashes.get("sha512").cloned();
        if sha1.is_none() && sha512.is_none() {
            return Err(LauncherError::InvalidInput(format!(
                "в манифесте нет хэша для {}",
                f.path
            )));
        }
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let kind = if rel_str.starts_with("mods/") {
            Some(ContentKind::Mod)
        } else if rel_str.starts_with("resourcepacks/") {
            Some(ContentKind::ResourcePack)
        } else if rel_str.starts_with("shaderpacks/") {
            Some(ContentKind::Shader)
        } else {
            None // конфиги и прочее — не контент для манифеста (как при установке)
        };
        let (url_project, url_version) = f
            .downloads
            .first()
            .map(|u| ids_from_cdn_url(u))
            .unwrap_or((None, None));
        out.push(PlannedFile {
            file: rel_str,
            url: f.downloads[0].clone(),
            sha1,
            sha512,
            size: f.file_size,
            kind,
            project_id: f.project_id.clone().or(url_project),
            version_id: f.version_id.clone().or(url_version),
        });
    }
    Ok(out)
}

/// Записи старого манифеста, поставленные старым паком (D71: `from_pack`,
/// а не source — Modrinth-мод, поставленный пользователем через «Добавить
/// моды», пишет from_pack=false и удалению не подлежит) и выбывшие из новой
/// версии — кандидаты на удаление. Совпавшие с новым паком пути сюда не
/// попадают. Легаси-записи без поля читаются как паковые (serde default
/// true) — поведение D69 сохранено.
fn removed_pack_entries(old: &[ContentEntry], planned: &[PlannedFile]) -> Vec<ContentEntry> {
    old.iter()
        .filter(|e| e.from_pack)
        .filter(|e| !planned.iter().any(|p| p.file == e.file))
        .cloned()
        .collect()
}

/// Пользовательский контент (не из пака: Local, CurseForge и любые записи
/// с from_pack=false): переносится в новый манифест как есть — обновление
/// пака не имеет права трогать чужие файлы.
fn preserved_user_entries(old: &[ContentEntry], planned: &[PlannedFile]) -> Vec<ContentEntry> {
    old.iter()
        .filter(|e| !e.from_pack)
        .filter(|e| !planned.iter().any(|p| p.file == e.file))
        .cloned()
        .collect()
}

/// Прежнее состояние вкл/выкл файла (.disabled-суффикс = «тот же файл»):
/// Some — файл был у старого пака, None — новый.
fn old_enabled_for(old: &[ContentEntry], file: &str) -> Option<bool> {
    old.iter().find(|e| e.file == file).map(|e| e.enabled)
}

/// Файл на диске побитово совпадает с sha1 из манифеста (нечитаем/нет — нет).
fn file_matches_sha1(dest: &Path, want: &str) -> bool {
    crate::util::fs::sha1_file(&crate::util::fs::long_path(dest))
        .map(|actual| actual.eq_ignore_ascii_case(want))
        .unwrap_or(false)
}

/// D71: rel-путь из overrides распаковывается, только если НИ ОДИН компонент
/// пути (по `/`) не равен «saves» без учёта регистра. Прежний фильтр
/// (`starts_with("saves/") || contains("/saves/")`) был регистрозависим:
/// на Windows `Saves/...` из чужого архива резолвился в РЕАЛЬНЫЕ миры
/// пользователя, а «голое» имя компонента `SAVES` (файл без слэша) вовсе
/// проходило. Компонентное сравнение отсекает и `mods/Saves/x`; при этом
/// `savegames/x` и `saveslike.jar` (подстрока, но не компонент) проходят.
fn touches_saves(rel: &str) -> bool {
    rel.replace('\\', "/")
        .split('/')
        .any(|c| c.eq_ignore_ascii_case("saves"))
}

/// Задачи загрузки для файлов пака. Уже лежащий файл с сошедшимся sha1
/// повторно не качается (паки бывают на сотни МБ, большинство файлов между
/// версиями не меняется); выключенный (.disabled) файл качается сразу в
/// выключенном имени, чтобы на диске не появилось два экземпляра.
fn build_tasks(
    planned: &[PlannedFile],
    old: &[ContentEntry],
    mc_dir: &Path,
    group: &str,
) -> Vec<DownloadTask> {
    planned
        .iter()
        .filter_map(|p| {
            let enabled = old_enabled_for(old, &p.file).unwrap_or(true);
            let dest = mc_dir.join(crate::instances::content::disk_path(&p.file, enabled));
            if p.sha1
                .as_deref()
                .is_some_and(|sha| file_matches_sha1(&dest, sha))
            {
                return None; // уже на диске и хэш сошёлся — сеть не трогаем
            }
            Some(DownloadTask {
                id: format!("packupdate:{}", p.file),
                url: p.url.clone(),
                dest,
                sha1: p.sha1.clone(),
                sha512: p.sha512.clone(),
                size: Some(p.size),
                group: group.to_string(),
                priority: 15,
            })
        })
        .collect()
}

/// tmp .mrpack живёт только до конца apply — guard снесёт его при ЛЮБОМ
/// выходе (успех, ошибка, отмена), как DuplicateCleanup в instances/mod.rs.
struct TempPack(PathBuf);

impl Drop for TempPack {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(crate::util::fs::long_path(&self.0));
    }
}

/// Обновить модпак-инстанс до свежей версии на месте. Скачивания идут через
/// движок с группой `instance:{id}:packupdate` — прогресс виден в рельсе
/// «Загрузки», отмена группы отменяет обновление. `progress` — текстовые
/// статусы шагов для UI. Возвращает обновлённый инстанс.
pub async fn apply(
    paths: &Paths,
    settings: &crate::settings::Settings,
    engine: Arc<DownloadEngine>,
    http: Arc<HttpClient>,
    instance_id: &str,
    keep: u32,
    progress: &(dyn Fn(String) + Send + Sync),
) -> Result<Instance> {
    let inst = crate::instances::load(paths, instance_id)?;
    let Some(old_pack) = inst.modpack.clone() else {
        return Err(LauncherError::InvalidInput(
            "инстанс установлен не из модпака — обновлять нечего".into(),
        ));
    };
    if old_pack.project_id.is_empty() {
        return Err(LauncherError::InvalidInput(
            "модпак импортирован без проекта Modrinth — проверить обновления нельзя".into(),
        ));
    }
    // B9-принцип: у работающей игры заняты файлы — бэкап/обновление только
    // после выхода.
    if crate::instances::running_pid(&crate::instances::instance_dir(paths, instance_id)).is_some()
    {
        return Err(LauncherError::InstanceRunning(instance_id.to_string()));
    }
    // F16: офлайн — мгновенный честный отказ (скачивать пачку гигабайт смысла нет).
    if http.offline() {
        return Err(LauncherError::OfflineMode("обновление модпака".into()));
    }

    // 1. Свежая версия пака — ДЕШЁВАЯ сетевая проверка ДО дорогого бэкапа
    // (D69-ревизия): устаревший баннер или сбой сети не должны стоить
    // пользователю гигабайтного zip. Modrinth отдаёт от новых к старым;
    // совпадение первой записи с установленной — обновления нет.
    let versions = project_versions(&http, &old_pack.project_id).await?;
    let Some(newest) = versions
        .first()
        .filter(|v| v.id != old_pack.version_id)
        .cloned()
    else {
        return Err(LauncherError::InvalidInput(
            "свежая версия модпака не найдена — установленная уже последняя".into(),
        ));
    };
    let new_ref = ModpackRef {
        project_id: old_pack.project_id.clone(),
        version_id: newest.id.clone(),
        version_number: newest.version_number.clone(),
    };

    // 3. .mrpack новой версии: primary-файл (или первый). Хост — из
    // allowlist-проверки, sha1 из версии Modrinth ОБЯЗАТЕЛЕН: непроверенный
    // архив модпака не скачиваем вовсе.
    let pack_file = newest
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| newest.files.first())
        .ok_or_else(|| {
            LauncherError::InvalidInput(format!(
                "у версии {} модпака нет файлов",
                newest.version_number
            ))
        })?;
    let pack_sha1 = pack_file.hashes.get("sha1").cloned().ok_or_else(|| {
        LauncherError::InvalidInput(format!(
            "у архива версии {} нет sha1 — качать непроверенное нельзя",
            newest.version_number
        ))
    })?;
    ensure_allowed_mrpack_url(&pack_file.url)?;
    let tmp_dir = std::env::temp_dir().join("mc-launcher-v2");
    std::fs::create_dir_all(crate::util::fs::long_path(&tmp_dir))?;
    let tmp_tail = uuid::Uuid::new_v4().simple().to_string();
    let tmp_path = tmp_dir.join(format!(
        "{}-{}",
        &tmp_tail[..8],
        api::sanitize_file_name(&pack_file.filename)
    ));
    let _tmp = TempPack(tmp_path.clone());
    let group = format!("instance:{instance_id}:packupdate");
    progress(format!("скачиваю модпак {}…", newest.version_number));
    engine.add_tasks(vec![DownloadTask {
        id: format!("packupdate:{}", pack_file.filename),
        url: pack_file.url.clone(),
        dest: tmp_path.clone(),
        sha1: Some(pack_sha1),
        sha512: pack_file.hashes.get("sha512").cloned(),
        size: Some(pack_file.size),
        group: group.clone(),
        priority: 15,
    }]);
    engine.run().await?;
    let st = engine.queue_state();
    if st.failed > 0 {
        return Err(LauncherError::network(format!(
            "архив модпака не скачался: {}",
            st.failed_items
                .first()
                .map(|(u, r)| format!("{u}: {r}"))
                .unwrap_or_default()
        )));
    }
    if st.cancelled > 0 {
        return Err(LauncherError::Cancelled);
    }

    // 4. Манифест нового пака + валидация загрузчика ДО всяких мутаций:
    // неизвестный загрузчик/битый манифест не должны стоить пользователю
    // удалённых файлов. Снимок старого манифеста НЕ делается заранее (D71):
    // эвристика скачивания (вкл/выкл, пропуск совпавших хэшей) берёт СВЕЖУЮ
    // перечитку под локом в момент построения задач (шаг 5), а сам дифф —
    // внутри критической секции (шаг 6–8).
    let index = crate::modrinth::mrpack::open_mrpack(&tmp_path)?;
    let loader = crate::modrinth::mrpack::resolve_loader(&index.dependencies)?;
    let planned = plan_files(&index)?;

    // Бэкап инстанса целиком (zip в backups/) — честная защита: дальше идут
    // массовые удаления/перезаписи. Теперь ПОСЛЕ сетевых проверок и разбора
    // манифеста: до мутаций, но без бесполезного zip при устаревшем баннере.
    progress("создаю бэкап инстанса…".into());
    let backup = {
        let b_paths = paths.clone();
        let b_inst = inst.clone();
        let b_name = inst.name.clone();
        tokio::task::spawn_blocking(move || -> crate::errors::Result<crate::instances::content::ArchiveResult> {
            let res = crate::instances::content::backup(&b_paths, &b_inst)?;
            // D69-ревизия: настройка ротации действует и на авто-бэкап
            // обновления пака, а не только на ручной instance_backup.
            crate::instances::backups::rotate(&b_paths, &b_name, keep);
            Ok(res)
        })
        .await
        .map_err(|e| LauncherError::Io(std::io::Error::other(format!("бэкап: {e}"))))??
    };
    progress(format!(
        "бэкап готов: {} ({} файлов)",
        backup.path, backup.files
    ));

    // 5. Файлы нового пака (кроме уже лежащих с сошедшимся sha1) — одним
    // прогоном движка, та же dl-группа. Хэш-дифф — в blocking-пул: sha1 по
    // сотням файлов пака не должен морозить async-рантайм (A20/D14).
    let mc_dir =
        crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, instance_id));
    // D71: dest задач строится по СВЕЖЕМУ манифесту, перечитанному под
    // per-instance локом в момент построения задач (а не по снимку, снятому
    // до скачивания .mrpack): toggle/remove, завершившиеся за это время,
    // больше не игнорируются — задача выключенной записи сразу пишется в
    // `.disabled`-имя, и на диске не появляется дубль. ОСТАТОЧНОЕ ОКНО
    // (документировано осознанно): переключение контента ПОСЛЕ построения
    // задач, но до фактической записи файла движком, всё ещё может положить
    // файл в «старое» имя — движок скачивания недоступен для такой проверки
    // из этого слоя. Манифест-финал это не ломает: шаг 6–8 перечитывает
    // состояние под тем же локом и переносит вкл/выкл из него.
    let tasks = {
        let t_planned = planned.clone();
        let t_paths = paths.clone();
        let t_id = instance_id.to_string();
        let t_dir = mc_dir.clone();
        let t_group = group.clone();
        tokio::task::spawn_blocking(move || {
            let lock = content_manifest_lock(&t_id);
            let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
            let fresh = crate::instances::load_content_manifest(&t_paths, &t_id);
            build_tasks(&t_planned, &fresh, &t_dir, &t_group)
        })
        .await
        .map_err(|e| LauncherError::Io(std::io::Error::other(format!("дифф пака: {e}"))))?
    };
    if !tasks.is_empty() {
        progress(format!("файлов модпака: {}…", tasks.len()));
        engine.add_tasks(tasks);
        engine.run().await?;
        let st = engine.queue_state();
        if st.failed > 0 {
            return Err(LauncherError::network(format!(
                "{} файлов модпака не скачались: {}",
                st.failed,
                st.failed_items
                    .first()
                    .map(|(u, r)| format!("{u}: {r}"))
                    .unwrap_or_default()
            )));
        }
        if st.cancelled > 0 {
            return Err(LauncherError::Cancelled);
        }
    }

    // 6–8. D70: критическая секция read-modify-write манифеста под
    // per-instance локом content_manifest_lock — параллельные контентные
    // операции (toggle/remove/install_local/copy_entry) и повторный apply
    // больше не теряют обновления манифеста. Лок НЕ держится через сетевые
    // await: скачивания завершены выше, а вся секция (перечитать манифест →
    // дифф → удалить выбывших → overrides → записать манифест) — локальный
    // диск без сети, одним куском в blocking-пуле. Манифест ПЕРЕЧИТЫВАЕТСЯ
    // внутри секции: пока качались файлы, пользователь мог переключить или
    // удалить контент — дифф по устаревшему снимку вернул бы его выбор назад.
    let c_planned = planned.clone();
    let c_paths = paths.clone();
    let c_id = instance_id.to_string();
    let c_dir = mc_dir.clone();
    let c_tmp = tmp_path.clone();
    let (removed_len, extracted) = {
        tokio::task::spawn_blocking(move || -> Result<(usize, usize)> {
            let lock = content_manifest_lock(&c_id);
            let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
            let fresh = crate::instances::load_content_manifest(&c_paths, &c_id);
            let removed = removed_pack_entries(&fresh, &c_planned);

            // 6. Удаление выбывших из пака (обе формы имени, .disabled — «тот же
            // файл»). До записи манифеста: сбой здесь оставляет инстанс в старом
            // состоянии, повторная попытка пересчитает тот же дифф.
            for e in &removed {
                // Барьер traversal: удаляем только внутри каталогов контента.
                if crate::instances::content::resolve_content_file(&c_paths, &c_id, &e.file)
                    .is_err()
                {
                    tracing::warn!(
                        "обновление пака: подозрительный путь {} — удаление пропущено",
                        e.file
                    );
                    continue;
                }
                for enabled in [true, false] {
                    let p = crate::util::fs::long_path(
                        &c_dir.join(crate::instances::content::disk_path(&e.file, enabled)),
                    );
                    if p.exists() {
                        // ENV-11b: антивирус придерживает jar — повторяем удаление.
                        crate::util::fs::remove_file_with_retry(&p)?;
                    }
                }
            }

            // 7. Overrides нового пака поверх (перезапись — семантика обновления
            // пака). saves/ из overrides не распаковываем никогда: пользовательские
            // миры не пачкаются даже чужим архивом (zip-slip закрывает сам
            // экстрактор). D71: совпадение — по компонентам пути и без учёта
            // регистра (см. touches_saves): `Saves/…` на Windows это те же миры.
            let pack = std::fs::File::open(crate::util::fs::long_path(&c_tmp))?;
            let extracted = crate::util::zip::extract_zip_stripping(
                pack,
                &c_dir,
                "overrides/",
                |rel| !touches_saves(rel),
                |_| {},
            )
            .map(|v| v.len())?;

            // 8. Новый манифест: файлы пака (с прежним вкл/выкл) + пользовательское.
            // D71: записи пака маркируются from_pack=true — следующий diff
            // отличит их от пользовательских установок (from_pack=false).
            let manifest: Vec<ContentEntry> = c_planned
                .iter()
                .filter_map(|p| {
                    Some(ContentEntry {
                        kind: p.kind?,
                        file: p.file.clone(),
                        source: ContentSource::Modrinth,
                        project_id: p.project_id.clone(),
                        version_id: p.version_id.clone(),
                        sha1: p.sha1.clone(),
                        url: Some(p.url.clone()),
                        enabled: old_enabled_for(&fresh, &p.file).unwrap_or(true),
                        from_pack: true,
                    })
                })
                .chain(preserved_user_entries(&fresh, &c_planned))
                .collect();
            crate::instances::save_content_manifest(&c_paths, &c_id, &manifest)?;
            Ok((removed.len(), extracted))
        })
        .await
        .map_err(|e| LauncherError::Io(std::io::Error::other(format!("обновление пака: {e}"))))??
    };
    if removed_len > 0 {
        progress(format!("удаляю файлы старой версии: {removed_len}…"));
    }
    tracing::info!("обновление пака: overrides применены ({extracted} файлов)");

    // 9. instance.json: новая ссылка пака; сменился minecraft в dependencies —
    // и mc_version инстанса (instances::update — A28, под SAVE_LOCK).
    let new_mc = index
        .dependencies
        .get("minecraft")
        .cloned()
        .unwrap_or_else(|| inst.mc_version.clone());
    let mc_changed = new_mc != inst.mc_version;
    if mc_changed {
        progress(format!(
            "версия Minecraft: {new_mc} — переустанавливаю загрузчик…"
        ));
    }
    crate::instances::update(paths, instance_id, |i| {
        i.modpack = Some(new_ref.clone());
        if mc_changed {
            // D69-ревизия (P1): старый version JSON невалиден для новой MC —
            // без сброса prepare запускал бы СТАРУЮ цепочку с новыми модами.
            i.mc_version = new_mc.clone();
            i.version_id = None;
        }
    })?;
    if mc_changed {
        // Тот же путь, что смена версии инстанса: профиль Fabric/Quilt из
        // meta или headless-инсталлятор Forge/NeoForge под новую MC —
        // install_loader сам выставит и сохранит version_id.
        if let Some((name, ver)) = &loader {
            let mut fresh = crate::instances::load(paths, instance_id)?;
            fresh.loader = Some((*name).to_string());
            fresh.loader_version = Some((*ver).to_string());
            fresh.version_id = None;
            crate::instances::save(paths, &fresh)?;
            crate::instances::run::install_loader(
                paths,
                settings,
                http.clone(),
                &fresh,
                name,
                Some(ver),
            )
            .await?;
            tracing::info!(
                "обновление пака: загрузчик {name} {ver} переустановлен под {new_mc}"
            );
        }
    } else if let Some((name, ver)) = &loader {
        // Смена загрузчика без смены MC — вне рамок «обновления контента»:
        // честно фиксируем в логе, цепочку не трогаем.
        let differs = inst.loader.as_deref() != Some(*name)
            || inst.loader_version.as_deref() != Some(*ver);
        if differs {
            tracing::warn!(
                "новая версия пака требует загрузчик {name} {ver}, установлен {:?} {:?} — загрузчик не менялся",
                inst.loader,
                inst.loader_version
            );
        }
    }

    progress("готово".into());
    crate::instances::load(paths, instance_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBus;
    use std::io::Write as _;
    use std::sync::Mutex;

    fn test_paths(dir: &tempfile::TempDir) -> Paths {
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        paths
    }

    /// Тело «мода» (sha1 считается по этим байтам).
    const SAME_BYTES: &[u8] = b"same payload";
    const BUMP_OLD: &[u8] = b"bump old payload";
    const BUMP_NEW: &[u8] = b"bump new payload";
    const NEW_BYTES: &[u8] = b"brand new payload";

    fn entry(
        file: &str,
        project: Option<&str>,
        version: Option<&str>,
        sha1: Option<&str>,
        source: ContentSource,
        enabled: bool,
        from_pack: bool,
    ) -> ContentEntry {
        ContentEntry {
            kind: ContentKind::Mod,
            file: file.into(),
            source,
            project_id: project.map(String::from),
            version_id: version.map(String::from),
            sha1: sha1.map(String::from),
            url: None,
            enabled,
            from_pack,
        }
    }

    /// Минимальный modrinth.index.json с файлами files: [(путь, url, sha1, size)].
    fn index_json(mc: &str, files: &[(&str, &str, &str, usize)]) -> String {
        let files_json: Vec<String> = files
            .iter()
            .map(|(path, url, sha, size)| {
                format!(
                    r#"{{"path":"{path}","hashes":{{"sha1":"{sha}"}},
                        "downloads":["{url}"],"fileSize":{size}}}"#
                )
            })
            .collect();
        format!(
            r#"{{"formatVersion":1,"game":"minecraft","versionId":"pack-2.0.0",
                "name":"Тест-пак","files":[{}],
                "dependencies":{{"minecraft":"{mc}"}}}}"#,
            files_json.join(",")
        )
    }

    /// .mrpack в памяти: index + произвольные записи (overrides).
    fn pack_zip(index: &str, entries: &[(&str, &[u8])]) -> Vec<u8> {
        let buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(buf);
        let opts: zip::write::SimpleFileOptions = Default::default();
        zip.start_file("modrinth.index.json", opts).unwrap();
        zip.write_all(index.as_bytes()).unwrap();
        for (name, data) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    // ---------- newest_update (чистая функция, фикстуры строками) ----------

    /// Разбор ответа /version (фикстура строкой) и выбор свежей версии: есть
    /// новее → Some с усечённым до 600 символов ченджлогом; мы уже на свежей
    /// (первый id совпал) → None; пустой список → None.
    #[test]
    fn newest_update_from_fixture_and_changelog_cap() {
        let long_log = "ж".repeat(700);
        let fixture = format!(
            r#"[
                {{"id":"v-new","project_id":"p","version_number":"2.0.0",
                  "changelog":"{long_log}"}},
                {{"id":"v-mid","project_id":"p","version_number":"1.5.0"}},
                {{"id":"v-old","project_id":"p","version_number":"1.0.0"}}
            ]"#
        );
        let versions: Vec<Version> = serde_json::from_str(&fixture).unwrap();
        assert_eq!(versions.len(), 3);

        let upd = newest_update(&versions, "v-old").expect("обновление найдено");
        assert_eq!(upd.version_id, "v-new");
        assert_eq!(upd.version_number, "2.0.0");
        let log_len = upd
            .changelog
            .as_deref()
            .map(|c| c.chars().count())
            .expect("ченджлог переносится");
        assert_eq!(log_len, CHANGELOG_MAX_CHARS, "обрезка по символам");
        assert!(
            upd.changelog
                .as_deref()
                .is_some_and(|c| c.chars().all(|c| c == 'ж')),
            "ченджлог не искажён обрезкой"
        );

        // Уже на свежей версии — обновления нет (и не предлагается СТАРАЯ).
        assert!(newest_update(&versions, "v-new").is_none());
        // Пустой ответ — None.
        let empty: Vec<Version> = serde_json::from_str("[]").unwrap();
        assert!(newest_update(&empty, "v-old").is_none());
        // Контракт фронту camelCase.
        let json = serde_json::to_string(&upd).unwrap();
        assert!(json.contains("\"versionId\":\"v-new\""), "{json}");
        assert!(json.contains("\"versionNumber\":\"2.0.0\""), "{json}");
        assert!(!json.contains("version_id"), "{json}");
    }

    /// D70: установленная версия удалена с Modrinth (в списке её нет) — чек
    /// предлагает последнюю ОСТАВШУЮСЯ, даже если её номер МЕНЬШЕ
    /// установленного (валидный даунгрейд: автор откатил релиз). Контракт
    /// PackUpdateInfo прежний.
    #[test]
    fn newest_update_offers_downgrade_when_installed_version_gone() {
        let fixture = r#"[
            {"id":"v-1.5","project_id":"p","version_number":"1.5.0"},
            {"id":"v-1.0","project_id":"p","version_number":"1.0.0"}
        ]"#;
        let versions: Vec<Version> = serde_json::from_str(fixture).unwrap();
        // Установлена была «2.0.0», которой на Modrinth больше нет.
        let upd = newest_update(&versions, "v-2.0")
            .expect("после удаления версии должно быть предложено обновление");
        assert_eq!(upd.version_id, "v-1.5");
        assert_eq!(upd.version_number, "1.5.0");
        assert!(
            upd.version_number.as_str() < "2.0.0",
            "номер предложенной версии меньше установленного — это даунгрейд"
        );
    }

    /// D70: предикат gone — 404/410 (снят с Modrinth), прочие ошибки (403,
    /// сеть) остаются ошибками.
    #[test]
    fn gone_predicate_matches_only_404_410() {
        let mk = |status: u16| LauncherError::HttpStatus {
            status,
            url: "https://api.modrinth.com/v2/project/p/version".into(),
        };
        assert!(gone(&mk(404)));
        assert!(gone(&mk(410)));
        assert!(!gone(&mk(403)));
        assert!(!gone(&mk(500)));
        assert!(!gone(&LauncherError::OfflineMode("x".into())));
    }

    /// D70: реестр per-instance локов — один id выдаёт ОДИН и тот же лок,
    /// разные id — разные; критические секции двух потоков на одном локе
    /// не пересекаются.
    #[test]
    fn content_manifest_lock_serializes_sections() {
        let a1 = content_manifest_lock("d70-lock-a");
        let a2 = content_manifest_lock("d70-lock-a");
        assert!(Arc::ptr_eq(&a1, &a2), "один id — один и тот же лок");
        let b = content_manifest_lock("d70-lock-b");
        assert!(!Arc::ptr_eq(&a1, &b), "разные id — разные локи");

        use std::sync::atomic::{AtomicUsize, Ordering};
        static IN_SECTION: AtomicUsize = AtomicUsize::new(0);
        static VIOLATIONS: AtomicUsize = AtomicUsize::new(0);
        let shared = content_manifest_lock("d70-lock-shared");
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let lock = shared.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        let _g = lock.lock().unwrap();
                        // Секция занята ещё кем-то — пересечение, это баг.
                        if IN_SECTION.fetch_add(1, Ordering::SeqCst) != 0 {
                            VIOLATIONS.fetch_add(1, Ordering::SeqCst);
                        }
                        // Имитация критической секции read-modify-write.
                        std::thread::sleep(std::time::Duration::from_micros(200));
                        IN_SECTION.fetch_sub(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(
            VIOLATIONS.load(Ordering::SeqCst),
            0,
            "критические секции не должны пересекаться"
        );
    }

    // ---------- дифф манифестов (чистые функции) ----------

    fn planned(file: &str, sha1: &str) -> PlannedFile {
        PlannedFile {
            file: file.into(),
            url: format!("https://cdn.modrinth.com/data/x/versions/y/{file}"),
            sha1: Some(sha1.into()),
            sha512: None,
            size: 1,
            kind: Some(ContentKind::Mod),
            project_id: Some("proj".into()),
            version_id: Some("ver".into()),
        }
    }

    /// Дифф: выбывший из пака Modrinth-файл — на удаление, пользовательские
    /// (Local) сохраняются в любом случае, файлы нового пака не удаляются и
    /// не дублируются в «пользовательских».
    #[test]
    fn diff_removes_gone_keeps_user() {
        let old = vec![
            entry(
                "mods/a.jar",
                Some("p1"),
                Some("v1"),
                Some("aa"),
                ContentSource::Modrinth,
                true,
                true,
            ),
            entry(
                "mods/gone.jar",
                Some("p2"),
                Some("v2"),
                Some("bb"),
                ContentSource::Modrinth,
                true,
                true,
            ),
            entry(
                "mods/user.jar",
                None,
                None,
                None,
                ContentSource::Local,
                true,
                false,
            ),
            entry(
                "mods/user2.jar",
                Some("p9"),
                Some("v9"),
                Some("ff"),
                ContentSource::Local,
                false,
                false,
            ),
        ];
        let planned = vec![planned("mods/a.jar", "aa"), planned("mods/new.jar", "cc")];

        let removed = removed_pack_entries(&old, &planned);
        assert_eq!(
            removed.iter().map(|e| e.file.as_str()).collect::<Vec<_>>(),
            vec!["mods/gone.jar"],
            "удаляется только выбывший из пака"
        );
        let kept = preserved_user_entries(&old, &planned);
        assert_eq!(
            kept.iter().map(|e| e.file.as_str()).collect::<Vec<_>>(),
            vec!["mods/user.jar", "mods/user2.jar"],
            "пользовательские файлы не трогаем (включая выключенные)"
        );
    }

    /// D71: решение «пак или пользователь» принимается по from_pack, а не по
    /// source: (а) Modrinth-мод, поставленный пользователем через «Добавить
    /// моды» (from_pack=false), обновление пака СОХРАНЯЕТ, даже если новой
    /// версии пака он не касается; (б) выбывший член пака (from_pack=true,
    /// включая легаси-записи без поля — serde default true) УДАЛЯЕТСЯ.
    #[test]
    fn diff_respects_from_pack_not_source() {
        let old = vec![
            // Член пака, выбывший из новой версии — кандидат на удаление.
            entry(
                "mods/gone.jar",
                Some("p2"),
                Some("v2"),
                Some("bb"),
                ContentSource::Modrinth,
                true,
                true,
            ),
            // Тот же source (Modrinth), но ставил пользователь — сохраняется.
            entry(
                "mods/user-modrinth.jar",
                Some("p3"),
                Some("v3"),
                Some("cc"),
                ContentSource::Modrinth,
                true,
                false,
            ),
        ];
        let planned = vec![];

        let removed = removed_pack_entries(&old, &planned);
        assert_eq!(
            removed.iter().map(|e| e.file.as_str()).collect::<Vec<_>>(),
            vec!["mods/gone.jar"],
            "выбывший член пака (from_pack=true) удаляется"
        );
        let kept = preserved_user_entries(&old, &planned);
        assert_eq!(
            kept.iter().map(|e| e.file.as_str()).collect::<Vec<_>>(),
            vec!["mods/user-modrinth.jar"],
            "пользовательский Modrinth-мод (from_pack=false) сохраняется"
        );
    }

    /// D71: ни один компонент пути overrides не может быть «saves» без учёта
    /// регистра (Windows резолвит `Saves/…` в реальные миры), при этом
    /// одноимённые ПОДСТРОКИ в других именах проходить обязаны.
    #[test]
    fn touches_saves_is_case_insensitive_per_component() {
        for bad in [
            "Saves/world/level.dat",
            "SAVES",
            "saves",
            "saves/",
            "saves/w/level.dat",
            "mods/Saves/x",
            "config/SAVES/tweak.txt",
            "a/b/SaveS/c",
            "config\\SAVES\\x", // обратные слэши нормализуются
        ] {
            assert!(touches_saves(bad), "{bad:?} обязан быть отклонён");
        }
        for ok in [
            "savegames/x",
            "mods/saveslike.jar",
            "config/options.txt",
            "saveskeeper/world",
            "shaderpacks/pack.zip",
        ] {
            assert!(!touches_saves(ok), "{ok:?} обязан проходить");
        }
    }

    /// .disabled — «тот же файл»: выключенный файл пака с сошедшимся хэшем
    /// не перекачивается, задача для изменившегося файла пишет в ВЫКЛЮЧЕННОЕ
    /// имя (не оставляя на диске два экземпляра).
    #[test]
    fn build_tasks_skip_matching_and_respect_disabled_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let mc = dir.path();
        std::fs::create_dir_all(mc.join("mods")).unwrap();
        let sha_same = crate::util::fs::sha1_bytes(SAME_BYTES);
        let sha_bump = crate::util::fs::sha1_bytes(BUMP_NEW);

        // same.jar лежит включённым с тем же содержимым — задачи нет.
        std::fs::write(mc.join("mods").join("same.jar"), SAME_BYTES).unwrap();
        // dis.jar лежит ВЫКЛЮЧЕННЫМ (.disabled) с тем же содержимым — задачи нет.
        std::fs::write(mc.join("mods").join("dis.jar.disabled"), SAME_BYTES).unwrap();
        // bump.jar включён, но содержимое устарело — задача в .disabled-имя,
        // если в старом манифесте он был выключен.
        std::fs::write(mc.join("mods").join("bump.jar.disabled"), BUMP_OLD).unwrap();

        let old = vec![
            entry(
                "mods/same.jar",
                Some("p"),
                Some("v"),
                Some(&sha_same),
                ContentSource::Modrinth,
                true,
                true,
            ),
            entry(
                "mods/dis.jar",
                Some("p"),
                Some("v"),
                Some(&sha_same),
                ContentSource::Modrinth,
                false,
                true,
            ),
            entry(
                "mods/bump.jar",
                Some("p"),
                Some("v"),
                Some("old-sha"),
                ContentSource::Modrinth,
                false,
                true,
            ),
            entry(
                "mods/fresh.jar",
                Some("p"),
                Some("v"),
                Some("old-sha"),
                ContentSource::Modrinth,
                true,
                true,
            ),
        ];
        let planned = vec![
            planned("mods/same.jar", &sha_same),
            planned("mods/dis.jar", &sha_same),
            planned("mods/bump.jar", &sha_bump),
            planned("mods/fresh.jar", &sha_bump),
        ];

        let tasks = build_tasks(&planned, &old, mc, "g");
        assert_eq!(tasks.len(), 2, "{tasks:?}");
        let bump = tasks
            .iter()
            .find(|t| t.id == "packupdate:mods/bump.jar")
            .unwrap();
        assert!(
            bump.dest.ends_with("bump.jar.disabled"),
            "выключенный файл качается сразу в выключенное имя: {:?}",
            bump.dest
        );
        let fresh = tasks
            .iter()
            .find(|t| t.id == "packupdate:mods/fresh.jar")
            .unwrap();
        assert!(
            fresh.dest.ends_with("fresh.jar"),
            "новый файл — в обычное имя"
        );
        assert_eq!(fresh.sha1.as_deref(), Some(sha_bump.as_str()));
        assert_eq!(fresh.group, "g");
    }

    // ---------- check/apply: guard-ветки без сети ----------

    /// Инстанс без modpack — Ok(None) и ни одного похода в сеть.
    #[tokio::test]
    async fn check_without_modpack_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let inst = Instance::new("Обычный", "1.20.1");
        let client = HttpClient::new(None).unwrap();
        let upd = check(&paths, &client, &inst).await.unwrap();
        assert!(upd.is_none(), "без пака обновлений не бывает");
    }

    /// Офлайн — мгновенный OfflineMode (F16), а не сетевой таймаут.
    #[tokio::test]
    async fn check_offline_is_honest_error() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let mut inst = Instance::new("Пак", "1.20.1");
        inst.modpack = Some(ModpackRef {
            project_id: "pack-proj".into(),
            version_id: "v-old".into(),
            version_number: "1.0.0".into(),
        });
        let client = HttpClient::new(None).unwrap();
        client.set_offline(true);
        let err = check(&paths, &client, &inst).await.unwrap_err();
        assert!(matches!(err, LauncherError::OfflineMode(_)), "{err}");
    }

    /// apply на инстансе без пака — InvalidInput до всяких действий
    /// (бэкап не создаётся, сеть не дёргается).
    #[tokio::test]
    async fn apply_without_modpack_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let inst = Instance::new("Обычный", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let err = apply(&paths, &crate::settings::Settings::default(), engine, client, &inst.id, 3, &|_| {})
            .await
            .unwrap_err();
        assert_eq!(err.code(), "invalid_input", "{err}");
        assert!(
            paths
                .backups_dir()
                .read_dir()
                .map(|d| d.count())
                .unwrap_or(0)
                == 0,
            "бэкап до проверки пака не создаётся"
        );
    }

    /// Импорт .mrpack без project_id (пустая строка в ModpackRef) — обновление
    /// недоступно: источник версий неизвестен.
    #[tokio::test]
    async fn apply_without_project_id_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let mut inst = Instance::new("Импорт", "1.20.1");
        inst.modpack = Some(ModpackRef {
            project_id: String::new(),
            version_id: "x".into(),
            version_number: "импорт".into(),
        });
        crate::instances::save(&paths, &inst).unwrap();
        let client = Arc::new(HttpClient::new(None).unwrap());
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let err = apply(&paths, &crate::settings::Settings::default(), engine, client, &inst.id, 3, &|_| {})
            .await
            .unwrap_err();
        assert_eq!(err.code(), "invalid_input", "{err}");
    }

    // ---------- e2e: check + apply на локальном mock-сервере ----------

    /// Полный сценарий на ОДНОМ mock-сервере (TEST_API_BASE — глобальный на
    /// процесс, параллельные серверы мешали бы друг другу — A43): check находит
    /// свежую версию с усечённым ченджлогом, apply делает бэкап, скачивает
    /// .mrpack по sha1, удаляет выбывший мод, перекачивает изменившийся,
    /// НЕ трогает пользовательский и совпавший файлы, распаковывает overrides
    /// и пишет новую ссылку пака + mc_version в instance.json. Повторный check
    /// после обновления — None. Скачивания — только локальный axum (сети нет).
    #[tokio::test]
    async fn check_and_apply_end_to_end_on_mock() {
        let sha_same = crate::util::fs::sha1_bytes(SAME_BYTES);
        let sha_bump = crate::util::fs::sha1_bytes(BUMP_NEW);
        let sha_new = crate::util::fs::sha1_bytes(NEW_BYTES);
        let long_log = "ч".repeat(900);

        // .mrpack новой версии: same/dis не менялись, bump обновился, new — новый.
        let index = index_json(
            "1.20.4",
            &[
                (
                    "mods/same.jar",
                    "PLACEHOLDER/files/same.jar",
                    &sha_same,
                    SAME_BYTES.len(),
                ),
                (
                    "mods/dis.jar",
                    "PLACEHOLDER/files/dis.jar",
                    &sha_same,
                    SAME_BYTES.len(),
                ),
                (
                    "mods/bump.jar",
                    "PLACEHOLDER/files/bump.jar",
                    &sha_bump,
                    BUMP_NEW.len(),
                ),
                (
                    "mods/new.jar",
                    "PLACEHOLDER/files/new.jar",
                    &sha_new,
                    NEW_BYTES.len(),
                ),
            ],
        );
        // Ссылки файлов известны только после привязки слушателя — подставим
        // в index после spawn: собираем zip внутри обработчиков? Нет — zip
        // нужен заранее (sha1 архива в фикстуре версии). Поэтому: слушатель
        // биндим сначала, потом строим index/zip, потом поднимаем сервер.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let index = index.replace("PLACEHOLDER", &base);
        let zip_bytes = pack_zip(&index, &[("overrides/config/pack.toml", b"cfg".as_slice())]);
        let zip_sha1 = crate::util::fs::sha1_bytes(&zip_bytes);

        let versions_json = serde_json::json!([
            {
                "id": "v-new", "project_id": "pack-proj", "version_number": "2.0.0",
                "changelog": long_log,
                "files": [{
                    "hashes": {"sha1": zip_sha1},
                    "url": format!("{base}/pack.mrpack"),
                    "filename": "pack-2.0.0.mrpack", "primary": true,
                    "size": zip_bytes.len(),
                }],
            },
            {
                "id": "v-old", "project_id": "pack-proj", "version_number": "1.0.0",
                "files": [],
            },
        ]);
        let zip_body = zip_bytes.clone();
        let hits: Arc<std::sync::atomic::AtomicUsize> = Default::default();
        let pack_hits = hits.clone();
        let file_hits: Arc<Mutex<std::collections::HashMap<String, usize>>> =
            Arc::new(Mutex::new(Default::default()));
        let fh = file_hits.clone();
        let app = axum::Router::new()
            .route(
                "/v2/project/{id}/version",
                axum::routing::get(move |path: axum::extract::Path<String>| {
                    let v = versions_json.clone();
                    async move {
                        // D70: «proj-gone» — пак, снятый с Modrinth целиком
                        // (404): тихая проверка обязана дать Ok(None), а не
                        // ошибку при каждом запуске.
                        if path.as_str() == "proj-gone" {
                            Err(axum::http::StatusCode::NOT_FOUND)
                        } else {
                            Ok(axum::Json(v))
                        }
                    }
                }),
            )
            .route(
                "/pack.mrpack",
                axum::routing::get(move || {
                    let b = zip_body.clone();
                    pack_hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    async move { axum::body::Body::from(b) }
                }),
            )
            .route(
                "/files/{name}",
                axum::routing::get(move |path: axum::extract::Path<String>| {
                    let fh = fh.clone();
                    async move {
                        *fh.lock().unwrap().entry(path.clone()).or_default() += 1;
                        let body: &[u8] = match path.as_str() {
                            "bump.jar" => BUMP_NEW,
                            "new.jar" => NEW_BYTES,
                            _ => SAME_BYTES,
                        };
                        axum::body::Body::from(body.to_vec())
                    }
                }),
            );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        // Единственная точка подмены базового URL (форма prod-константы).
        TEST_API_BASE.set(format!("{base}/v2")).ok();

        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);

        // Старый инстанс «после установки пака 1.0.0»: выбывающий gone,
        // пользовательский keep, НЕ-паковый Modrinth-мод keepmr (D71: поставлен
        // через «Добавить моды», from_pack=false), неменяющиеся same/dis
        // (один выключен), обновляющийся bump.
        let mut inst = Instance::new("Пак", "1.20.1");
        inst.modpack = Some(ModpackRef {
            project_id: "pack-proj".into(),
            version_id: "v-old".into(),
            version_number: "1.0.0".into(),
        });
        crate::instances::save(&paths, &inst).unwrap();
        let mc = crate::instances::minecraft_dir(&crate::instances::instance_dir(&paths, &inst.id));
        std::fs::create_dir_all(mc.join("mods")).unwrap();
        std::fs::write(mc.join("mods").join("gone.jar"), b"gone").unwrap();
        std::fs::write(mc.join("mods").join("keep.jar"), b"user").unwrap();
        std::fs::write(mc.join("mods").join("keepmr.jar"), b"user-modrinth").unwrap();
        std::fs::write(mc.join("mods").join("same.jar"), SAME_BYTES).unwrap();
        std::fs::write(mc.join("mods").join("dis.jar.disabled"), SAME_BYTES).unwrap();
        std::fs::write(mc.join("mods").join("bump.jar"), BUMP_OLD).unwrap();
        crate::instances::save_content_manifest(
            &paths,
            &inst.id,
            &[
                entry(
                    "mods/gone.jar",
                    Some("p1"),
                    Some("u1"),
                    Some("gg"),
                    ContentSource::Modrinth,
                    true,
                    true,
                ),
                entry(
                    "mods/keep.jar",
                    None,
                    None,
                    None,
                    ContentSource::Local,
                    true,
                    false,
                ),
                entry(
                    "mods/keepmr.jar",
                    Some("p5"),
                    Some("u5"),
                    Some("mm"),
                    ContentSource::Modrinth,
                    true,
                    false,
                ),
                entry(
                    "mods/same.jar",
                    Some("p1"),
                    Some("u1"),
                    Some(&sha_same),
                    ContentSource::Modrinth,
                    true,
                    true,
                ),
                entry(
                    "mods/dis.jar",
                    Some("p1"),
                    Some("u1"),
                    Some(&sha_same),
                    ContentSource::Modrinth,
                    false,
                    true,
                ),
                entry(
                    "mods/bump.jar",
                    Some("p1"),
                    Some("u1"),
                    Some("bb"),
                    ContentSource::Modrinth,
                    true,
                    true,
                ),
            ],
        )
        .unwrap();

        let client = Arc::new(HttpClient::new(None).unwrap());

        // check до обновления: свежая версия найдена, ченджлог обрезан.
        let upd = check(&paths, &client, &inst)
            .await
            .unwrap()
            .expect("обновление есть");
        assert_eq!(upd.version_id, "v-new");
        assert_eq!(upd.version_number, "2.0.0");
        assert_eq!(
            upd.changelog.expect("ченджлог есть").chars().count(),
            CHANGELOG_MAX_CHARS,
            "900 символов → 600"
        );

        // apply: бэкап → скачивания → дифф → overrides → новая ссылка.
        let engine = DownloadEngine::new(client.clone(), 2, EventBus::default());
        let logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let lg = logs.clone();
        let collector = move |m: String| lg.lock().unwrap().push(m);
        let inst2 = apply(&paths, &crate::settings::Settings::default(), engine, client.clone(), &inst.id, 3, &collector)
            .await
            .expect("обновление прошло");

        assert_eq!(
            inst2.mc_version, "1.20.4",
            "dependencies.minecraft → mc_version"
        );
        let pack = inst2.modpack.as_ref().unwrap();
        assert_eq!(pack.version_id, "v-new");
        assert_eq!(pack.version_number, "2.0.0");
        assert_eq!(pack.project_id, "pack-proj");
        // instance.json на диске тоже обновлён.
        let on_disk = crate::instances::load(&paths, &inst.id).unwrap();
        assert_eq!(on_disk.modpack.as_ref().unwrap().version_id, "v-new");
        assert_eq!(on_disk.mc_version, "1.20.4");

        // Бэкап создан (архив в backups/).
        let backups = std::fs::read_dir(crate::util::fs::long_path(&paths.backups_dir()))
            .unwrap()
            .count();
        assert!(backups >= 1, "бэкап инстанса обязан быть создан");

        // Дифф: выбывший удалён, пользовательский нетронут, неменяющиеся не
        // качались, изменившийся перекачан, новый появился.
        assert!(
            !mc.join("mods").join("gone.jar").exists(),
            "выбывший удалён"
        );
        assert_eq!(
            std::fs::read(mc.join("mods").join("keep.jar")).unwrap(),
            b"user",
            "пользовательский файл не тронут"
        );
        assert_eq!(
            std::fs::read(mc.join("mods").join("keepmr.jar")).unwrap(),
            b"user-modrinth",
            "D71: Modrinth-мод пользователя (from_pack=false) не тронут"
        );
        assert_eq!(
            std::fs::read(mc.join("mods").join("same.jar")).unwrap(),
            SAME_BYTES,
            "совпавший файл не перекачивался"
        );
        assert_eq!(
            *file_hits.lock().unwrap().get("same.jar").unwrap_or(&0),
            0,
            "совпавший файл — ноль запросов"
        );
        assert_eq!(
            *file_hits.lock().unwrap().get("dis.jar").unwrap_or(&0),
            0,
            "выключенный совпавший файл — ноль запросов"
        );
        assert!(
            mc.join("mods").join("dis.jar.disabled").exists(),
            "выключенный остался выключенным"
        );
        assert!(
            !mc.join("mods").join("dis.jar").exists(),
            "дубля без .disabled нет"
        );
        assert_eq!(
            std::fs::read(mc.join("mods").join("bump.jar")).unwrap(),
            BUMP_NEW,
            "изменившийся перекачан"
        );
        assert_eq!(
            std::fs::read(mc.join("mods").join("new.jar")).unwrap(),
            NEW_BYTES
        );
        assert_eq!(
            std::fs::read(mc.join("config").join("pack.toml")).unwrap(),
            b"cfg",
            "overrides распакованы"
        );
        assert_eq!(
            std::fs::read_dir(crate::util::fs::long_path(&mc.join("saves")))
                .map(|d| d.count())
                .unwrap_or(0),
            0,
            "saves из overrides не появляется"
        );

        // Манифест: 4 файла пака + пользовательские (Local и Modrinth-мод
        // from_pack=false), gone исчез.
        let manifest = crate::instances::load_content_manifest(&paths, &inst.id);
        let mut files: Vec<&str> = manifest.iter().map(|e| e.file.as_str()).collect();
        files.sort();
        assert_eq!(
            files,
            vec![
                "mods/bump.jar",
                "mods/dis.jar",
                "mods/keep.jar",
                "mods/keepmr.jar",
                "mods/new.jar",
                "mods/same.jar"
            ],
            "{manifest:?}"
        );
        let keep = manifest.iter().find(|e| e.file == "mods/keep.jar").unwrap();
        assert_eq!(
            keep.source,
            ContentSource::Local,
            "пользовательская запись сохранена"
        );
        // D71: не-паковый Modrinth-мод остался в манифесте как пользовательский,
        // записи пака промаркированы from_pack=true.
        let keepmr = manifest.iter().find(|e| e.file == "mods/keepmr.jar").unwrap();
        assert_eq!(keepmr.source, ContentSource::Modrinth);
        assert!(!keepmr.from_pack, "fromPack=false перенесён в новый манифест");
        for f in ["mods/bump.jar", "mods/same.jar", "mods/new.jar", "mods/dis.jar"] {
            let e = manifest.iter().find(|e| e.file == f).unwrap();
            assert!(e.from_pack, "член пака {f} промаркирован from_pack=true");
        }
        let dis = manifest.iter().find(|e| e.file == "mods/dis.jar").unwrap();
        assert!(
            !dis.enabled,
            "выключенное состояние переносится в новый манифест"
        );
        let bump = manifest.iter().find(|e| e.file == "mods/bump.jar").unwrap();
        assert_eq!(
            bump.sha1.as_deref(),
            Some(sha_bump.as_str()),
            "новый sha1 записан"
        );

        // Статусы шагов шли в progress-колбэк.
        {
            let log = logs.lock().unwrap();
            assert!(
                log.iter().any(|m| m.contains("бэкап")),
                "лог шагов: {log:?}"
            );
            assert!(log.last().is_some_and(|m| m == "готово"), "{log:?}");
        }

        // Повторный check после обновления: мы уже на свежей — None.
        let again = check(&paths, &client, &inst2).await.unwrap();
        assert!(again.is_none(), "после обновления баннер должен исчезнуть");
        // Архив пака скачивался ровно один раз.
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);

        // D70: установленная версия удалена с Modrinth (в списке её нет) —
        // чек предлагает последнюю ОСТАВШУЮСЯ (apply применяет как даунгрейд).
        let mut ghost = Instance::new("Пак-призрак", "1.20.1");
        ghost.modpack = Some(ModpackRef {
            project_id: "pack-proj".into(),
            version_id: "v-deleted".into(),
            version_number: "9.9.9".into(),
        });
        let ghost_upd = check(&paths, &client, &ghost)
            .await
            .unwrap()
            .expect("оставшаяся версия предлагается");
        assert_eq!(ghost_upd.version_id, "v-new");
        assert_eq!(ghost_upd.version_number, "2.0.0");

        // D70: пак снят с Modrinth целиком (404 от /project/{id}/version) —
        // тихая проверка не падает ошибкой, а честно отвечает «обновлений нет».
        let mut gone_inst = Instance::new("Пак-удалённый", "1.20.1");
        gone_inst.modpack = Some(ModpackRef {
            project_id: "proj-gone".into(),
            version_id: "v-old".into(),
            version_number: "1.0.0".into(),
        });
        let gone_upd = check(&paths, &client, &gone_inst).await.unwrap();
        assert!(
            gone_upd.is_none(),
            "404 проекта — не ошибка, а «обновлений нет»: {gone_upd:?}"
        );
    }
}
