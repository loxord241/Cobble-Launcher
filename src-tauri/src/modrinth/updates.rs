//! Обновления контента (спека §6.6): проверка совместимых свежих версий,
//! батч-обновление с ченджлогами и откатом (старый jar → .trash/).

use crate::errors::{LauncherError, Result};
use crate::instances::{ContentKind, ContentSource, Instance};
use crate::modrinth::api;
use crate::net::download::{DownloadEngine, DownloadTask};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use serde::Serialize;
use std::sync::Arc;

/// Найденное обновление одного контента.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub file: String,
    pub project_id: String,
    pub current_version_id: String,
    pub latest_version_id: String,
    pub latest_version_number: String,
    pub changelog: Option<String>,
}

/// Запись манифеста, для которой осмысленна проверка обновлений (A25: собираем
/// кандидатов заранее, чтобы дважды не перебирать манифест и не путать группы).
struct Candidate {
    file: String,
    project_id: String,
    current: String,
    sha1: Option<String>,
    kind: ContentKind,
}

fn candidates(manifest: &[crate::instances::ContentEntry]) -> Vec<Candidate> {
    manifest
        .iter()
        .filter(|e| e.source == ContentSource::Modrinth)
        .filter_map(|e| {
            Some(Candidate {
                file: e.file.clone(),
                project_id: e.project_id.clone()?,
                current: e.version_id.clone()?,
                sha1: e.sha1.clone(),
                kind: e.kind,
            })
        })
        .collect()
}

/// Базовый URL Modrinth API: в прод-сборке — константа, в тестах — локальный
/// mock-сервер (внешней сети в тестах нет).
fn api_base() -> String {
    #[cfg(test)]
    if let Some(base) = TEST_API_BASE.get() {
        return base.clone();
    }
    crate::modrinth::api::API_BASE.to_string()
}

#[cfg(test)]
static TEST_API_BASE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Батч-эндпоинт Modrinth: `hash → версия` ОДНИМ запросом (A25) вместо
/// N последовательных `/project/{id}/version`. Фильтры (loaders/game_versions)
/// применяет сервер — так же, как локальный `pick_compatible_version`.
/// Хэши, которых нет в ответе, вызывающий добирает старым путём.
async fn batch_latest_versions(
    client: &HttpClient,
    hashes: &[String],
    loader: Option<&str>,
    mc_version: Option<&str>,
) -> Result<std::collections::HashMap<String, api::Version>> {
    // F16: офлайн-режим — мгновенный честный отказ вместо сетевого таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode("проверка обновлений модов".into()));
    }
    let mut body = serde_json::Map::new();
    body.insert("hashes".to_string(), serde_json::json!(hashes));
    body.insert("algorithm".to_string(), serde_json::json!("sha1"));
    if let Some(l) = loader {
        body.insert("loaders".to_string(), serde_json::json!([l]));
    }
    if let Some(mc) = mc_version {
        body.insert("game_versions".to_string(), serde_json::json!([mc]));
    }
    let url = format!("{}/version_files/update", api_base());
    let resp = client
        .raw()
        .post(&url)
        .json(&serde_json::Value::Object(body))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "HTTP {} для {url}",
            resp.status()
        )));
    }
    // Значения могут прийти как null (файл не найден) — их отфильтровываем.
    let raw: std::collections::HashMap<String, Option<api::Version>> = resp.json().await?;
    Ok(raw.into_iter().filter_map(|(k, v)| Some((k, v?))).collect())
}

