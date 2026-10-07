//! Импорт (спека §6.11): CF-zip, MultiMC/Prism zip, свой формат.
//! Чужие архивы — недоверенный вход: zip-slip-защита в util::zip.

use crate::errors::{LauncherError, Result};
use crate::instances::Instance;
use crate::paths::Paths;
use serde::Deserialize;
use std::io::Read as _;
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

/// Кап на текстовый манифест из чужого zip: 16 МБ (D23). Манифесты модпаков —
/// килобайты; больше — либо битый файл, либо попытка съесть память ядра.
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

/// Прочитать текстовый манифест из записи архива. Заявленный распакованный
/// размер проверяем ДО чтения в память: чужой zip может объявить гигабайтный
/// manifest.json и уронить ядро по OOM (D23).
fn read_manifest<R: std::io::Read + ?Sized>(
    entry: &mut zip::read::ZipFile<'_, R>,
) -> Result<String> {
    let size = entry.size();
    if size > MAX_MANIFEST_BYTES {
        return Err(LauncherError::InvalidInput(format!(
            "манифест слишком большой: {size} байт (лимит {} МБ)",
            MAX_MANIFEST_BYTES / (1024 * 1024)
        )));
    }
    let mut s = String::new();
    entry.read_to_string(&mut s)?;
    Ok(s)
}

/// Ошибка фоновой задачи (аналог `join_err` в commands): поток с импортом
/// упал или запаниковал.
fn join_err(e: tauri::Error) -> LauncherError {
    LauncherError::Io(std::io::Error::other(format!("фоновая задача импорта: {e}")))
}

/// Откат несостоявшегося импорта — локальный аналог `rollback_instance` в
/// modrinth/mrpack.rs (A29): на момент сбоя инстанс уже сохранён (запись в
/// списке — это каталог `instances/<id>/instance.json`), но контента в нём
/// нет, и сбой оставил бы «призрак» — битый инстанс навсегда. Каталог
/// удаляем целиком; ошибку удаления только логируем — наружу идёт исходная
/// причина сбоя.
fn rollback_instance(paths: &Paths, id: &str) {
    let dir = crate::instances::instance_dir(paths, id);
    if let Err(e) = std::fs::remove_dir_all(crate::util::fs::long_path(&dir)) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("откат инстанса {id}: {} не удалён: {e}", dir.display());
        }
    }
}

/// Все шаги импорта ПОСЛЕ `instances::save` (распаковка, оверрайды, разбор
/// загрузчика): при ошибке откатываем инстанс-«призрак» и возвращаем
/// исходную ошибку (A29). Сам `instances::save` не оборачивается: если
/// сохранилось, инстанс есть; если нет — и откатывать нечего.
fn rollback_on_err<T>(paths: &Paths, inst: &Instance, r: Result<T>) -> Result<T> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => {
            tracing::warn!("импорт не удался ({e}) — откатываю инстанс");
            rollback_instance(paths, &inst.id);
            Err(e)
        }
    }
}

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

/// Разбор `modLoaders[].id` из CF manifest.json (D64). Реальные форматы id
/// (сверено с packwiz и Prism): «fabric-0.15.11», «quilt-0.21.0» (в т.ч.
/// беты «quilt-0.26.1-beta.2»), «forge-47.3.0», но также «forge-1.16.5-36.2.39»
/// и «neoforge-1.20.1-47.1.106» — CF вклеивает версию MC в середину, и её
/// надо срезать (то же делает packwiz), иначе в установку уезжает мусорная
/// версия. Вид загрузчика — сегмент до первого дефиса, строго из известного
/// списка; числовая версия — хвост после дефиса. Прежний разбор через
/// `split_once` без среза MC-префикса ломал forge/neoforge-паки, а quilt
/// проходил лишь пока версия не содержала дефисов. Неизвестный вид — честная
/// ошибка вместо тихого прохода в install_loader.
fn parse_cf_modloader(id: &str) -> Result<(String, String)> {
    let (kind, ver) = id
        .split_once('-')
        .ok_or_else(|| LauncherError::InvalidInput(format!("битый modloader id: {id}")))?;
    if !matches!(kind, "fabric" | "quilt" | "forge" | "neoforge") {
        return Err(LauncherError::InvalidInput(format!(
            "неизвестный загрузчик в manifest.json: {kind} (ожидали fabric|quilt|forge|neoforge)"
        )));
    }
    // forge-семейство кладёт версию MC в середину id (forge-1.16.5-36.2.39):
    // реальная версия загрузчика — хвост после сегмента, похожего на версию MC
    // (начинается с «1.»; сами версии forge/neoforge так не начинаются).
    let ver = if matches!(kind, "forge" | "neoforge") {
        match ver.split_once('-') {
            Some((head, rest)) if head.starts_with("1.") && !rest.is_empty() => rest,
            _ => ver,
        }
    } else {
        ver
    };
    if ver.is_empty() {
        return Err(LauncherError::InvalidInput(format!(
            "битый modloader id (пустая версия): {id}"
        )));
    }
    Ok((kind.to_string(), ver.to_string()))
}

