//! Модпаки .mrpack (спека §6.2, §6.6): modrinth.index.json + overrides/,
//! зависимости → создание инстанса нужного вида, env-фильтр client.

use crate::errors::{LauncherError, Result};
use crate::instances::{ContentEntry, ContentKind, ContentSource, Instance};
use crate::net::download::{DownloadEngine, DownloadTask};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackIndex {
    pub format_version: u32,
    #[serde(default)]
    pub game: Option<String>,
    pub version_id: String,
    pub name: String,
    #[serde(default)]
    pub files: Vec<MrpackFile>,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackFile {
    pub path: String,
    pub hashes: HashMap<String, String>,
    /// env может отсутствовать — тогда считается required (спека §6.2).
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    pub downloads: Vec<String>,
    #[serde(default)]
    pub file_size: u64,
}

impl MrpackIndex {
    /// Проверка структуры: внятные ошибки вместо паник (спека §6.7 — битые
    /// манифесты сообщать понятно).
    pub fn validate(&self) -> Result<()> {
        if self.format_version != 1 {
            return Err(LauncherError::InvalidInput(format!(
                "formatVersion {} не поддерживается (ждём 1)",
                self.format_version
            )));
        }
        if let Some(game) = &self.game {
            if game != "minecraft" {
                return Err(LauncherError::InvalidInput(format!(
                    "игра модпака: {game}, ждём minecraft"
                )));
            }
        }
        Ok(())
    }

    /// Файлы для КЛИЕНТА: env.client != "unsupported" (спека §6.6).
    pub fn client_files(&self) -> impl Iterator<Item = &MrpackFile> {
        self.files.iter().filter(|f| {
            f.env
                .as_ref()
                .and_then(|e| e.get("client").map(|c| c != "unsupported"))
                .unwrap_or(true)
        })
    }
}

/// Открыть .mrpack (zip) и распарсить modrinth.index.json.
fn open_mrpack(path: &Path) -> Result<MrpackIndex> {
    let file = std::fs::File::open(crate::util::fs::long_path(path))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| LauncherError::Zip(format!("не удалось открыть .mrpack: {e}")))?;
    let index_entry = zip
        .by_name("modrinth.index.json")
        .map_err(|_| {
            LauncherError::InvalidInput(
                "в .mrpack нет modrinth.index.json — это не модпак Modrinth".into(),
            )
        })?;
    let index: MrpackIndex = serde_json::from_reader(index_entry)?;
    index.validate()?;
    Ok(index)
}

/// Полная установка .mrpack: инстанс по dependencies → overrides поверх →
/// файлы с хэшами → content-manifest. Возвращает созданный инстанс.
pub async fn install_mrpack(
    paths: &Paths,
    settings: &crate::settings::Settings,
    client: Arc<HttpClient>,
    mrpack_path: &Path,
    name_override: Option<&str>,
) -> Result<Instance> {
    let index = open_mrpack(mrpack_path)?;

    // 1. Инстанс нужного вида: dependencies {minecraft, fabric-loader?, …}.
    let mc = index.dependencies.get("minecraft").ok_or_else(|| {
        LauncherError::InvalidInput("в модпаке нет зависимости minecraft".into())
    })?;
    let mut inst = Instance::new(name_override.unwrap_or(&index.name), mc);
    // Загрузчик: fabric/quilt — через profile JSON, neoforge — инсталлятор,
    // forge — честный отказ до M7 (спека §15: не падать молча).
    let loader_entry = index
        .dependencies
        .iter()
        .find(|(k, _)| k.ends_with("-loader"))
        .map(|(k, v)| (k.clone(), v.clone()));
    inst.mc_version = mc.clone();

    // 2. Файлы модпака в очередь (до создания инстанса на диске — каталоги
    //    создаст движок).
    let engine = DownloadEngine::new(client.clone(), settings.download_parallelism as usize, crate::events::EventBus::default());
    let group = format!("mrpack:{}", index.version_id);
    let mut tasks = Vec::new();
    for f in index.client_files() {
        if f.downloads.is_empty() {
            return Err(LauncherError::InvalidInput(format!(
                "у файла {} нет ссылок для скачивания",
                f.path
            )));
        }
        // Санитизация пути из ЧУЖОГО модпака (спека §3): только относительные
        // безопасные компоненты.
        let rel = crate::util::zip::safe_relative_path(&f.path)?;
        tasks.push(DownloadTask {
            id: format!("mrpack:{}", f.path),
            url: f.downloads[0].clone(),
            dest: crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id))
                .join(rel),
            sha1: f.hashes.get("sha1").cloned(),
            size: Some(f.file_size),
            group: group.clone(),
            priority: 15,
        });
    }

    // 3. Сохранить инстанс, поставить загрузчик.
    crate::instances::save(paths, &inst)?;
    if let Some((loader_key, loader_version)) = &loader_entry {
        let loader = match loader_key.as_str() {
            "fabric-loader" => "fabric",
            "quilt-loader" => "quilt",
            "neoforge" => "neoforge",
            "forge" => {
                return Err(LauncherError::InvalidInput(
                    "модпак на Forge: поддержка Forge появится на этапе M7".into(),
                ));
            }
            other => {
                return Err(LauncherError::InvalidInput(format!(
                    "неизвестный загрузчик модпака: {other}"
                )));
            }
        };
        // Версию загрузчика можно не указывать: install_loader берёт latest
        // стабильную, если None — но модпак требует конкретную.
        inst = crate::instances::run::install_loader(
            paths,
            settings,
            client.clone(),
            &inst,
            loader,
            Some(loader_version),
        )
        .await?;
    }

    // 4. Загрузка файлов.
    engine.add_tasks(tasks);
    engine.run().await?;
    engine.persist_queue(&paths.queue_file())?;
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

    // 5. Overrides из .mrpack поверх каталога игры (zip-slip-защита в extract).
    apply_overrides(mrpack_path, paths, &inst.id)?;

    // 6. content-manifest: только моды/рп/шейдеры (пути не в mods/ — local).
    let mut manifest = Vec::new();
    for f in index.client_files() {
        let rel = crate::util::zip::safe_relative_path(&f.path)?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let kind = if rel_str.starts_with("mods/") {
            ContentKind::Mod
        } else if rel_str.starts_with("resourcepacks/") {
            ContentKind::ResourcePack
        } else if rel_str.starts_with("shaderpacks/") {
            ContentKind::Shader
        } else {
            continue; // конфиги и прочее — не контент для обновлений
        };
        manifest.push(ContentEntry {
            kind,
            file: rel_str,
            source: ContentSource::Modrinth,
            project_id: None,
            version_id: Some(index.version_id.clone()),
            sha1: f.hashes.get("sha1").cloned(),
            enabled: true,
        });
    }
    crate::instances::save_content_manifest(paths, &inst.id, &manifest)?;
    Ok(inst)
}

/// Распаковать `overrides/` из .mrpack поверх каталога игры.
fn apply_overrides(mrpack_path: &Path, paths: &Paths, instance_id: &str) -> Result<()> {
    let file = std::fs::File::open(crate::util::fs::long_path(mrpack_path))?;
    let dest = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, instance_id));
    let extracted = crate::util::zip::extract_zip_filtered(
        file,
        &dest,
        |name| name.starts_with("overrides/") && !name.ends_with('/'),
        |_| {},
    )?;
    tracing::info!("overrides применены: {} файлов", extracted.len());
    Ok(())
}