/// Проверить обновления для всех записей манифеста с source=modrinth.
///
/// A25: вместо N последовательных запросов идём батчем `/version_files/update`
/// — одна пара запросов на всю проверку (моды отдельно: у них фильтр по
/// загрузчику, остальной контент без фильтра). Если батч упал или не покрыл
/// запись (нет sha1 / хэша нет в ответе) — для этой записи работает прежний
/// поштучный путь, контракт ответа для UI не меняется.
pub async fn check_updates(
    paths: &Paths,
    client: &HttpClient,
    inst: &Instance,
) -> Result<Vec<UpdateCheck>> {
    let manifest = crate::instances::load_content_manifest(paths, &inst.id);
    // Фильтр по загрузчику применяется только к модам (ресурспаки/шейдеры
    // загрузчика не имеют) — как и раньше в поштучном пути.
    let loader = inst.loader.clone();
    let all = candidates(&manifest);

    let mut mod_hashes: Vec<String> = Vec::new();
    let mut other_hashes: Vec<String> = Vec::new();
    for c in &all {
        if let Some(sha) = &c.sha1 {
            if c.kind == ContentKind::Mod {
                mod_hashes.push(sha.clone());
            } else {
                other_hashes.push(sha.clone());
            }
        }
    }

    let mut by_hash: std::collections::HashMap<String, api::Version> = Default::default();
    if !mod_hashes.is_empty() {
        match batch_latest_versions(client, &mod_hashes, loader.as_deref(), Some(&inst.mc_version))
            .await
        {
            Ok(map) => by_hash.extend(map),
            Err(e) => tracing::warn!("батч-проверка модов не удалась ({e}) — проверяю поштучно"),
        }
    }
    if !other_hashes.is_empty() {
        match batch_latest_versions(client, &other_hashes, None, Some(&inst.mc_version)).await {
            Ok(map) => by_hash.extend(map),
            Err(e) => tracing::warn!("батч-проверка контента не удалась ({e}) — проверяю поштучно"),
        }
    }

    let mut out = Vec::new();
    for c in &all {
        let latest = match c.sha1.as_ref().and_then(|h| by_hash.get(h)).cloned() {
            Some(v) => Some(v),
            None => {
                // Батч эту запись не покрыл — прежний путь (один запрос на проект).
                let versions = api::project_versions(client, &c.project_id).await?;
                let loader_filter = if c.kind == ContentKind::Mod {
                    loader.as_deref()
                } else {
                    None
                };
                crate::modrinth::install::pick_compatible_version(
                    &versions,
                    &inst.mc_version,
                    loader_filter,
                )
                .cloned()
            }
        };
        if let Some(latest) = latest {
            if latest.id != c.current {
                out.push(UpdateCheck {
                    file: c.file.clone(),
                    project_id: c.project_id.clone(),
                    current_version_id: c.current.clone(),
                    latest_version_id: latest.id.clone(),
                    latest_version_number: latest.version_number.clone(),
                    changelog: latest.changelog.clone(),
                });
            }
        }
    }
    Ok(out)
}

