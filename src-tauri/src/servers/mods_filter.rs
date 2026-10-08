//! Фильтр модов при создании сервера из инстанса — главная ценность фичи:
//! клиент-моды (шейдеры-загрузчики, HUD, миникарты) на сервер не попадают,
//! серверные и двусторонние копируются.
//!
//! Источник истины — content-manifest инстанса (kind=mod, enabled):
//! - есть project_id → батч-запрос Modrinth `/projects?ids=[..]` (чанки ≤100):
//!   `server_side=="unsupported"` → клиент-мод, НЕ копируем;
//!   `client_side=="unsupported"` → серверный, копируем обязательно;
//! - нет project_id → `environment` из fabric.mod.json внутри jar:
//!   `"client"` → пропуск, иначе копия «как есть»;
//! - офлайн/ошибка API → весь create НЕ валим: записи с project_id копируются
//!   как непроверенные (лучше лишний мод, чем молча потерянный серверный).

use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use crate::util::fs::long_path;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Решение по одному моду (чистая классификация — тестируется без сети).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModDisposition {
    /// Клиент-мод: на сервер не копируем.
    SkipClient,
    /// Копируем, сторона проверена (API или однозначный манифест).
    Copy,
    /// Копируем, но честно не проверено (нет данных).
    CopyUnchecked,
}

/// Итог фильтра для UI/отчёта создания.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModsFilterResult {
    /// Скопировано проверенных модов.
    pub copied: u32,
    /// Пропущено клиент-модов.
    pub skipped_client: u32,
    /// Скопировано непроверенных (нет project_id / офлайн / нет данных).
    pub unchecked: u32,
    /// Не скопировано из-за дубля имени (вложенные пути сводятся к одному
    /// dst — второй файл молча перезаписал бы первый, P3-ревизии).
    pub collisions: u32,
    /// Имена файлов пропущенных клиент-модов и дублей («имя (дубль имени)»).
    pub skipped_projects: Vec<String>,
}

/// Классификация одного мода. Приоритет — данные API (`project_sides` =
/// (client_side, server_side) от Modrinth); без них — `environment` из
/// fabric.mod.json (`"client"` — клиентский); данных нет вовсе — копия
/// «как есть».
pub fn classify(project_sides: Option<(&str, &str)>, fabric_env: Option<&str>) -> ModDisposition {
    if let Some((_client, server)) = project_sides {
        if server.eq_ignore_ascii_case("unsupported") {
            return ModDisposition::SkipClient;
        }
        // Включая client_side=="unsupported" (серверный): серверу нужен.
        return ModDisposition::Copy;
    }
    match fabric_env {
        Some(env) if env.eq_ignore_ascii_case("client") => ModDisposition::SkipClient,
        // "server"/"*"/прочее: не клиентский, но API-проверки не было.
        Some(_) => ModDisposition::CopyUnchecked,
        None => ModDisposition::CopyUnchecked,
    }
}

/// Имена манифестов внутри jar (строго в корне архива) и лимит чтения —
/// паттерн instances/modmeta.rs: верить заголовку zip нельзя.
const FABRIC_MANIFEST: &str = "fabric.mod.json";
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

/// Нормализованное окружение из fabric.mod.json: `Some("client")` — только
/// клиент, `Some("*")` — обе стороны/сервер/поле отсутствует, `None` —
/// манифеста нет или не распарсился (не ошибка: мод без метаданных).
fn environment_from_json(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    match value.get("environment") {
        None => Some("*".into()),
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        // Объектная форма {"client": "...", "server": "..."}: клиентский
        // только если серверная сторона явно "unsupported".
        Some(serde_json::Value::Object(map)) => {
            let server = map
                .get("server")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("required");
            Some(if server.eq_ignore_ascii_case("unsupported") {
                "client".into()
            } else {
                "*".into()
            })
        }
        Some(_) => Some("*".into()),
    }
}

/// Прочитать `environment` из fabric.mod.json внутри jar (zip) без распаковки.
fn read_fabric_environment(jar: &Path) -> Option<String> {
    let file = std::fs::File::open(long_path(jar)).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let entry = archive.by_name(FABRIC_MANIFEST).ok()?;
    let mut bytes = Vec::new();
    entry
        .take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    environment_from_json(&bytes)
}

/// Стороны проекта из батч-ответа Modrinth (REST отдаёт snake_case).
#[derive(Debug, Deserialize)]
struct ProjectSides {
    #[serde(alias = "id", alias = "project_id")]
    project_id: String,
    #[serde(default = "default_side")]
    client_side: String,
    #[serde(default = "default_side")]
    server_side: String,
}