#[derive(Deserialize)]
#[derive(Debug)]
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

/// Импорт архива в новый инстанс: создаёт Instance, распаковывает игровое
/// содержимое и ставит распознанный загрузчик — D62: установка загрузчика
/// живёт ВНУТРИ этой транзакции (сбой откатывает импорт целиком; офлайн при
/// требуемом загрузчике — честный OfflineMode-отказ с тем же откатом, D64).
/// Возвращает (инстанс, подсказка по загрузчику).
///
/// Тяжёлая синхронная часть (zip на тысячи файлов) уезжает в blocking-пул:
/// в async-контексте она фризила бы рантайм IPC вместе с окном (D14). Сеть
/// (ссылки на файлы CF, загрузчик) остаётся в async.
pub async fn import_archive(
    paths: &Paths,
    settings: &crate::settings::Settings,
    client: std::sync::Arc<crate::net::http::HttpClient>,
    archive: &Path,
    name_override: Option<&str>,
) -> Result<(Instance, Option<(String, String)>)> {
    let paths = paths.clone();
    let archive = archive.to_path_buf();
    let name_override = name_override.map(str::to_string);
    let save_paths = paths.clone();
    let (mut inst, loader, cf_files) = tauri::async_runtime::spawn_blocking(move || {
        import_archive_blocking(&paths, &archive, name_override.as_deref())
    })
    .await
    .map_err(join_err)??;

    // Файлы модпака из CF: без ключа API — список ссылок в notes инстанса.
    if !cf_files.is_empty() {
        let links = cf_missing_files_links(&client, &cf_files).await;
        inst.notes = format!(
            "Файлы CurseForge для ручного скачивания (нет API-ключа):\n{}",
            links.join("\n")
        );
        crate::instances::save(&save_paths, &inst)?;
    }

    // D62: загрузчик ставится под тем же откатом A29, что и распаковка.
    // Раньше его ставил вызывающий ПОСЛЕ import_archive: сбой install_loader
    // оставлял сохранённый инстанс при Err — повторный импорт плодил дубль
    // с копией контента.
    if let Some((kind, ver)) = &loader {
        if client.offline() {
            // D64: офлайн + пак требует загрузчика — честный отказ с полным
            // откатом (A29), а не «частичная установка под видом успеха»:
            // прежде инстанс без загрузчика уходил наружу как успех, и
            // пользователь получал молча нерабочий сбор. Установщика из кэша
            // нет: install_loader всегда ходит в сеть (manifest/meta/инсталлятор).
            let err = LauncherError::OfflineMode(format!(
                "для импорта «{}» нужен установчик {kind} — включи сеть и повтори импорт",
                inst.name
            ));
            return rollback_on_err(&save_paths, &inst, Err(err));
        } else {
            // paths уже уехал в spawn_blocking — жив только его клон save_paths.
            let installed = crate::instances::run::install_loader(
                &save_paths,
                settings,
                client.clone(),
                &inst,
                kind,
                Some(ver.as_str()),
            )
            .await;
            // Err — откат инстанса-«призрака», наружу исходная причина сбоя;
            // Ok — обновлённый (с загрузчиком) инстанс.
            let inst = rollback_on_err(&save_paths, &inst, installed)?;
            return Ok((inst, loader));
        }
    }
    Ok((inst, loader))
}

