//! Импорт (спека §6.11): CF-zip, MultiMC/Prism zip, свой формат.
//! Чужие архивы — недоверенный вход: zip-slip-защита в util::zip.

use crate::errors::{LauncherError, Result};
use crate::instances::Instance;
use crate::paths::Paths;
use serde::Deserialize;
use std::path::Path;

/// Тип архива (эвристика по содержимому).
pub enum ArchiveKind {
    CurseForge,
    MultiMc,
    Own,
}

fn sniff(zip: &mut zip::ZipArchive<impl std::io::Read + std::io::Seek>) -> Result<ArchiveKind> {
    for i in 0..zip.len() {
        let name = zip
            .by_index(i)
            .map_err(|e| LauncherError::Zip(e.to_string()))?
            .name()
            .to_string();
        if name.ends_with("manifest.json") && name.matches('/').count() <= 1 {
            return Ok(ArchiveKind::CurseForge);
        }
        if name.ends_with("mmc-pack.json") || name.ends_with("instance.cfg") {
            return Ok(ArchiveKind::MultiMc);
        }
        if name == "instance.json" {
            return Ok(ArchiveKind::Own);
        }
    }
    Err(LauncherError::InvalidInput(
        "не удалось определить тип архива: нет manifest.json / mmc-pack.json / instance.json".into(),
    ))
}

// ---------- CurseForge zip (спека §6.2) ----------

#[derive(Deserialize)]
pub struct CfManifest {
    pub minecraft: CfMinecraft,
    pub name: String,
    #[serde(default)]
    pub files: Vec<CfFileRef>,
    #[serde(default = "default_overrides")]
    pub overrides: String,
}

fn default_overrides() -> String {
    "overrides".into()
}

#[derive(Deserialize)]
pub struct CfMinecraft {
    pub version: String,
    /// CF manifest.json использует camelCase .
    #[serde(default, rename = "modLoaders")]
    pub modloaders: Vec<CfModloader>,
}

#[derive(Deserialize)]
pub struct CfModloader {
    pub id: String,
}

#[derive(Deserialize)]
pub struct CfFileRef {
    #[serde(rename = "projectID")]
    pub project_id: u64,
    #[serde(rename = "fileID")]
    pub file_id: u64,
}

/// CF-файлы без ключа API: список ссылок для ручного скачивания (спека §6.11).
pub async fn cf_missing_files_links(
    client: &crate::net::http::HttpClient,
    files: &[CfFileRef],
) -> Vec<String> {
    let mut links = Vec::new();
    for f in files {
        // Без ключа в CF API не попасть; честная ссылка на файл модпака.
        links.push(format!(
            "https://www.curseforge.com/api/v1/mods/{}/files/{}/download",
            f.project_id, f.file_id
        ));
    }
    let _ = client;
    links
}

// ---------- MultiMC/Prism (спека §6.2) ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MmcPack {
    #[serde(default)]
    pub components: Vec<MmcComponent>,
}

#[derive(Deserialize)]
pub struct MmcComponent {
    #[serde(default)]
    pub cached_name: Option<String>,
    pub uid: String,
    #[serde(default)]
    pub version: Option<String>,
}

fn loader_from_uid(uid: &str, version: Option<&str>) -> Option<(String, String)> {
    match uid {
        "net.fabricmc.fabric-loader" => Some(("fabric".into(), version?.to_string())),
        "org.quiltmc.quilt-loader" => Some(("quilt".into(), version?.to_string())),
        "net.neoforged.neoforge" => Some(("neoforge".into(), version?.to_string())),
        "net.minecraftforge" => Some(("forge".into(), version?.to_string())),
        _ => None,
    }
}