fn default_side() -> String {
    "*".into()
}

/// project_id годится для подстановки в query (A1-принцип: IPC/файлам не доверяем).
fn valid_project_id(id: &str) -> bool {
    !id.is_empty() && !id.chars().any(char::is_whitespace) && !id.contains("..")
}

/// Батч-запрос сторон: `GET {API_BASE}/projects?ids=["a","b"]`, чанки ≤
/// PROJECTS_BATCH (паттерн modrinth/api.rs::projects_meta). Офлайн-гейт
/// делает вызывающий (здесь сеть уже разрешена).
async fn fetch_project_sides(
    http: &HttpClient,
    ids: &[String],
) -> Result<HashMap<String, (String, String)>> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let url = format!("{}/projects", crate::modrinth::api::API_BASE);
    for chunk in ids.chunks(crate::modrinth::api::PROJECTS_BATCH) {
        let ids_json = serde_json::to_string(chunk)?;
        let list: Vec<ProjectSides> = http
            .raw()
            .get(&url)
            .query(&[("ids", ids_json)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        for p in list {
            out.insert(p.project_id, (p.client_side, p.server_side));
        }
    }
    Ok(out)
}

/// Имя файла jar из логического пути записи манифеста ("mods/x.jar" → "x.jar").
fn base_name(file: &str) -> String {
    file.rsplit(['/', '\\'])
        .next()
        .unwrap_or(file)
        .to_string()
}

/// Учёт копии одного мода: dst строится по base_name, дубли не копируются.
/// P3-ревизии: resolve_content_file допускает вложенные пути — «mods/x.jar» и
/// «mods/sub/x.jar» сводятся к одному dst «mods/x.jar», и второй тихо
/// перезаписал бы первый (порядок обхода манифеста решал бы, что на сервере).
/// Занятый dst → `collisions` + имя в skipped_projects с пометкой; копируется
/// первый пришедший. `checked` — сторона проверена (счётчик copied, иначе
/// unchecked). Выделено в функцию, чтобы тест покрывал счёт без сети.
fn account_copy(
    taken: &mut HashMap<PathBuf, String>,
    mods_dst: &Path,
    res: &mut ModsFilterResult,
    jobs: &mut Vec<(PathBuf, PathBuf)>,
    src: PathBuf,
    logical_file: &str,
    checked: bool,
) {
    let name = base_name(logical_file);
    let dst = mods_dst.join(&name);
    match taken.entry(dst.clone()) {
        std::collections::hash_map::Entry::Occupied(first) => {
            res.collisions += 1;
            tracing::warn!(
                "mods_filter: «{logical_file}» не скопирован — dst {} уже занят «{}» (дубль имени)",
                dst.display(),
                first.get()
            );
            res.skipped_projects.push(format!("{name} (дубль имени)"));
        }
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert(logical_file.to_string());
            if checked {
                res.copied += 1;
            } else {
                res.unchecked += 1;
            }
            jobs.push((src, dst));
        }
    }
}