/// Батч-обновление: новая версия скачивается, старый файл → `.trash/`,
/// манифест обновляется. Возвращает число обновлённых.
pub async fn update_all(
    paths: &Paths,
    client: Arc<HttpClient>,
    engine: Arc<DownloadEngine>,
    inst: &Instance,
) -> Result<usize> {
    let checks = check_updates(paths, &client, inst).await?;
    let mut manifest = crate::instances::load_content_manifest(paths, &inst.id);
    let game_dir =
        crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
    let trash = game_dir.join(".trash");
    std::fs::create_dir_all(crate::util::fs::long_path(&trash))?;
    let mut updated = 0;

    // A25: версии, которые нужно скачать, резолвим ОДНИМ батч-запросом по хэшам
    // старых файлов (раньше на каждый check уходил отдельный /project/{id}/version).
    // Если батч не вернул нужную версию — для этой записи используем прежний путь.
    let check_hashes: Vec<String> = checks
        .iter()
        .filter_map(|c| {
            manifest
                .iter()
                .find(|e| e.file == c.file)
                .and_then(|e| e.sha1.clone())
        })
        .collect();
    let by_hash: std::collections::HashMap<String, api::Version> = if check_hashes.is_empty() {
        Default::default()
    } else {
        batch_latest_versions(&client, &check_hashes, None, Some(&inst.mc_version))
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("батч-обновление: {e} — резолвлю версии поштучно");
                Default::default()
            })
    };

    // PERF#2 (аудит 2026-10-03): двухфазная схема. Фаза 1 — план: резолв
    // версий + задачи скачивания; движок получает ВСЕ задачи сразу и качает
    // с полным параллелизмом (раньше run() звался на каждый файл — движок
    // вырождался в последовательную скачку). Фаза 2 — свопы старых файлов и
    // ОДНА запись манифеста (раньше — N×atomic_write с fsync).
    struct PlannedUpdate {
        task: DownloadTask,
        temp: std::path::PathBuf,
        dest: std::path::PathBuf,
        old_path: std::path::PathBuf,
        filename: String,
        check_file: String,
        version_id: String,
        new_file_field: String,
        new_sha1: Option<String>,
    }

    let mut plan: Vec<PlannedUpdate> = Vec::new();

    for check in &checks {
        let from_batch = manifest
            .iter()
            .find(|e| e.file == check.file)
            .and_then(|e| e.sha1.as_ref())
            .and_then(|h| by_hash.get(h))
            .filter(|v| v.id == check.latest_version_id)
            .cloned();
        let latest = match from_batch {
            Some(v) => v,
            None => {
                let versions = api::project_versions(&client, &check.project_id).await?;
                match versions.into_iter().find(|v| v.id == check.latest_version_id) {
                    Some(v) => v,
                    None => continue,
                }
            }
        };
        let Some(file) = latest.files.iter().find(|f| f.primary).or_else(|| latest.files.first())
        else {
            continue;
        };
        let Some(old_entry) = manifest.iter().find(|e| e.file == check.file) else {
            continue;
        };

        // Новый файл качаем во ВРЕМЕННОЕ имя: старый мод уезжает в корзину
        // ТОЛЬКО после успешной скачки всей пачки — сбой сети/отмена оставляли
        // бы инстанс сломанным.
        let dir = crate::modrinth::install::dir_for_kind(old_entry.kind);
        let dest = game_dir.join(dir).join(&file.filename);
        let temp = dest.with_file_name(format!(".update-{}", file.filename));
        plan.push(PlannedUpdate {
            task: DownloadTask {
                id: format!("update:{}", file.filename),
                url: file.url.clone(),
                dest: temp.clone(),
                sha1: file.hashes.get("sha1").cloned(),
                sha512: file.hashes.get("sha512").cloned(),
                size: Some(file.size),
                group: format!("instance:{}", inst.id),
                priority: 25,
            },
            temp,
            dest,
            old_path: game_dir.join(&old_entry.file),
            filename: file.filename.clone(),
            check_file: check.file.clone(),
            version_id: latest.id.clone(),
            new_file_field: format!("{dir}/{}", file.filename),
            new_sha1: file.hashes.get("sha1").cloned(),
        });
    }

    if plan.is_empty() {
        return Ok(0);
    }

    engine.add_tasks(plan.iter().map(|p| p.task.clone()).collect());
    if let Err(e) = engine.run().await {
        for p in &plan {
            crate::util::fs::remove_file_ignore(&p.temp);
        }
        return Err(e);
    }
    let st = engine.queue_state();
    if st.failed > 0 {
        for p in &plan {
            crate::util::fs::remove_file_ignore(&p.temp);
        }
        return Err(LauncherError::network(format!(
            "обновление не скачалось: {}/{} файлов",
            st.failed,
            plan.len()
        )));
    }

    // Всё скачалось: свопы (старый → корзина, temp → место) и одна запись
    // манифеста. Сбой свопа (антивирус держит файл) — честная ошибка,
    // манифест ещё не тронут.
    for p in &plan {
        swap_updated_file(&p.old_path, &p.temp, &p.dest, &trash, &p.filename)?;
        if let Some(e) = manifest.iter_mut().find(|e| e.file == p.check_file) {
            e.file = p.new_file_field.clone();
            e.version_id = Some(p.version_id.clone());
            e.sha1 = p.new_sha1.clone();
        }
        updated += 1;
    }
    crate::instances::save_content_manifest(paths, &inst.id, &manifest)?;
    Ok(updated)
}

fn chrono_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

/// ENV-1: перенос скачанного temp на место старого файла с корзиной.
/// Старый уезжает в `.trash` (для ручного отката), temp занимает его место;
/// если финальный перенос сорвался (свежий .jar придержал антивирус) — старый
/// возвращается из корзины, temp сносится, запись манифеста не тронута:
/// инстанс остаётся рабочим. Если не вышел даже возврат — честно называем
/// путь в корзине: файл не потерян, его можно вернуть вручную.
fn swap_updated_file(
    old_path: &std::path::Path,
    temp: &std::path::Path,
    dest: &std::path::Path,
    trash: &std::path::Path,
    label: &str,
) -> Result<()> {
    if old_path.exists() {
        std::fs::create_dir_all(crate::util::fs::long_path(trash))?;
    }
    let mut trashed: Option<std::path::PathBuf> = None;
    if old_path.exists() {
        let name = old_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("old-{label}"));
        let in_trash = trash.join(format!("{}-{}", chrono_suffix(), name));
        if crate::util::fs::rename_with_retry(old_path, &in_trash).is_ok() {
            trashed = Some(in_trash);
        }
    }
    if let Err(e) = crate::util::fs::rename_with_retry(temp, dest) {
        crate::util::fs::remove_file_ignore(temp);
        if let Some(in_trash) = trashed {
            if let Err(r) = crate::util::fs::rename_with_retry(&in_trash, old_path) {
                return Err(LauncherError::Internal(format!(
                    "обновление {label} сорвалось ({e}); старый файл ждёт в корзине: {} ({r})",
                    in_trash.display()
                )));
            }
        }
        return Err(e);
    }
    Ok(())
}