/// Импорт архива в новый инстанс: создаёт Instance (без загрузчика — его
/// ставит вызывающий через install_loader, если распознан) и распаковывает
/// игровое содержимое. Возвращает (инстанс, подсказка по загрузчику).
pub async fn import_archive(
    paths: &Paths,
    _settings: &crate::settings::Settings,
    client: std::sync::Arc<crate::net::http::HttpClient>,
    archive: &Path,
    name_override: Option<&str>,
) -> Result<(Instance, Option<(String, String)>)> {
    let file = std::fs::File::open(crate::util::fs::long_path(archive))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| LauncherError::Zip(format!("не удалось открыть архив: {e}")))?;
    let kind = sniff(&mut zip)?;

    match kind {
        ArchiveKind::CurseForge => {
            // Прочитать manifest.json
            let mut manifest_text = None;
            for i in 0..zip.len() {
                let mut e = zip.by_index(i).map_err(|e| LauncherError::Zip(e.to_string()))?;
                if e.name().ends_with("manifest.json") && e.name().matches('/').count() <= 1 {
                    let mut s = String::new();
                    std::io::Read::read_to_string(&mut e, &mut s).ok();
                    manifest_text = Some(s);
                    break;
                }
            }
            let Some(text) = manifest_text else {
                return Err(LauncherError::InvalidInput("manifest.json не найден".into()));
            };
            let manifest: CfManifest = serde_json::from_str(&text)
                .map_err(|e| LauncherError::InvalidInput(format!("битый manifest.json: {e}")))?;
            let mut inst = Instance::new(name_override.unwrap_or(&manifest.name), &manifest.minecraft.version);
            crate::instances::save(paths, &inst)?;

            // overrides/ поверх каталога игры (zip-slip защищён).
            let game_dir = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
            let extracted = crate::util::zip::extract_zip_stripping(
                std::fs::File::open(crate::util::fs::long_path(archive))?,
                &game_dir,
                &format!("{}/", manifest.overrides),
                |_| true,
                |_| {},
            )?;
            tracing::info!("CF-zip: {} файлов overrides", extracted.len());

            // Загрузчик из manifest (id = "forge-47.3.0" / "fabric-0.15.11").
            let mut loader = None;
            for ml in &manifest.minecraft.modloaders {
                let (kind, ver) = ml.id.split_once('-').ok_or_else(|| {
                    LauncherError::InvalidInput(format!("битый modloader id: {}", ml.id))
                })?;
                loader = Some((kind.to_string(), ver.to_string()));
            }

            // Файлы модпака: без ключа CF — список ссылок в notes инстанса.
            if !manifest.files.is_empty() {
                let links = cf_missing_files_links(&client, &manifest.files).await;
                inst.notes = format!(
                    "Файлы CurseForge для ручного скачивания (нет API-ключа):\n{}",
                    links.join("\n")
                );
                crate::instances::save(paths, &inst)?;
            }
            Ok((inst, loader))
        }
        ArchiveKind::MultiMc => {
            // mmc-pack.json → mc + loader; minecraft/ → наш каталог игры.
            let mut pack_text = None;
            for i in 0..zip.len() {
                let mut e = zip.by_index(i).map_err(|e| LauncherError::Zip(e.to_string()))?;
                if e.name().ends_with("mmc-pack.json") {
                    let mut s = String::new();
                    std::io::Read::read_to_string(&mut e, &mut s).ok();
                    pack_text = Some(s);
                    break;
                }
            }
            let Some(text) = pack_text else {
                return Err(LauncherError::InvalidInput("mmc-pack.json не найден".into()));
            };
            let pack: MmcPack = serde_json::from_str(&text)
                .map_err(|e| LauncherError::InvalidInput(format!("битый mmc-pack.json: {e}")))?;
            let mc = pack
                .components
                .iter()
                .find(|c| c.uid == "net.minecraft")
                .and_then(|c| c.version.clone())
                .ok_or_else(|| LauncherError::InvalidInput("в mmc-pack нет версии Minecraft".into()))?;
            let loader = pack
                .components
                .iter()
                .find_map(|c| loader_from_uid(&c.uid, c.version.as_deref()));
            let inst = Instance::new(name_override.unwrap_or("Импорт"), &mc);
            crate::instances::save(paths, &inst)?;

            let game_dir = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
            // Содержимое minecraft/ (в zip это обычно `<root>/minecraft/…`).
            std::fs::File::open(crate::util::fs::long_path(archive))?.rewind()?;
            let extracted = crate::util::zip::extract_zip_filtered(
                std::fs::File::open(crate::util::fs::long_path(archive))?,
                &game_dir,
                |n| {
                    let lower = n.to_lowercase();
                    (lower.contains("minecraft/") || lower.contains(".minecraft/"))
                        && !n.ends_with('/')
                },
                |_| {},
            )?;
            tracing::info!("MultiMC: {} файлов minecraft/", extracted.len());
            Ok((inst, loader))
        }
        ArchiveKind::Own => {
            // Свой формат: instance.json + minecraft/.
            let mut inst_text = None;
            for i in 0..zip.len() {
                let mut e = zip.by_index(i).map_err(|e| LauncherError::Zip(e.to_string()))?;
                if e.name() == "instance.json" {
                    let mut s = String::new();
                    std::io::Read::read_to_string(&mut e, &mut s).ok();
                    inst_text = Some(s);
                    break;
                }
            }
            let Some(text) = inst_text else {
                return Err(LauncherError::InvalidInput("instance.json не найден".into()));
            };
            let src: Instance = serde_json::from_str(&text)
                .map_err(|e| LauncherError::InvalidInput(format!("битый instance.json: {e}")))?;
            let mut inst = Instance::new(name_override.unwrap_or(&src.name), &src.mc_version);
            inst.loader = src.loader.clone();
            inst.loader_version = src.loader_version.clone();
            inst.version_id = src.version_id.clone();
            crate::instances::save(paths, &inst)?;

            let game_dir = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
            std::fs::File::open(crate::util::fs::long_path(archive))?.rewind()?;
            let extracted = crate::util::zip::extract_zip_filtered(
                std::fs::File::open(crate::util::fs::long_path(archive))?,
                &game_dir,
                |n| n.starts_with("minecraft/") && !n.ends_with('/'),
                |_| {},
            )?;
            tracing::info!("own-zip: {} файлов", extracted.len());
            Ok((inst, None))
        }
    }
}

use std::io::Seek as _;