/// Скопировать моды инстанса на сервер с фильтрацией клиент-модов.
/// `instance_id` — источник (манифест + `minecraft/mods`), `server_dir` —
/// каталог сервера (моды ложатся в `mods/`). Выключенные моды и не-моды
/// игнорируются; отсутствующий на диске файл — warn и пропуск, не ошибка.
pub async fn copy_server_mods(
    paths: &Paths,
    http: &HttpClient,
    instance_id: &str,
    server_dir: &Path,
) -> Result<ModsFilterResult> {
    let entries: Vec<_> = crate::instances::load_content_manifest(paths, instance_id)
        .into_iter()
        .filter(|e| e.kind == crate::instances::ContentKind::Mod && e.enabled)
        .collect();

    // Батч сторон по project_id (офлайн/ошибка — не валим, всё unchecked).
    let project_ids: Vec<String> = entries
        .iter()
        .filter_map(|e| e.project_id.as_deref())
        .filter(|id| valid_project_id(id))
        .map(str::to_string)
        .collect();
    let sides = if http.offline() {
        tracing::warn!(
            "mods_filter: офлайн-режим — моды с project_id копируются непроверенными"
        );
        None
    } else {
        match fetch_project_sides(http, &project_ids).await {
            Ok(map) => Some(map),
            Err(e) => {
                tracing::warn!(
                    "mods_filter: стороны проектов не получены ({e}) — копирую всё непроверенным"
                );
                None
            }
        }
    };

    let mut jobs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut res = ModsFilterResult {
        copied: 0,
        skipped_client: 0,
        unchecked: 0,
        collisions: 0,
        skipped_projects: Vec::new(),
    };
    let mods_dst = server_dir.join("mods");
    // Занятые dst этой выкладки: dst → логический путь первого владельца.
    let mut taken: HashMap<PathBuf, String> = HashMap::new();

    for e in &entries {
        // resolve_content_file — traversal-барьер по логическому пути записи.
        let src = match crate::instances::content::resolve_content_file(paths, instance_id, &e.file)
        {
            Ok(p) => p,
            Err(err) => {
                tracing::warn!("mods_filter: пропуск {}: {err}", e.file);
                continue;
            }
        };
        if !src.is_file() {
            tracing::warn!("mods_filter: файла нет на диске, пропуск: {}", e.file);
            continue;
        }
        let env = if e.project_id.is_some() {
            None // сторона решается по API; манифест тут не решает (см. офлайн-ветку)
        } else {
            read_fabric_environment(&src)
        };
        let disposition = match (&sides, e.project_id.as_deref()) {
            (Some(map), Some(pid)) if valid_project_id(pid) => match map.get(pid) {
                Some((c, s)) => classify(Some((c, s)), None),
                // Проект удалён/скрыт на Modrinth: локальный fabric.mod.json
                // всё же знает environment — используем его, а не слепую копию
                // (P2-ревизии: клиент-моду с мёртвой ссылкой катило на сервер).
                None => classify(None, read_fabric_environment(&src).as_deref()),
            },
            // Нет project_id, офлайн или id не прошёл валидацию: manifest/env.
            _ => classify(None, env.as_deref()),
        };
        match disposition {
            ModDisposition::SkipClient => {
                res.skipped_client += 1;
                res.skipped_projects.push(base_name(&e.file));
            }
            ModDisposition::Copy => {
                account_copy(&mut taken, &mods_dst, &mut res, &mut jobs, src, &e.file, true)
            }
            ModDisposition::CopyUnchecked => {
                account_copy(&mut taken, &mods_dst, &mut res, &mut jobs, src, &e.file, false)
            }
        }
    }

    // Тяжёлая побайтовая копия — вне async-исполнителя.
    tokio::task::spawn_blocking(move || -> Result<()> {
        for (src, dst) in &jobs {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(long_path(parent))?;
            }
            std::fs::copy(long_path(src), long_path(dst))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| LauncherError::internal(format!("копирование модов: {e}")))??;

    tracing::info!(
        "mods_filter: скопировано {}, непроверенных {}, клиент-модов пропущено {}, дублей имени {}",
        res.copied,
        res.unchecked,
        res.skipped_client,
        res.collisions
    );
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn classify_by_api_sides() {
        // server_side unsupported → клиент-мод.
        assert_eq!(
            classify(Some(("required", "unsupported")), None),
            ModDisposition::SkipClient
        );
        // Двусторонний и server-only → копируем проверенно.
        assert_eq!(
            classify(Some(("optional", "required")), Some("client")),
            ModDisposition::Copy,
            "данные API приоритетнее манифеста"
        );
        assert_eq!(
            classify(Some(("unsupported", "required")), None),
            ModDisposition::Copy,
            "server-only нужен серверу"
        );
        // "unknown"/"optional" — не unsupported → копия.
        assert_eq!(classify(Some(("unknown", "unknown")), None), ModDisposition::Copy);
    }

    #[test]
    fn classify_by_fabric_environment() {
        assert_eq!(
            classify(None, Some("client")),
            ModDisposition::SkipClient
        );
        assert_eq!(classify(None, Some("server")), ModDisposition::CopyUnchecked);
        assert_eq!(classify(None, Some("*")), ModDisposition::CopyUnchecked);
        assert_eq!(classify(None, None), ModDisposition::CopyUnchecked);
    }

    #[test]
    fn environment_json_forms() {
        assert_eq!(environment_from_json(br#"{"id":"a","environment":"client"}"#).as_deref(), Some("client"));
        assert_eq!(environment_from_json(br#"{"id":"a","environment":"*"}"#).as_deref(), Some("*"));
        // Поле отсутствует → обе стороны.
        assert_eq!(environment_from_json(br#"{"id":"a"}"#).as_deref(), Some("*"));
        // Объектная форма: server unsupported → клиентский.
        assert_eq!(
            environment_from_json(br#"{"environment":{"client":"required","server":"unsupported"}}"#).as_deref(),
            Some("client")
        );
        assert_eq!(
            environment_from_json(br#"{"environment":{"client":"unsupported","server":"required"}}"#).as_deref(),
            Some("*")
        );
        // Битый json → данных нет.
        assert!(environment_from_json(b"{ nope").is_none());
    }

    /// Фикстуры: jar (zip) с fabric.mod.json / без манифеста / битый файл.
    fn write_jar(path: &Path, entries: &[(&str, &str)]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(long_path(parent)).unwrap();
        }
        let f = std::fs::File::create(long_path(path)).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = Default::default();
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    /// Офлайн-прогон на фикстурах: клиент-мод по манифесту пропущен,
    /// моды без манифеста и с project_id (батч недоступен) — unchecked,
    /// выключенные и не-моды игнорируются. Сети нет — create не валится.
    #[tokio::test]
    async fn copy_server_mods_offline_filters_by_manifest() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::new(d.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let inst = crate::instances::Instance::new("Тест", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();

        let mods = crate::instances::minecraft_dir(&crate::instances::instance_dir(
            &paths, &inst.id,
        ))
        .join("mods");
        write_jar(
            &mods.join("client-only.jar"),
            &[("fabric.mod.json", r#"{"id":"c","environment":"client"}"#)],
        );
        write_jar(
            &mods.join("both.jar"),
            &[("fabric.mod.json", r#"{"id":"b","environment":"*"}"#)],
        );
        write_jar(&mods.join("forge-like.jar"), &[("META-INF/mods.toml", "x")]);
        std::fs::write(long_path(&mods.join("broken.jar")), b"not a zip").unwrap();
        // jar для записи с project_id (стороны недоступны офлайн).
        write_jar(&mods.join("api-mod.jar"), &[("fabric.mod.json", r#"{"id":"api"}"#)]);

        let rp = crate::instances::ContentEntry {
            kind: crate::instances::ContentKind::Mod,
            file: "mods/client-only.jar".into(),
            source: crate::instances::ContentSource::Local,
            project_id: None,
            version_id: None,
            sha1: None,
            url: None,
            enabled: true,
        };
        let mut api_mod = rp.clone();
        api_mod.file = "mods/api-mod.jar".into();
        api_mod.project_id = Some("AABBCCDD".into());
        let mut forge_like = rp.clone();
        forge_like.file = "mods/forge-like.jar".into();
        let mut broken = rp.clone();
        broken.file = "mods/broken.jar".into();
        let mut disabled = rp.clone();
        disabled.file = "mods/both.jar".into();
        disabled.enabled = false;
        let mut pack = rp.clone();
        pack.kind = crate::instances::ContentKind::ResourcePack;
        pack.file = "resourcepacks/p.zip".into();
        // ghost: запись манифеста без jar на диске — warn-пропуск.
        let mut ghost = rp.clone();
        ghost.file = "mods/ghost.jar".into();
        crate::instances::save_content_manifest(
            &paths,
            &inst.id,
            &[rp, api_mod, forge_like, broken, disabled, pack, ghost],
        )
        .unwrap();

        let http = HttpClient::new(None).unwrap();
        http.set_offline(true);
        let srv = d.path().join("srv");
        std::fs::create_dir_all(&srv).unwrap();

        let res = copy_server_mods(&paths, &http, &inst.id, &srv).await.unwrap();
        assert_eq!(res.skipped_client, 1, "client-only.jar пропущен");
        assert_eq!(res.skipped_projects, vec!["client-only.jar".to_string()]);
        assert_eq!(
            res.unchecked, 3,
            "forge-like (манифеста нет) + broken (битый) + api-mod (офлайн)"
        );
        assert_eq!(res.copied, 0, "офлайн — проверенных копий нет");
        // На диске: непроверенные реально скопированы.
        assert!(srv.join("mods/forge-like.jar").is_file());
        assert!(srv.join("mods/broken.jar").is_file());
        assert!(srv.join("mods/api-mod.jar").is_file());
        assert!(!srv.join("mods/client-only.jar").exists());
        assert!(!srv.join("mods/both.jar").exists(), "выключенный не копируется");
        assert!(!srv.join("mods/ghost.jar").exists(), "файла не было — копии нет");
    }

    #[test]
    fn base_name_handles_separators() {
        assert_eq!(base_name("mods/jei.jar"), "jei.jar");
        assert_eq!(base_name("mods\\jei.jar"), "jei.jar");
        assert_eq!(base_name("jei.jar"), "jei.jar");
    }

    #[test]
    fn valid_project_id_rules() {
        assert!(valid_project_id("AABBCCDD"));
        assert!(!valid_project_id(""));
        assert!(!valid_project_id("a b"));
        assert!(!valid_project_id("../evil"));
    }

    /// Сериализация результата — camelCase-контракт с UI.
    #[test]
    fn mods_filter_result_serializes_camel_case() {
        let v = serde_json::to_value(ModsFilterResult {
            copied: 3,
            skipped_client: 1,
            unchecked: 2,
            collisions: 4,
            skipped_projects: vec!["x.jar".into()],
        })
        .unwrap();
        assert_eq!(v["copied"], 3);
        assert_eq!(v["skippedClient"], 1);
        assert_eq!(v["unchecked"], 2);
        assert_eq!(v["collisions"], 4);
        assert_eq!(v["skippedProjects"], serde_json::json!(["x.jar"]));
    }

    /// P3: два вложенных мода с одинаковым base_name — «mods/a/dupe.jar» и
    /// «mods/b/dupe.jar» легли бы в один dst «mods/dupe.jar». Копируется
    /// первый (copied=1), второй НЕ копируется: collisions=1 и имя в
    /// skipped_projects с пометкой.
    #[test]
    fn collision_nested_same_base_name_copied_1_collisions_1() {
        let mods_dst = Path::new("srv/mods");
        let mut taken: HashMap<PathBuf, String> = HashMap::new();
        let mut res = ModsFilterResult {
            copied: 0,
            skipped_client: 0,
            unchecked: 0,
            collisions: 0,
            skipped_projects: Vec::new(),
        };
        let mut jobs: Vec<(PathBuf, PathBuf)> = Vec::new();

        account_copy(
            &mut taken,
            mods_dst,
            &mut res,
            &mut jobs,
            PathBuf::from("inst/minecraft/mods/a/dupe.jar"),
            "mods/a/dupe.jar",
            true,
        );
        account_copy(
            &mut taken,
            mods_dst,
            &mut res,
            &mut jobs,
            PathBuf::from("inst/minecraft/mods/b/dupe.jar"),
            "mods/b/dupe.jar",
            true,
        );

        assert_eq!(res.copied, 1, "второй дубль не увеличивает copied");
        assert_eq!(res.collisions, 1);
        assert_eq!(res.skipped_projects, vec!["dupe.jar (дубль имени)".to_string()]);
        assert_eq!(jobs.len(), 1, "копия ровно одна");
        assert_eq!(jobs[0].1, mods_dst.join("dupe.jar"));
        assert_eq!(jobs[0].0, PathBuf::from("inst/minecraft/mods/a/dupe.jar"));
    }

    /// Тот же конфликт на живом прогоне (офлайн, манифеста в jar нет — оба
    /// unchecked): на диске ровно один файл, второй ушёл в collisions.
    #[tokio::test]
    async fn copy_server_mods_nested_dupes_collide_once() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::new(d.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let inst = crate::instances::Instance::new("Тест", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();

        let mods = crate::instances::minecraft_dir(&crate::instances::instance_dir(
            &paths, &inst.id,
        ))
        .join("mods");
        write_jar(&mods.join("a").join("dupe.jar"), &[("META-INF/mods.toml", "a")]);
        write_jar(&mods.join("b").join("dupe.jar"), &[("META-INF/mods.toml", "b")]);

        let entry = crate::instances::ContentEntry {
            kind: crate::instances::ContentKind::Mod,
            file: "mods/a/dupe.jar".into(),
            source: crate::instances::ContentSource::Local,
            project_id: None,
            version_id: None,
            sha1: None,
            url: None,
            enabled: true,
        };
        let mut second = entry.clone();
        second.file = "mods/b/dupe.jar".into();
        crate::instances::save_content_manifest(&paths, &inst.id, &[entry, second]).unwrap();

        let http = HttpClient::new(None).unwrap();
        http.set_offline(true);
        let srv = d.path().join("srv");
        std::fs::create_dir_all(&srv).unwrap();

        let res = copy_server_mods(&paths, &http, &inst.id, &srv).await.unwrap();
        assert_eq!(res.collisions, 1, "второй вложенный дубль посчитан");
        assert_eq!(res.unchecked, 1, "скопирован только первый (офлайн — unchecked)");
        assert_eq!(res.copied, 0);
        assert!(srv.join("mods/dupe.jar").is_file());
        assert_eq!(
            std::fs::read_dir(srv.join("mods")).unwrap().count(),
            1,
            "на сервере ровно один файл, перезаписи нет"
        );
        assert!(res
            .skipped_projects
            .contains(&"dupe.jar (дубль имени)".to_string()));
    }
}