/// Разбор и распаковка архива — только синхронный IO (зовётся из
/// `import_archive` через spawn_blocking). Ссылки на файлы CF отдаём наружу:
/// их список собирает сетевой код уже в async.
#[allow(clippy::type_complexity)]
fn import_archive_blocking(
    paths: &Paths,
    archive: &Path,
    name_override: Option<&str>,
) -> Result<(Instance, Option<(String, String)>, Vec<CfFileRef>)> {
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
                    manifest_text = Some(read_manifest(&mut e)?);
                    break;
                }
            }
            let Some(text) = manifest_text else {
                return Err(LauncherError::InvalidInput("manifest.json не найден".into()));
            };
            let manifest: CfManifest = serde_json::from_str(&text)
                .map_err(|e| LauncherError::InvalidInput(format!("битый manifest.json: {e}")))?;
            let inst = Instance::new(name_override.unwrap_or(&manifest.name), &manifest.minecraft.version);
            crate::instances::save(paths, &inst)?;

            // Всё ПОСЛЕ save: сбой распаковки/загрузчика не должен оставлять
            // сохранённый инстанс-«призрак» в списке (A29).
            let post_save = || -> Result<(Option<(String, String)>, Vec<CfFileRef>)> {
                // overrides/ поверх каталога игры (zip-slip защищён).
                // D62: `"overrides": ""` в манифесте давал strip-префикс "/",
                // под который не матчится ни одна запись архива → тихий пустой
                // импорт без ошибок. Пустая (после trim) строка = дефолтный
                // каталог "overrides", как при отсутствующем поле.
                let overrides_dir = if manifest.overrides.trim().is_empty() {
                    "overrides"
                } else {
                    manifest.overrides.as_str()
                };
                let game_dir = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
                let extracted = crate::util::zip::extract_zip_stripping(
                    std::fs::File::open(crate::util::fs::long_path(archive))?,
                    &game_dir,
                    &format!("{overrides_dir}/"),
                    |_| true,
                    |_| {},
                )?;
                tracing::info!("CF-zip: {} файлов overrides", extracted.len());

                // Загрузчик из manifest — вида "fabric-0.15.11", "quilt-0.21.0",
                // "forge-1.16.5-36.2.39" и т.д. (см. parse_cf_modloader).
                let mut loader = None;
                for ml in &manifest.minecraft.modloaders {
                    loader = Some(parse_cf_modloader(&ml.id)?);
                }
                // Список файлов модпака — наружу: ссылки собирает async-часть.
                Ok((loader, manifest.files))
            };
            let (loader, files) = rollback_on_err(paths, &inst, post_save())?;
            Ok((inst, loader, files))
        }
        ArchiveKind::MultiMc => {
            // mmc-pack.json → mc + loader; minecraft/ → наш каталог игры.
            let mut pack_text = None;
            for i in 0..zip.len() {
                let mut e = zip.by_index(i).map_err(|e| LauncherError::Zip(e.to_string()))?;
                if e.name().ends_with("mmc-pack.json") {
                    pack_text = Some(read_manifest(&mut e)?);
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

            // Всё ПОСЛЕ save: сбой распаковки откатывает инстанс-«призрак» (A29).
            let post_save = || -> Result<()> {
                let game_dir = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
                // Содержимое minecraft/ (в zip это `<root>/minecraft/…` или
                // `<root>/.minecraft/…`) — стриппинг срезает префикс, иначе
                // файлы ложатся в minecraft/minecraft/ (двойная вложенность).
                std::fs::File::open(crate::util::fs::long_path(archive))?.rewind()?;
                let extracted = crate::util::zip::extract_zip_stripping(
                    std::fs::File::open(crate::util::fs::long_path(archive))?,
                    &game_dir,
                    "minecraft/",
                    |_| true,
                    |_| {},
                )?;
                let extracted2 = crate::util::zip::extract_zip_stripping(
                    std::fs::File::open(crate::util::fs::long_path(archive))?,
                    &game_dir,
                    ".minecraft/",
                    |_| true,
                    |_| {},
                )?;
                tracing::info!("MultiMC: {} файлов minecraft/", extracted.len() + extracted2.len());
                Ok(())
            };
            rollback_on_err(paths, &inst, post_save())?;
            Ok((inst, loader, Vec::new()))
        }
        ArchiveKind::Own => {
            // Свой формат: instance.json + minecraft/.
            let mut inst_text = None;
            for i in 0..zip.len() {
                let mut e = zip.by_index(i).map_err(|e| LauncherError::Zip(e.to_string()))?;
                if e.name() == "instance.json" {
                    inst_text = Some(read_manifest(&mut e)?);
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

            // Всё ПОСЛЕ save: сбой распаковки откатывает инстанс-«призрак» (A29).
            let post_save = || -> Result<()> {
                let game_dir = crate::instances::minecraft_dir(&crate::instances::instance_dir(paths, &inst.id));
                std::fs::File::open(crate::util::fs::long_path(archive))?.rewind()?;
                // Стриппинг: `minecraft/<x>` → `<x>` в корне каталога игры
                // (иначе двойная вложенность minecraft/minecraft).
                let extracted = crate::util::zip::extract_zip_stripping(
                    std::fs::File::open(crate::util::fs::long_path(archive))?,
                    &game_dir,
                    "minecraft/",
                    |_| true,
                    |_| {},
                )?;
                tracing::info!("own-zip: {} файлов", extracted.len());
                Ok(())
            };
            rollback_on_err(paths, &inst, post_save())?;
            Ok((inst, None, Vec::new()))
        }
    }
}

use std::io::Seek as _;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// Собрать zip в памяти (стиль util::zip::tests): недоверенный вход
    /// воспроизводим без сети и без настоящего модпака.
    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::SimpleFileOptions = Default::default();
            for (name, data) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(data).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    fn test_paths(dir: &tempfile::TempDir) -> Paths {
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        paths
    }

    /// D23: гигантский манифест из чужого zip отклоняется по размеру ДО чтения
    /// в память. Проверяем все три точки чтения манифеста: CF, MultiMC, свой.
    #[test]
    fn giant_manifest_rejected_by_size() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);

        // 17 МБ «манифеста»: валидный по форме, но JSON-парсер до него не дойдёт.
        let mut giant = String::from(r#"{"name":"big","minecraft":{"version":"1.20.1"},"x":""#);
        giant.push_str(&"x".repeat(17 * 1024 * 1024));
        giant.push_str("\"}");

        for name in ["manifest.json", "mmc-pack.json", "instance.json"] {
            let zip_path = dir.path().join(format!("{name}.zip"));
            std::fs::write(&zip_path, build_zip(&[(name, giant.as_bytes())])).unwrap();
            let Err(err) = import_archive_blocking(&paths, &zip_path, None) else {
                panic!("{name}: гигантский манифест должен быть отклонён");
            };
            assert_eq!(err.code(), "invalid_input", "{name}");
            assert!(
                err.to_string().contains("манифест слишком большой"),
                "{name}: {err}"
            );
        }
    }

    /// Свой формат: instance.json + minecraft/ распаковываются в каталог игры,
    /// остальное из архива не тащим.
    #[test]
    fn own_zip_imports_minecraft_tree() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let inst_json = r#"{"id":"src","name":"Из архива","mcVersion":"1.20.1","loader":"fabric","loaderVersion":"0.15.11"}"#;
        let zip_path = dir.path().join("own.zip");
        std::fs::write(
            &zip_path,
            build_zip(&[
                ("instance.json", inst_json.as_bytes()),
                ("minecraft/mods/a.jar", b"AAA"),
                ("minecraft/config/x.txt", b"X"),
                ("readme.txt", b"junk"),
            ]),
        )
        .unwrap();

        let (inst, loader, cf_files) =
            import_archive_blocking(&paths, &zip_path, Some("Своё имя")).unwrap();
        assert_eq!(inst.name, "Своё имя");
        assert_eq!(inst.mc_version, "1.20.1");
        assert_eq!(inst.loader.as_deref(), Some("fabric"));
        assert!(loader.is_none(), "в своём формате загрузчик не подсказываем");
        assert!(cf_files.is_empty());

        let mc = crate::instances::minecraft_dir(&crate::instances::instance_dir(&paths, &inst.id));
        // Раскладка фиксирована: префикс minecraft/ срезается — никакой
        // двойной вложенности minecraft/minecraft (дефект найден 2026-09-27).
        assert!(!mc.join("minecraft").exists(), "двойная вложенность запрещена");
        assert_eq!(read_named(&mc, "a.jar").as_deref(), Some(&b"AAA"[..]));
        assert_eq!(read_named(&mc, "x.txt").as_deref(), Some(&b"X"[..]));
        assert!(read_named(&mc, "readme.txt").is_none(), "чужой файл не распаковываем");
    }

    /// D14: публичный async-путь зовёт распаковку через blocking-пул — тот же
    /// результат, что и у синхронного ядра, но рантайм IPC не фризится.
    /// Сеть не нужна: свой формат + локальный архив.
    #[tokio::test]
    async fn async_import_goes_through_blocking_pool() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let inst_json = r#"{"id":"src2","name":"Async","mcVersion":"1.19.2"}"#;
        let zip_path = dir.path().join("async.zip");
        std::fs::write(
            &zip_path,
            build_zip(&[
                ("instance.json", inst_json.as_bytes()),
                ("minecraft/mods/b.jar", b"BBB"),
            ]),
        )
        .unwrap();

        let client = std::sync::Arc::new(crate::net::http::HttpClient::new(None).unwrap());
        let (inst, loader) = import_archive(
            &paths,
            &crate::settings::Settings::default(),
            client,
            &zip_path,
            None,
        )
        .await
        .unwrap();
        assert_eq!(inst.name, "Async");
        assert_eq!(inst.mc_version, "1.19.2");
        assert!(loader.is_none());
        assert!(crate::instances::load(&paths, &inst.id).is_ok(), "инстанс сохранён");
        assert!(
            read_named(&crate::instances::instance_dir(&paths, &inst.id), "b.jar").is_some(),
            "файлы архива распакованы"
        );
    }

    /// Содержимое файла с таким именем где угодно под `dir` (обход дерева).
    fn read_named(dir: &std::path::Path, name: &str) -> Option<Vec<u8>> {
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(data) = read_named(&p, name) {
                    return Some(data);
                }
            } else if p.file_name().is_some_and(|n| n == name) {
                return std::fs::read(&p).ok();
            }
        }
        None
    }

    /// Каталоги в `instances/` — «инстанс-призрак» виден именно здесь.
    fn instance_dirs(paths: &Paths) -> Vec<String> {
        let mut dirs: Vec<String> =
            std::fs::read_dir(crate::util::fs::long_path(&paths.instances_dir()))
                .map(|it| {
                    it.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
        dirs.sort();
        dirs
    }

    /// A29-регресс: сбой распаковки ПОСЛЕ instances::save не должен оставлять
    /// инстанс-«призрак» (битый, без контента, навсегда в списке). zip-slip-
    /// запись внутри overrides/ либо minecraft/ роняет распаковку без сети —
    /// для всех трёх импортёров: каталог и запись инстанса откатываются,
    /// наружу уходит исходная ошибка.
    #[test]
    fn failed_extraction_rolls_back_ghost_instance() {
        for (manifest_name, manifest_body, slip_entry) in [
            (
                "manifest.json",
                r#"{"name":"CF","minecraft":{"version":"1.20.1"},"files":[]}"#,
                "overrides/../evil.txt",
            ),
            (
                "mmc-pack.json",
                r#"{"components":[{"uid":"net.minecraft","version":"1.20.1"}]}"#,
                "minecraft/../evil.txt",
            ),
            (
                "instance.json",
                r#"{"id":"src3","name":"Ghost","mcVersion":"1.20.1"}"#,
                "minecraft/../evil.txt",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let paths = test_paths(&dir);
            let zip_path = dir.path().join(format!("{manifest_name}.zip"));
            std::fs::write(
                &zip_path,
                build_zip(&[(manifest_name, manifest_body.as_bytes()), (slip_entry, b"X")]),
            )
            .unwrap();

            let err = import_archive_blocking(&paths, &zip_path, None).unwrap_err();
            assert_eq!(err.code(), "zip_slip", "{manifest_name}: {err}");
            assert!(
                crate::instances::list(&paths).is_empty(),
                "{manifest_name}: инстанс-«призрак» не должен остаться в списке"
            );
            assert!(
                instance_dirs(&paths).is_empty(),
                "{manifest_name}: каталог несостоявшегося инстанса обязан быть удалён"
            );
        }
    }

    /// D62: пустой `"overrides": ""` в манифесте — не «тихий пустой импорт»
    /// (strip-префикс "/" не матчит ни одну запись архива), а дефолтный
    /// каталог "overrides": содержимое распаковывается как обычно.
    #[test]
    fn cf_manifest_with_empty_overrides_uses_default_dir() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let manifest =
            r#"{"name":"CF","minecraft":{"version":"1.20.1"},"overrides":"","files":[]}"#;
        let zip_path = dir.path().join("cf-empty-overrides.zip");
        std::fs::write(
            &zip_path,
            build_zip(&[
                ("manifest.json", manifest.as_bytes()),
                ("overrides/config/x.txt", b"X"),
                ("overrides/mods/a.jar", b"AAA"),
            ]),
        )
        .unwrap();

        let (inst, loader, cf_files) =
            import_archive_blocking(&paths, &zip_path, None).unwrap();
        assert!(loader.is_none(), "modLoaders нет — подсказки нет");
        assert!(cf_files.is_empty());
        let mc = crate::instances::minecraft_dir(&crate::instances::instance_dir(&paths, &inst.id));
        assert_eq!(
            read_named(&mc, "x.txt").as_deref(),
            Some(&b"X"[..]),
            "overrides/ при пустом поле распакованы по дефолтному каталогу"
        );
        assert!(read_named(&mc, "a.jar").is_some(), "{:?}", mc);
    }

    /// D64: офлайн-импорт CF-пака с загрузчиком — честный OfflineMode-отказ
    /// с полным откатом (A29), а не «частичная установка под видом успеха»
    /// (прежде: warn и инстанс без загрузчика, возвращённый как успех).
    #[tokio::test]
    async fn offline_import_with_loader_fails_and_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(&dir);
        let manifest = r#"{"name":"CF офлайн","minecraft":{"version":"1.20.1",
            "modLoaders":[{"id":"fabric-0.15.11"}]},"files":[]}"#;
        let zip_path = dir.path().join("cf-offline.zip");
        std::fs::write(
            &zip_path,
            build_zip(&[
                ("manifest.json", manifest.as_bytes()),
                ("overrides/mods/a.jar", b"AAA"),
            ]),
        )
        .unwrap();

        let client = std::sync::Arc::new(crate::net::http::HttpClient::new(None).unwrap());
        client.set_offline(true);
        let err = import_archive(
            &paths,
            &crate::settings::Settings::default(),
            client,
            &zip_path,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), "offline_mode", "{err}");
        let msg = err.to_string();
        assert!(msg.contains("CF офлайн"), "{msg}");
        assert!(msg.contains("установчик fabric"), "{msg}");
        assert!(
            crate::instances::list(&paths).is_empty(),
            "инстанс-«призрак» должен быть откатан"
        );
        assert!(
            instance_dirs(&paths).is_empty(),
            "каталог несостоявшегося импорта обязан быть удалён"
        );
    }

    /// D64: разбор modLoaders[].id — quilt/fabric/forge/neoforge, включая
    /// реальные варианты с версией MC в середине (packwiz issue #366) и
    /// беты quilt; неизвестный вид и мусор — ошибка.
    #[test]
    fn cf_modloader_ids_parse_to_known_kinds() {
        for (id, kind, ver) in [
            ("quilt-0.21.0", "quilt", "0.21.0"),
            ("quilt-0.26.1-beta.2", "quilt", "0.26.1-beta.2"),
            ("fabric-0.15.11", "fabric", "0.15.11"),
            ("forge-47.3.0", "forge", "47.3.0"),
            ("forge-1.16.5-36.2.39", "forge", "36.2.39"),
            ("neoforge-20.4.237", "neoforge", "20.4.237"),
            ("neoforge-1.20.1-47.1.106", "neoforge", "47.1.106"),
        ] {
            let parsed = parse_cf_modloader(id).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(parsed, (kind.to_string(), ver.to_string()), "{id}");
        }
        for bad in ["quilt", "rift-1.13", "forge-", ""] {
            assert!(
                parse_cf_modloader(bad).is_err(),
                "{bad:?} должен быть отклонён"
            );
        }
    }

    /// D64: CF-zip с quilt-загрузчиком — вид определяется как quilt (прежде
    /// quilt-паки с beta-версиями и forge/neoforge-паки с версией MC в id
    /// импортировались с битой версией), подсказка возвращается с чистым id.
    #[test]
    fn cf_zip_imports_quilt_and_mc_prefixed_loader_ids() {
        for (manifest, kind, ver) in [
            (
                r#"{"name":"Q","minecraft":{"version":"1.20.1","modLoaders":[{"id":"quilt-0.21.0"}]},"files":[]}"#,
                "quilt",
                "0.21.0",
            ),
            (
                r#"{"name":"F","minecraft":{"version":"1.20.1","modLoaders":[{"id":"forge-1.20.1-47.3.0"}]},"files":[]}"#,
                "forge",
                "47.3.0",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let paths = test_paths(&dir);
            let zip_path = dir.path().join("cf-loader.zip");
            std::fs::write(
                &zip_path,
                build_zip(&[
                    ("manifest.json", manifest.as_bytes()),
                    ("overrides/mods/a.jar", b"A"),
                ]),
            )
            .unwrap();
            let (inst, loader, cf_files) =
                import_archive_blocking(&paths, &zip_path, None).unwrap();
            assert!(cf_files.is_empty());
            assert_eq!(
                loader,
                Some((kind.to_string(), ver.to_string())),
                "пак {}: {inst:?}",
                inst.name
            );
        }
    }
}
