//! Проверка целостности и починка файлов инстанса (F2, D37).
//!
//! Семантика: перезапуск prepare-цепочки скачивания (client jar + libraries +
//! assets) для инстанса. Движок сам проверяет хэш каждого файла на диске и
//! докачивает только отсутствующие/битые — это и есть починка. Уникальные
//! данные (saves/, config/, mods/, screenshots/, options.txt) в план не входят
//! и не затрагиваются; жёсткие ссылки в каталог игры восстанавливаются при
//! следующем запуске (prepare() делает link_or_copy безусловно).

use crate::errors::{LauncherError, Result};
use crate::instances::Instance;
use crate::mojang::assets::{fetch_asset_index, plan_asset_tasks};
use crate::mojang::client::plan_client_task;
use crate::mojang::libraries::plan_libraries;
use crate::mojang::manifest::{fetch_manifest, fetch_version_json_cached, resolve_entry};
use crate::mojang::rules::OsContext;
use crate::mojang::version::VersionJson;
use crate::net::download::{DownloadEngine, DownloadTask};
use crate::paths::Paths;
use std::collections::BTreeMap;

/// Итог проверки файлов (зеркало `RepairReport` в src/api/types.ts).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairReport {
    /// Сколько файлов движок проверил/скачал в ходе этого ремонта.
    pub checked: u32,
    /// Сколько задач очереди завершились успешно (валидные на диске — тоже:
    /// так считает счётчик done движка).
    pub redownloaded: u32,
}

/// Сумма задач очереди по всем состояниям (поля DlQueueState).
fn queue_total(st: &crate::events::DlQueueState) -> u64 {
    st.pending + st.downloading + st.done + st.failed + st.cancelled
}