/// Самозалечивающийся манифест (legacy-модпаки): записи без `project_id`
/// (установлены до появления поля) получают его батч-запросом sha1 → версия.
/// Возвращает число дополненных записей. Найденные вне Modrinth хэши
/// молча пропускаются (локальные моды остаются без проекта).
pub async fn backfill_project_ids(
    paths: &Paths,
    client: &HttpClient,
    inst: &Instance,
) -> Result<usize> {
    let mut manifest = crate::instances::load_content_manifest(paths, &inst.id);
    let need: Vec<String> = manifest
        .iter()
        .filter(|e| {
            e.source == ContentSource::Modrinth && e.project_id.is_none() && e.sha1.is_some()
        })
        .filter_map(|e| e.sha1.clone())
        .collect();
    if need.is_empty() {
        return Ok(0);
    }
    // Без фильтров загрузчика/версии: задача — только опознать проект.
    let by_hash = batch_latest_versions(client, &need, None, None)
        .await
        .unwrap_or_default();
    let mut filled = 0usize;
    for e in manifest.iter_mut() {
        if e.project_id.is_some() || e.source != ContentSource::Modrinth {
            continue;
        }
        if let Some(sha) = &e.sha1 {
            if let Some(v) = by_hash.get(sha) {
                e.project_id = Some(v.project_id.clone());
                filled += 1;
            }
        }
    }
    if filled > 0 {
        crate::instances::save_content_manifest(paths, &inst.id, &manifest)?;
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{ContentEntry, ContentKind, ContentSource};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// Регресс ENV-1 (счастливый путь): старый мод уезжает в .trash, temp
    /// занимает его место, ничего лишнего не остаётся.
    #[test]
    fn update_swap_happy_path_trashes_old() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("mods");
        std::fs::create_dir_all(&game).unwrap();
        let old = game.join("old.jar");
        std::fs::write(&old, b"old").unwrap();
        let temp = game.join(".update-new.jar");
        std::fs::write(&temp, b"new").unwrap();
        let dest = game.join("new.jar");
        let trash = game.join(".trash");

        swap_updated_file(&old, &temp, &dest, &trash, "new.jar").unwrap();

        assert!(!old.exists(), "старого файла на месте быть не должно");
        assert!(!temp.exists(), "temp должен переехать на место dest");
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        let trashed: Vec<_> = std::fs::read_dir(&trash)
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(trashed.len(), 1, "старый мод должен лежать в корзине");
        assert_eq!(std::fs::read(trashed[0].path()).unwrap(), b"old");
    }

    /// Регресс ENV-1 (антивирус придержал новый файл): temp снесён, старый
    /// вернулся из корзины — инстанс остаётся рабочим, файлы не теряются.
    #[cfg(windows)]
    #[test]
    fn update_swap_blocked_dest_restores_old() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("mods");
        std::fs::create_dir_all(&game).unwrap();
        let old = game.join("old.jar");
        std::fs::write(&old, b"old").unwrap();
        let temp = game.join(".update-new.jar");
        std::fs::write(&temp, b"new").unwrap();
        let dest = game.join("new.jar");
        let trash = game.join(".trash");

        // Держим dest занятым без share-доступа (как антивирус): rename
        // выдаёт os error 32, после исчерпания ретраев (~850 мс) должен
        // сработать возврат старого из корзины.
        use std::os::windows::fs::OpenOptionsExt as _;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .share_mode(0)
            .open(&dest)
            .unwrap();
        let res = swap_updated_file(&old, &temp, &dest, &trash, "new.jar");
        drop(lock);

        assert!(res.is_err());
        assert!(dest.exists(), "заблокированный dest остаётся (пустой)");
        assert!(old.exists(), "старый мод обязан вернуться из корзины");
        assert_eq!(std::fs::read(&old).unwrap(), b"old");
        assert!(!temp.exists(), "temp сносится при сбое");
    }

    fn test_paths(dir: &tempfile::TempDir) -> Paths {
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        paths
    }

    async fn spawn_axum(app: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn version_json(id: &str, number: &str, project_id: &str, sha1: &str, changelog: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "project_id": project_id,
            "version_number": number,
            "changelog": changelog,
            "game_versions": ["1.21.1"],
            "loaders": ["fabric"],
            "dependencies": [],
            "files": [{
                "hashes": {"sha1": sha1},
                "url": "https://cdn.modrinth.com/data/x/versions/y/x.jar",
                "filename": "x.jar",
                "primary": true,
                "size": 10
            }]
        })
    }

    fn entry(kind: ContentKind, file: &str, project: &str, version: &str, sha1: &str) -> ContentEntry {
        ContentEntry {
            kind,
            file: file.into(),
            source: ContentSource::Modrinth,
            project_id: Some(project.into()),
            version_id: Some(version.into()),
            sha1: Some(sha1.into()),
            url: None,
            enabled: true,
        }
    }

    /// A25-регресс: проверка обновлений идёт ОДНИМ батч-запросом
    /// (`/version_files/update`) вместо N поштучных, а контракт ответа прежний.
    /// Один тест на всю проверку: `TEST_API_BASE` — глобальный на процесс,
    /// параллельные тесты с разными mock-серверами мешали бы друг другу (A43-принцип).
    #[tokio::test]
    async fn check_updates_uses_single_batch_request() {
        let batch_calls = Arc::new(AtomicUsize::new(0));
        let project_calls = Arc::new(AtomicUsize::new(0));
        let seen_body: Arc<Mutex<serde_json::Value>> = Arc::new(Mutex::new(serde_json::Value::Null));
        let (batch_srv, project_srv, body_srv) =
            (batch_calls.clone(), project_calls.clone(), seen_body.clone());
        let app = axum::Router::new()
            .route(
                "/v2/version_files/update",
                axum::routing::post(move |body: String| {
                    let (calls, seen) = (batch_srv.clone(), body_srv.clone());
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        *seen.lock().unwrap() =
                            serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
                        axum::Json(serde_json::json!({
                            "HASH-A": version_json("new-1", "2.0.0", "proj-a", "HASH-A", "починили"),
                            "HASH-B": version_json("old-2", "1.0.0", "proj-b", "HASH-B", ""),
                        }))
                    }
                }),
            )
            // Поштучный эндпоинт: если код в него пойдёт — счётчик это покажет.
            .route(
                "/v2/project/{id}/version",
                axum::routing::get(move || {
                    let c = project_srv.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        axum::Json(serde_json::json!([]))
                    }
                }),
            );
        let base = spawn_axum(app).await;
        // Единственная точка подмены базового URL: mock подставляется как
        // `.../v2` (форма prod-константы), внешней сети в тестах нет.
        TEST_API_BASE.set(format!("{base}/v2")).ok();

        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let mut inst = Instance::new("Тест", "1.21.1");
        inst.loader = Some("fabric".into());
        crate::instances::save_content_manifest(
            &paths,
            &inst.id,
            &[
                // Есть апдейт (батч отдаёт new-1).
                entry(ContentKind::Mod, "mods/a.jar", "proj-a", "old-1", "HASH-A"),
                // Хэш в батче есть, но версия та же — обновления нет.
                entry(ContentKind::Mod, "mods/b.jar", "proj-b", "old-2", "HASH-B"),
            ],
        )
        .unwrap();

        let client = HttpClient::new(None).unwrap();
        let updates = check_updates(&paths, &client, &inst).await.unwrap();

        assert_eq!(
            batch_calls.load(Ordering::SeqCst),
            1,
            "должен быть ровно один батч-запрос (не N поштучных)"
        );
        assert_eq!(
            project_calls.load(Ordering::SeqCst),
            0,
            "покрытые батчем записи не должны ходить поштучно"
        );
        let body = seen_body.lock().unwrap().clone();
        assert_eq!(body["algorithm"], "sha1");
        assert_eq!(body["loaders"], serde_json::json!(["fabric"]));
        assert_eq!(body["game_versions"], serde_json::json!(["1.21.1"]));
        let mut hashes: Vec<String> = body["hashes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        hashes.sort();
        assert_eq!(hashes, vec!["HASH-A".to_string(), "HASH-B".to_string()]);

        assert_eq!(updates.len(), 1, "обновился только a.jar: {updates:?}");
        assert_eq!(updates[0].file, "mods/a.jar");
        assert_eq!(updates[0].project_id, "proj-a");
        assert_eq!(updates[0].current_version_id, "old-1");
        assert_eq!(updates[0].latest_version_id, "new-1");
        assert_eq!(updates[0].latest_version_number, "2.0.0");
        assert_eq!(updates[0].changelog.as_deref(), Some("починили"));
    }
}