/// Проверить/восстановить игровые файлы инстанса (F2).
/// Логика плана повторяет prepare() (instances/run.rs): та же цепочка
/// crate::mojang pub-функций; вынести её из run.rs нельзя (файл не мой).
pub async fn repair_instance(
    paths: &Paths,
    http: std::sync::Arc<crate::net::http::HttpClient>,
    engine: std::sync::Arc<DownloadEngine>,
    instance_id: &str,
) -> Result<RepairReport> {
    let inst: Instance = crate::instances::load(paths, instance_id)?;

    // Guard: работающий инстанс чинить нельзя — игра держит файлы.
    let dir = crate::instances::instance_dir(paths, instance_id);
    if crate::instances::running_pid(&dir).is_some() {
        return Err(LauncherError::InstanceRunning(format!(
            "инстанс {instance_id} запущен — остановите игру перед проверкой файлов"
        )));
    }
    paths.ensure_dirs()?;

    // 1. Версия: тот же резолвинг, что в prepare(), но манифест запрашивается
    //    только когда он действительно нужен (version_id отсутствует):
    //    ремонт инстанса с загрузчиком не должен зависеть от доступности сети.
    let instance_versions = crate::instances::instance_versions_dir(paths, &inst.id);
    let root_json = if let Some(vid) = &inst.version_id {
        let path = instance_versions.join(format!("{vid}.json"));
        if !path.exists() {
            return Err(LauncherError::NotFound(format!(
                "version JSON загрузчика {vid} — установите загрузчик заново"
            )));
        }
        VersionJson::load(&path)?
    } else {
        let manifest = fetch_manifest(&http, &paths.manifests_cache()).await?;
        let entry = resolve_entry(&manifest, &inst.mc_version)?;
        fetch_version_json_cached(&http, entry, &paths.manifests_cache()).await?
    };
    // Родители цепочки ищутся в versions/ инстанса, затем в кэше Mojang.
    let rv = VersionJson::resolve_chain_dirs(
        &[instance_versions, paths.manifests_cache()],
        &root_json,
    )?;

    // 2. План загрузок — то же, что prepare() кладёт в движок. Битность берём
    //    из процесса: java в ремонте не резолвится, от неё зависят только
    //    классификаторы нативов (`${arch}`).
    let arch_bits = if std::env::consts::ARCH == "x86" { 32 } else { 64 };
    let os_ctx = OsContext::current(arch_bits, std::env::consts::ARCH.contains("arm"));
    let group = format!("instance:{}", inst.id);
    let mut tasks: Vec<DownloadTask> = vec![plan_client_task(&rv, &paths.clients_store(), &group)?];
    let libs = plan_libraries(&rv, &os_ctx, &BTreeMap::new())?;
    for l in &libs {
        if let Some(a) = &l.artifact {
            tasks.push(DownloadTask {
                id: format!("lib:{}", a.rel_path),
                url: a.url.clone(),
                dest: paths.libraries_store().join(&a.rel_path),
                sha1: a.sha1.clone(),
                sha512: None,
                size: a.size,
                group: group.clone(),
                priority: 10,
            });
        }
        if let Some(n) = &l.native {
            tasks.push(DownloadTask {
                id: format!("native:{}", n.rel_path),
                url: n.url.clone(),
                dest: paths.libraries_store().join(&n.rel_path),
                sha1: n.sha1.clone(),
                sha512: None,
                size: n.size,
                group: group.clone(),
                priority: 15,
            });
        }
    }
    let ai = rv.asset_index.clone().ok_or_else(|| {
        LauncherError::InvalidInput(format!("в версии {} нет assetIndex", rv.id))
    })?;
    let index = fetch_asset_index(
        &http,
        &ai.url,
        &ai.sha1,
        &ai.id,
        &paths.assets_indexes(),
    )
    .await?;
    tasks.extend(plan_asset_tasks(&index, &paths.assets_objects(), &group));
    if let Some(lc) = rv.logging.as_ref().and_then(|l| l.client.as_ref()) {
        tasks.push(DownloadTask {
            id: format!("log-config:{}", lc.file.id),
            url: lc.file.url.clone(),
            dest: paths.cache_dir().join("log_configs").join(&lc.file.id),
            sha1: Some(lc.file.sha1.clone()),
            sha512: None,
            size: lc.file.size,
            group: group.clone(),
            priority: 20,
        });
    }

    // 3. Прогон через движок. Дельты очереди (до/после): движок живёт дольше
    //    одного ремонта, поэтому «проверено/перекачано» считаем только по
    //    задачам этого прогона, а не по всем с момента его создания.
    let before = engine.queue_state();
    engine.add_tasks(tasks);
    engine.run().await?;
        let after = engine.queue_state();

    let checked = queue_total(&after).saturating_sub(queue_total(&before));
    let redownloaded = after.done.saturating_sub(before.done);
    let failed = after.failed.saturating_sub(before.failed);
    if failed > 0 {
        for (url, reason) in &after.failed_items {
            tracing::error!("ремонт {instance_id}: загрузка не удалась: {url}: {reason}");
        }
        return Err(LauncherError::network(format!(
            "{failed} загрузок не удалось (см. список в UI)"
        )));
    }
    tracing::info!("инстанс {instance_id}: проверено {checked}, перекачано {redownloaded}");
    Ok(RepairReport {
        checked: u32::try_from(checked).unwrap_or(u32::MAX),
        redownloaded: u32::try_from(redownloaded).unwrap_or(u32::MAX),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use super::*;
    use crate::net::http::HttpClient;

    fn test_paths() -> (tempfile::TempDir, crate::paths::Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        (dir, paths)
    }

    struct Fixture {
        version_json: serde_json::Value,
        index_json: serde_json::Value,
    }

    /// Автономная версия (загрузчикоподобная, без сети): клиент, библиотека и
    /// один ассет с хэшами от содержимого, которое тест сам кладёт в стор.
    fn fixture() -> Fixture {
        let client_sha = crate::util::fs::sha1_bytes(b"client-jar-bytes");
        let lib_sha = crate::util::fs::sha1_bytes(b"fake-lib-bytes");
        let asset_sha = crate::util::fs::sha1_bytes(b"asset-object-bytes");
        let index_json = serde_json::json!({
            "objects": {
                "misc/ping.ogg": { "hash": asset_sha, "size": 19 }
            }
        });
        let index_sha = crate::util::fs::sha1_bytes(index_json.to_string().as_bytes());
        let version_json = serde_json::json!({
            "id": "repair-loader-1.20.1",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "assets": "test-index",
            "assetIndex": {
                "id": "test-index",
                "sha1": index_sha,
                "size": index_json.to_string().len(),
                "url": "http://localhost/test-index.json"
            },
            "downloads": {
                "client": {
                    "url": "http://localhost/client.jar",
                    "sha1": client_sha,
                    "size": 16
                }
            },
            "javaVersion": { "majorVersion": 17 },
            "libraries": [
                {
                    "name": "com.example:fake-lib:1.0",
                    "downloads": {
                        "artifact": {
                            "url": "http://localhost/fake-lib-1.0.jar",
                            "path": "com/example/fake-lib/1.0/fake-lib-1.0.jar",
                            "sha1": lib_sha,
                            "size": 15
                        }
                    }
                }
            ]
        });
        Fixture {
            version_json,
            index_json,
        }
    }

    /// Записать version JSON в versions/ инстанса (ветка version_id) либо в
    /// кэш манифестов (ветка mc_version), разложить валидные файлы по стору.
    fn setup_instance(
        paths: &crate::paths::Paths,
        complete: bool,
        fx: &Fixture,
        with_version_id: bool,
    ) -> Instance {
        let client_sha = crate::util::fs::sha1_bytes(b"client-jar-bytes");
        let asset_sha = crate::util::fs::sha1_bytes(b"asset-object-bytes");

        let mut inst = Instance::new("repair-test", "1.20.1");
        if with_version_id {
            inst.version_id = Some(fx.version_json["id"].as_str().unwrap().to_string());
            let versions = crate::instances::instance_versions_dir(paths, &inst.id);
            std::fs::create_dir_all(crate::util::fs::long_path(&versions)).unwrap();
            std::fs::write(
                crate::util::fs::long_path(&versions.join(format!(
                    "{}.json",
                    fx.version_json["id"].as_str().unwrap()
                ))),
                fx.version_json.to_string(),
            )
            .unwrap();
        } else {
            // Ветка mc_version: манифест+version JSON кладутся в кэш —
            // fetch_manifest/fetch_version_json_cached отдадут их без сети
            // (TTL-метка свежая).
            let cache = paths.manifests_cache();
            std::fs::create_dir_all(crate::util::fs::long_path(&cache)).unwrap();
            let manifest = serde_json::json!({
                "latest": { "release": "1.20.1", "snapshot": "snapshot-x" },
                "versions": [
                    {
                        "id": "1.20.1",
                        "type": "release",
                        "url": "http://localhost/1.20.1.json",
                        "time": "2024-01-01T00:00:00+00:00",
                        "releaseTime": "2024-01-01T00:00:00+00:00",
                        "sha1": crate::util::fs::sha1_bytes(fx.version_json.to_string().as_bytes()),
                        "complianceLevel": 1
                    }
                ]
            });
            std::fs::write(
                crate::util::fs::long_path(&cache.join("version_manifest_v2.json")),
                manifest.to_string(),
            )
            .unwrap();
            std::fs::write(
                crate::util::fs::long_path(&cache.join("version_manifest_v2.meta.json")),
                serde_json::to_vec(&crate::instances::now_secs()).unwrap(),
            )
            .unwrap();
            std::fs::write(
                crate::util::fs::long_path(&cache.join("1.20.1.json")),
                fx.version_json.to_string(),
            )
            .unwrap();
        }
        // asset index в кэше по своему sha1 (fetch_asset_index — кэш-фёрст).
        std::fs::write(
            crate::util::fs::long_path(&paths.assets_indexes().join("test-index.json")),
            fx.index_json.to_string(),
        )
        .unwrap();

        if complete {
            std::fs::create_dir_all(crate::util::fs::long_path(&paths.clients_store())).unwrap();
            std::fs::write(
                crate::util::fs::long_path(&paths.clients_store().join(format!("{client_sha}.jar"))),
                b"client-jar-bytes",
            )
            .unwrap();
            let lib_rel = "com/example/fake-lib/1.0/fake-lib-1.0.jar";
            let lib_path = paths.libraries_store().join(lib_rel);
            std::fs::create_dir_all(crate::util::fs::long_path(lib_path.parent().unwrap()))
                .unwrap();
            std::fs::write(crate::util::fs::long_path(&lib_path), b"fake-lib-bytes").unwrap();
            let obj = paths.assets_objects().join(&asset_sha[..2]).join(&asset_sha);
            std::fs::create_dir_all(crate::util::fs::long_path(obj.parent().unwrap())).unwrap();
            std::fs::write(crate::util::fs::long_path(&obj), b"asset-object-bytes").unwrap();
        }

        crate::instances::save(paths, &inst).unwrap();
        inst
    }

    fn offline_engine() -> (Arc<HttpClient>, Arc<DownloadEngine>) {
        let client = Arc::new(HttpClient::new(None).unwrap());
        client.set_offline(true);
        let engine = DownloadEngine::new(client.clone(), 2, crate::events::EventBus::default());
        (client, engine)
    }

    /// F2: все файлы валидны → движок проверяет их офлайн (существующие с
    /// корректным хэшем отдаются до сетевых попыток), ремонт успешен.
    #[tokio::test]
    async fn repair_checks_valid_files_without_network() {
        let (_d, paths) = test_paths();
        let fx = fixture();
        let inst = setup_instance(&paths, true, &fx, true);
        let (client, engine) = offline_engine();

        let report = repair_instance(&paths, client, engine, &inst.id)
            .await
            .unwrap();
        // Клиент + библиотека + один ассет — все проверены.
        assert_eq!(report.checked, 3);
        // Семантика done движка: валидные на диске файлы тоже считаются
        // успешно завершёнными задачами.
        assert_eq!(report.redownloaded, 3);
    }

    /// F2: отсутствующий ассет-объект → офлайн-движок не может его скачать,
    /// ремонт падает сетевой ошибкой (сообщение как у prepare).
    #[tokio::test]
    async fn repair_fails_when_file_missing_and_offline() {
        let (_d, paths) = test_paths();
        let fx = fixture();
        let inst = setup_instance(&paths, false, &fx, true);
        let (client, engine) = offline_engine();

        let err = repair_instance(&paths, client, engine, &inst.id)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "network", "{err}");
    }

    /// F2: чинить работающий инстанс запрещено (игра держит файлы).
    #[tokio::test]
    async fn repair_rejects_running_instance() {
        let (_d, paths) = test_paths();
        let fx = fixture();
        let inst = setup_instance(&paths, true, &fx, true);
        let dir = crate::instances::instance_dir(&paths, &inst.id);
        std::fs::write(
            crate::util::fs::long_path(&dir.join(".lock")),
            std::process::id().to_string(),
        )
        .unwrap();
        let (client, engine) = offline_engine();

        let err = repair_instance(&paths, client, engine, &inst.id)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "instance_running", "{err}");
    }

    /// F2: инстанс без version_id резолвится через манифест+кэш Mojang —
    /// тот же путь, что в prepare() (manifest → resolve_entry → cached JSON).
    #[tokio::test]
    async fn repair_resolves_plain_mc_version_via_manifest_cache() {
        let (_d, paths) = test_paths();
        let fx = fixture();
        let inst = setup_instance(&paths, true, &fx, false);
        let (client, engine) = offline_engine();

        let report = repair_instance(&paths, client, engine, &inst.id)
            .await
            .unwrap();
        assert_eq!(report.checked, 3);
        assert_eq!(report.redownloaded, 3);
    }

    /// Отчёт сериализуется в UI-контракт camelCase (зеркало types.ts).
    #[test]
    fn report_serializes_camel_case_for_ui() {
        let v = serde_json::to_value(RepairReport {
            checked: 12,
            redownloaded: 3,
        })
        .unwrap();
        assert_eq!(v["checked"], 12);
        assert_eq!(v["redownloaded"], 3);
        assert!(v.get("checked_files").is_none());
    }
}
