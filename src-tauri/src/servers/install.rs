//! Установка серверного ПО Minecraft в каталог (vanilla / Fabric / Forge /
//! NeoForge). Vanilla — `server.jar` из version JSON Mojang через доверенный
//! движок загрузок (sha1+size из манифеста обязательны); Fabric — бутстрап с
//! официального meta (сам доустанавливается при первом старте сервера);
//! Forge/NeoForge — официальный installer с maven (sha1-сайдкар) и headless
//! `--installServer`. Откат каталога при ошибке — забота вызывающего.

use crate::errors::{LauncherError, Result};
use crate::events::EventBus;
use crate::net::download::{DownloadEngine, DownloadTask};
use crate::net::http::HttpClient;
use crate::paths::Paths;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

/// Версия формата server-launcher у meta Fabric (конечный сегмент эндпоинта).
const FABRIC_SERVER_FORMAT: &str = "1.0.3";

/// Группа задач движка загрузок для server.jar (без префикса `instance:` —
/// у установки сервера нет инстанса-владельца).
const DOWNLOAD_GROUP: &str = "server-install";

/// Установить серверное ПО в `dir` (server.jar / бутстрап / библиотеки
/// лоадера). `loader`: None (или "vanilla") | "fabric" | "forge" |
/// "neoforge"; "quilt" — честный InvalidInput (серверов Quilt не делаем).
/// `progress` — человеческие статусы по ходу установки.
pub async fn install_server_software(
    paths: &Paths,
    http: &HttpClient,
    dir: &Path,
    mc_version: &str,
    loader: Option<&str>,
    loader_version: Option<&str>,
    progress: &(dyn Fn(String) + Send + Sync),
) -> Result<()> {
    let kind = resolve_kind(mc_version, loader, loader_version)?;
    std::fs::create_dir_all(crate::util::fs::long_path(dir))?;

    // Идемпотентность Forge/NeoForge: библиотеки лоадера уже стоят — Ok без
    // сети и без переустановки (проверка до любых сетевых вызовов).
    let pre_lib = match &kind {
        ServerKind::Forge(lv) => Some(InstallerKind::Forge.lib_rel(mc_version, lv)),
        ServerKind::NeoForge(lv) => Some(InstallerKind::NeoForge.lib_rel(mc_version, lv)),
        _ => None,
    };
    if let Some(rel) = &pre_lib {
        if loader_installed(dir, rel) {
            progress(format!("{} уже установлен", kind.label()));
            return Ok(());
        }
    }

    match &kind {
        ServerKind::Vanilla => install_vanilla(paths, http, dir, mc_version, progress).await,
        ServerKind::Fabric(lv) => {
            install_fabric(http, dir, mc_version, lv, progress).await
        }
        ServerKind::Forge(lv) => {
            install_loader_server(
                paths,
                http,
                dir,
                mc_version,
                lv,
                InstallerKind::Forge,
                progress,
            )
            .await
        }
        ServerKind::NeoForge(lv) => {
            install_loader_server(
                paths,
                http,
                dir,
                mc_version,
                lv,
                InstallerKind::NeoForge,
                progress,
            )
            .await
        }
    }
}

/// Переустановить серверное ПО: снести следы предыдущей установки и поставить
/// заново (маркеры «уже установлен» игнорируются принудительно). Чинит
/// побитые библиотеки лоадера и случай «установщик убит сразу после записи
/// win_args.txt». Вход валидируется как в install_server_software; удаления —
/// только внутри `dir` (плюс кэш установщика этой версии внутри cache_dir).
pub async fn reinstall_server_software(
    paths: &Paths,
    http: &HttpClient,
    dir: &Path,
    mc_version: &str,
    loader: Option<&str>,
    loader_version: Option<&str>,
    progress: &(dyn Fn(String) + Send + Sync),
) -> Result<()> {
    let kind = resolve_kind(mc_version, loader, loader_version)?;

    // Снос следов предыдущей установки. Пути собираются из констант модуля и
    // уже проверенных id — но перед любым удалением страхуемся «всё внутри
    // dir»: лексическое сравнение не разворачивает «..», поэтому запрет
    // таких компонент — в самом contained_in.
    let targets: Vec<PathBuf> = reinstall_targets(&kind)
        .into_iter()
        .map(|rel| dir.join(rel))
        .collect();
    if !targets.iter().all(|p| contained_in(dir, p)) {
        return Err(LauncherError::Internal(format!(
            "пути переустановки выходят за каталог сервера {}: {targets:?}",
            dir.display()
        )));
    }

    progress(format!("Переустанавливаю {}", kind.label()));
    progress(wipe_status(&kind).into());
    for path in targets {
        remove_target(path).await?;
    }

    // Кэш установщика этой версии тоже сносим: при недоступном .sha1 maven
    // download_installer берёт файл из кэша без сверки — побитый кэш иначе
    // пережил бы переустановку.
    if let Some(cache) = installer_cache_path(paths, &kind, mc_version) {
        if !contained_in(&paths.cache_dir(), &cache) {
            return Err(LauncherError::Internal(format!(
                "путь кэша установщика вне cache_dir: {}",
                cache.display()
            )));
        }
        remove_target(cache).await?;
    }

    // Штатная установка: маркеров после сноса нет — ставится заново
    // (vanilla получит sha1 из манифеста, Fabric — свежий бутстрап).
    install_server_software(paths, http, dir, mc_version, loader, loader_version, progress).await
}

/// Что переустановка сносит перед новой установкой — относительные пути
/// внутри каталога сервера (чистая функция: без сети и диска). Forge/NeoForge:
/// каталог группы лоадера в libraries ЦЕЛИКОМ — там только артефакты
/// установщика (внутри же лежит маркер win_args.txt, из-за которого «уже
/// установлен» держался вечно); Fabric: бутстрап + libraries (бутстрап
/// докачает библиотеки при первом старте); vanilla: server.jar.
fn reinstall_targets(kind: &ServerKind) -> Vec<PathBuf> {
    match kind {
        ServerKind::Vanilla => vec![PathBuf::from("server.jar")],
        ServerKind::Fabric(_) => vec![
            PathBuf::from("fabric-server-launch.jar"),
            PathBuf::from("libraries"),
        ],
        ServerKind::Forge(_) => vec![PathBuf::from("libraries")
            .join("net")
            .join("minecraftforge")],
        ServerKind::NeoForge(_) => vec![PathBuf::from("libraries")
            .join("net")
            .join("neoforged")],
    }
}

/// Человекочитаемый статус сноса для progress.
fn wipe_status(kind: &ServerKind) -> &'static str {
    match kind {
        ServerKind::Vanilla => "Удаляю старый server.jar",
        ServerKind::Fabric(_) => "Удаляю старый бутстрап и библиотеки",
        ServerKind::Forge(_) | ServerKind::NeoForge(_) => "Удаляю старые библиотеки лоадера",
    }
}

/// Кэш установщика лоадера в cache_dir/installers. Имена файлов обязаны
/// совпадать с приватными installer_path в loaders/forge.rs и
/// loaders/neoforge.rs. None — у плана нет установщика (vanilla/fabric).
fn installer_cache_path(paths: &Paths, kind: &ServerKind, mc: &str) -> Option<PathBuf> {
    let name = match kind {
        ServerKind::Forge(lv) => format!("forge-{mc}-{lv}-installer.jar"),
        ServerKind::NeoForge(lv) => format!("neoforge-{lv}-installer.jar"),
        _ => return None,
    };
    Some(paths.cache_dir().join("installers").join(name))
}

/// Путь остаётся внутри `base`? Страховка от выхода за каталог (удаления —
/// только внутри dir): «..»-компоненты запрещены вовсе (лексическое
/// `starts_with` их не разворачивает), чужие абсолютные пути отсекает
/// сравнение по компонентам после снятия long-префикса.
fn contained_in(base: &Path, path: &Path) -> bool {
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return false;
    }
    crate::util::fs::normalize_for_compare(path)
        .starts_with(crate::util::fs::normalize_for_compare(base))
}

/// Снести один целевой путь (файл или каталог). Отсутствующий — не ошибка
/// (переустановка идемпотентна); каталог libraries лоадера может быть
/// большим — удаление в spawn_blocking, tokio-воркер не блокируем.
async fn remove_target(path: PathBuf) -> Result<()> {
    let shown = path.display().to_string();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let long = crate::util::fs::long_path(&path);
        if long.is_dir() {
            match std::fs::remove_dir_all(&long) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            }
        } else {
            // remove_file_with_retry: ретраи Windows-локов, NotFound → Ok.
            crate::util::fs::remove_file_with_retry(&path)
        }
    })
    .await
    .map_err(|e| LauncherError::Internal(format!("join удаления {shown}: {e}")))?
}

/// Распознанный план установки (чистый разбор входа, без сети).
#[derive(Debug, PartialEq, Eq)]
enum ServerKind {
    Vanilla,
    /// Валидированная версия загрузчика Fabric.
    Fabric(String),
    /// Валидированная версия сборки Forge (полное имя — «mc-loader»).
    Forge(String),
    /// Валидированная версия NeoForge.
    NeoForge(String),
}

impl ServerKind {
    /// Имя для статусов и ошибок.
    fn label(&self) -> &'static str {
        match self {
            ServerKind::Vanilla => "Vanilla",
            ServerKind::Fabric(_) => "Fabric",
            ServerKind::Forge(_) => "Forge",
            ServerKind::NeoForge(_) => "NeoForge",
        }
    }
}

/// Семейство headless-инсталляторов (Forge / NeoForge).
#[derive(Debug, Clone, Copy)]
enum InstallerKind {
    Forge,
    NeoForge,
}

impl InstallerKind {
    fn label(self) -> &'static str {
        match self {
            InstallerKind::Forge => "Forge",
            InstallerKind::NeoForge => "NeoForge",
        }
    }

    /// Каталог библиотек лоадера внутри каталога сервера: туда `--installServer`
    /// пишет win_args.txt (unix_args.txt) — признак «уже установлено».
    fn lib_rel(self, mc: &str, lv: &str) -> PathBuf {
        match self {
            InstallerKind::Forge => PathBuf::from("libraries")
                .join("net")
                .join("minecraftforge")
                .join("forge")
                .join(format!("{mc}-{lv}")),
            InstallerKind::NeoForge => PathBuf::from("libraries")
                .join("net")
                .join("neoforged")
                .join("neoforge")
                .join(lv),
        }
    }
}

/// Разобрать (loader, loader_version) в план установки — чистая функция.
/// Сетка входа: неизвестные загрузчики и Quilt — InvalidInput; для лоадеров
/// обязательна версия; id версии MC и загрузчика проверяются на безопасный
/// набор символов (попадают в URL и имена файлов кэша).
fn resolve_kind(
    mc: &str,
    loader: Option<&str>,
    loader_version: Option<&str>,
) -> Result<ServerKind> {
    if !crate::loaders::is_safe_version_id(mc) {
        return Err(LauncherError::InvalidInput(format!(
            "некорректный id версии MC: {mc}"
        )));
    }
    if loader.is_none() && loader_version.is_some() {
        return Err(LauncherError::InvalidInput(
            "версия загрузчика указана без загрузчика".into(),
        ));
    }
    let need_version = |what: &str| -> Result<String> {
        let v = loader_version.ok_or_else(|| {
            LauncherError::InvalidInput(format!(
                "для сервера {what} нужно указать версию загрузчика"
            ))
        })?;
        if !crate::loaders::is_safe_version_id(v) {
            return Err(LauncherError::InvalidInput(format!(
                "некорректная версия загрузчика: {v}"
            )));
        }
        Ok(v.to_string())
    };
    match loader {
        None | Some("vanilla") => Ok(ServerKind::Vanilla),
        Some("fabric") => Ok(ServerKind::Fabric(need_version("Fabric")?)),
        Some("forge") => {
            if !mc_supports_forge_server(mc) {
                return Err(LauncherError::InvalidInput(
                    "серверы Forge поддерживаются с 1.17+".into(),
                ));
            }
            Ok(ServerKind::Forge(need_version("Forge")?))
        }
        Some("neoforge") => Ok(ServerKind::NeoForge(need_version("NeoForge")?)),
        Some("quilt") => Err(LauncherError::InvalidInput(
            "серверы Quilt не поддерживаются".into(),
        )),
        Some(other) => Err(LauncherError::InvalidInput(format!(
            "неизвестный загрузчик {other} (fabric|forge|neoforge)"
        ))),
    }
}

/// Forge-сервер поддерживается с 1.17: только с этой версии `--installServer`
/// оставляет win_args.txt (для старых — честный отказ до скачиваний). Версии
/// вне схемы «1.x» (новая нумерация) считаются поддерживаемыми.
fn mc_supports_forge_server(mc: &str) -> bool {
    let Some(rest) = mc.strip_prefix("1.") else {
        return true;
    };
    let minor = rest.split('.').next().unwrap_or("");
    match minor.parse::<u32>() {
        Ok(m) => m >= 17,
        Err(_) => false,
    }
}

/// URL installer.jar на официальном maven (та же координата, что качает
/// loaders/forge.rs для клиента).
fn forge_installer_url(mc: &str, lv: &str) -> String {
    let full = format!("{mc}-{lv}");
    format!(
        "{}/net/minecraftforge/forge/{full}/forge-{full}-installer.jar",
        crate::loaders::forge::MAVEN_BASE
    )
}

/// URL installer.jar на официальном maven (та же координата, что качает
/// loaders/neoforge.rs для клиента).
fn neoforge_installer_url(lv: &str) -> String {
    format!(
        "{}/releases/net/neoforged/neoforge/{lv}/neoforge-{lv}-installer.jar",
        crate::loaders::neoforge::MAVEN_BASE
    )
}

/// URL бутстрапа сервера Fabric: один файл, при первом старте он сам
/// докачивает ванильный сервер и библиотеки.
fn fabric_server_url(mc: &str, lv: &str) -> String {
    format!(
        "{}/versions/loader/{mc}/{lv}/{FABRIC_SERVER_FORMAT}/server/jar",
        crate::loaders::fabric::META_BASE
    )
}

/// Первый токен текста `.sha1` (maven пишет «<hex>  <имя>») в нижнем регистре.
/// В продукте парсинг сайдкара живёт внутри download_installer загрузчиков
/// (переиспользуется целиком) — здесь хелпер нужен для тестов формата.
#[cfg(test)]
fn parse_sha1_text(text: &str) -> Option<String> {
    let sha = text.split_whitespace().next()?.to_lowercase();
    (!sha.is_empty()).then_some(sha)
}

/// Идемпотентность файла: нет/нечитается — перекачать; известен ожидаемый
/// размер — совпадение решает; размер неизвестен — ненулевой файл годен
/// (fabric: перезакачка только если размер 0).
fn file_fresh(path: &Path, expected_size: Option<u64>) -> bool {
    let Ok(meta) = std::fs::metadata(crate::util::fs::long_path(path)) else {
        return false;
    };
    match expected_size {
        Some(expected) => meta.len() == expected,
        None => meta.len() > 0,
    }
}

/// Лоадер уже установлен в каталог сервера? Признак — win_args.txt или
/// unix_args.txt в каталоге библиотек (то, что оставляет `--installServer`).
fn loader_installed(dir: &Path, lib_rel: &Path) -> bool {
    let base = dir.join(lib_rel);
    ["win_args.txt", "unix_args.txt"].iter().any(|m| {
        crate::util::fs::long_path(&base.join(m)).is_file()
    })
}

/// Последние `n` строк лога — честный хвост вывода установщика в ошибке.
fn tail_lines(log: &str, n: usize) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

// ---------------------------------------------------------------- vanilla

#[derive(Deserialize)]
struct ServerArtifact {
    url: String,
    #[serde(default)]
    sha1: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Default, Deserialize)]
struct ServerDownloadsSection {
    #[serde(default)]
    server: Option<ServerArtifact>,
}

#[derive(Deserialize)]
struct VersionHead {
    #[serde(default)]
    downloads: ServerDownloadsSection,
}

/// `downloads.server` из version JSON. JSON качается заново: кэш манифестов
/// хранит разобранный VersionJson без поля server (модели запуска игры оно
/// не нужно), sha1 самого JSON сверяется с манифестом (спека §6.2).
async fn fetch_server_download(
    http: &HttpClient,
    entry: &crate::mojang::manifest::ManifestVersion,
) -> Result<Option<ServerArtifact>> {
    if http.offline() {
        return Err(LauncherError::OfflineMode(entry.url.clone()));
    }
    let resp = http.send_timed(http.raw().get(&entry.url)).await?;
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "HTTP {} для version JSON {}",
            resp.status(),
            entry.url
        )));
    }
    let bytes = http.body_timed(resp).await?;
    let actual = crate::util::fs::sha1_bytes(&bytes);
    if actual != entry.sha1 {
        return Err(LauncherError::HashMismatch {
            path: entry.url.clone(),
            expected: entry.sha1.clone(),
            actual,
        });
    }
    let head: VersionHead = serde_json::from_slice(&bytes)?;
    Ok(head.downloads.server)
}

/// Vanilla: manifest → version JSON → server.jar в `dir/server.jar`.
async fn install_vanilla(
    paths: &Paths,
    http: &HttpClient,
    dir: &Path,
    mc: &str,
    progress: &(dyn Fn(String) + Send + Sync),
) -> Result<()> {
    progress("Получаю манифест версий".into());
    let manifest = crate::mojang::manifest::fetch_manifest(http, &paths.manifests_cache()).await?;
    let entry = crate::mojang::manifest::resolve_entry(&manifest, mc)?;
    progress(format!("Читаю данные версии {mc}"));
    let dest = dir.join("server.jar");
    let server = match fetch_server_download(http, entry).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return Err(LauncherError::InvalidInput(format!(
                "для версии {mc} нет официального сервера"
            )));
        }
        // P2-ревизии: офлайн с уже скачанным (значит, проверенным при загрузке)
        // server.jar — принимаем файл, не требуем сеть ради манифеста.
        Err(e @ LauncherError::OfflineMode(_)) if file_fresh(&dest, None) => {
            tracing::warn!("офлайн: использую уже скачанный server.jar ({})", dest.display());
            let _ = e;
            progress("server.jar уже скачан".into());
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let sha1 = server.sha1.clone().ok_or_else(|| {
        LauncherError::InvalidInput(
            "у server.jar нет sha1 в манифесте — скачивание запрещено (спека §3)".into(),
        )
    })?;
    let dest = dir.join("server.jar");
    // P2-ревизии: совпавшего размера мало (битый сектор) — на fast-path
    // сверяем и sha1; хэш всё равно обязателен, чтение 47 МБ ~секунда.
    if file_fresh(&dest, server.size)
        && crate::util::fs::sha1_file(&crate::util::fs::long_path(&dest))
            .is_ok_and(|h| h.eq_ignore_ascii_case(&sha1))
    {
        progress("server.jar уже скачан".into());
        return Ok(());
    }
    progress("Качаю server.jar".into());
    download_via_engine(http, &server.url, &dest, &sha1, server.size).await?;
    progress("server.jar готов".into());
    Ok(())
}

/// Одно файловое скачивание доверенным движком: очередь, ретраи с backoff,
/// обязательная сверка sha1, атомарная запись через `.part`.
async fn download_via_engine(
    http: &HttpClient,
    url: &str,
    dest: &Path,
    sha1: &str,
    size: Option<u64>,
) -> Result<()> {
    let engine = DownloadEngine::new(std::sync::Arc::new(http.clone()), 2, EventBus::default());
    engine.add_tasks(vec![DownloadTask {
        id: format!(
            "server:{}",
            dest.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ),
        url: url.to_string(),
        dest: dest.to_path_buf(),
        sha1: Some(sha1.to_string()),
        sha512: None,
        size,
        group: DOWNLOAD_GROUP.into(),
        priority: 20,
    }]);
    engine.run().await?;
    let st = engine.queue_state();
    if st.failed > 0 {
        return Err(LauncherError::network(format!(
            "не удалось скачать {url}: {}",
            st.failed_items
                .first()
                .map(|(_, r)| r.clone())
                .unwrap_or_default()
        )));
    }
    // Отмена (группа/глобальная) — Cancelled, а не сетевая ошибка.
    if st.cancelled > 0 {
        return Err(LauncherError::Cancelled);
    }
    Ok(())
}

// ---------------------------------------------------------------- fabric

/// Fabric: один файл-бутстрап `fabric-server-launch.jar` с официального
/// meta.fabricmc.net. Отдельного `.sha1` у эндпоинта нет — источник тот же
/// официальный, что и профили клиента (loaders/fabric.rs); целостность
/// гарантирует HTTPS, размер проверяем > 0. Ничего больше не ставим:
/// бутстрап сам докачает ванильный сервер и библиотеки при первом старте.
async fn install_fabric(
    http: &HttpClient,
    dir: &Path,
    mc: &str,
    lv: &str,
    progress: &(dyn Fn(String) + Send + Sync),
) -> Result<()> {
    let dest = dir.join("fabric-server-launch.jar");
    if file_fresh(&dest, None) {
        progress("Бутстрап Fabric уже скачан".into());
        progress("Fabric доустановится при первом запуске".into());
        return Ok(());
    }
    progress("Качаю бутстрап Fabric".into());
    let url = fabric_server_url(mc, lv);
    if http.offline() {
        return Err(LauncherError::OfflineMode("загрузчик Fabric".into()));
    }
    let resp = http.send_timed(http.raw().get(&url)).await?;
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "HTTP {} для {url}",
            resp.status()
        )));
    }
    let bytes = http.body_timed(resp).await?;
    if bytes.is_empty() {
        return Err(LauncherError::network("пустой ответ для бутстрапа Fabric"));
    }
    crate::util::fs::atomic_write(&dest, &bytes)?;
    progress("Fabric доустановится при первом запуске".into());
    Ok(())
}

// ------------------------------------------------------- forge / neoforge

/// Java для headless-инсталлятора: тот же резолвинг, что у установки
/// загрузчиков клиенту (instances::run::resolve_java) — рантайм jdk{major}
/// по javaVersion версии MC, системные Java, затем Adoptium. Настройки
/// (прокси, доп. пути Java) читаются из стандартного settings.json.
/// Требуемый мажор Java для серверной версии: javaVersion из version JSON
/// (для снапшотов 26.x эвристика fallback_major занижает — ловили
/// UnsupportedClassVersionError на приёмке), fallback — старая схема.
pub(crate) async fn required_java_major(
    paths: &crate::paths::Paths,
    http: &crate::net::http::HttpClient,
    mc: &str,
) -> u32 {
    let Ok(manifest) =
        crate::mojang::manifest::fetch_manifest(http, &paths.manifests_cache()).await
    else {
        return crate::instances::run::fallback_major(mc);
    };
    let Ok(entry) = crate::mojang::manifest::resolve_entry(&manifest, mc) else {
        return crate::instances::run::fallback_major(mc);
    };
    crate::mojang::manifest::fetch_version_json_cached(http, entry, &paths.manifests_cache())
        .await
        .ok()
        .and_then(|v| v.java_version.as_ref().map(|j| j.major_version))
        .unwrap_or_else(|| crate::instances::run::fallback_major(mc))
}

async fn resolve_installer_java(
    paths: &Paths,
    http: &HttpClient,
    mc: &str,
) -> Result<crate::java::detect::JavaInstall> {
    let manifest = crate::mojang::manifest::fetch_manifest(http, &paths.manifests_cache()).await?;
    let entry = crate::mojang::manifest::resolve_entry(&manifest, mc)?;
    let parent =
        crate::mojang::manifest::fetch_version_json_cached(http, entry, &paths.manifests_cache())
            .await?;
    let need_major = parent
        .java_version
        .as_ref()
        .map(|j| j.major_version)
        .unwrap_or_else(|| crate::instances::run::fallback_major(mc));
    // P3-ревизии: битый settings.json не должен валить установку — прокси
    // некритичен, работаем на дефолтах (как resolve_java в run.rs).
    let settings = crate::settings::Settings::load(&paths.settings_file()).unwrap_or_default();
    crate::instances::run::resolve_java(
        paths,
        &settings,
        std::sync::Arc::new(http.clone()),
        need_major,
        &None,
        None,
    )
    .await
}

/// Forge/NeoForge: installer с maven (sha1-сайдкар — внутри download_installer
/// loaders, тот же артефакт, что и для клиента) → headless `--installServer`.
async fn install_loader_server(
    paths: &Paths,
    http: &HttpClient,
    dir: &Path,
    mc: &str,
    lv: &str,
    kind: InstallerKind,
    progress: &(dyn Fn(String) + Send + Sync),
) -> Result<()> {
    let label = kind.label();
    progress(format!("Качаю установщик {label}"));
    let installer = match kind {
        InstallerKind::Forge => {
            tracing::debug!("артефакт установщика: {}", forge_installer_url(mc, lv));
            crate::loaders::forge::download_installer(http, &paths.cache_dir(), mc, lv).await?
        }
        InstallerKind::NeoForge => {
            tracing::debug!("артефакт установщика: {}", neoforge_installer_url(lv));
            crate::loaders::neoforge::download_installer(http, &paths.cache_dir(), lv).await?
        }
    };
    let java = resolve_installer_java(paths, http, mc).await?;
    progress(format!("Устанавливаю {label}, это может занять несколько минут"));
    // Прокси из настроек в -D-флагах JVM инсталлятора (как у клиента).
    let jvm_extra = crate::loaders::installer_jvm_args(&paths.cache_dir());
    run_server_installer(
        java.java_exe.clone(),
        installer.clone(),
        dir.to_path_buf(),
        label,
        jvm_extra,
    )
    .await?;

    let lib_rel = kind.lib_rel(mc, lv);
    if !loader_installed(dir, &lib_rel) {
        return Err(LauncherError::Internal(format!(
            "установщик {label} завершился, но не оставил win_args.txt в {}",
            lib_rel.display()
        )));
    }
    // Установщик отработал — забираем его и из кэша (правило «installer.jar
    // после установки удалить»); при провале файл остаётся для повторной
    // попытки, откат каталога — забота вызывающего.
    let _ = crate::util::fs::remove_file_with_retry(&installer);
    progress(format!("{label} установлен"));
    Ok(())
}

/// Headless-инсталлятор сервера: `java -jar installer --installServer <dir>`.
/// Образец — neoforge::run_installer: shim launcher_profiles.json (инсталлятор
/// требует «вы запускали ванильный лаунчер»), дренаж пайпов в потоках,
/// hide_console, дедлайн INSTALLER_TIMEOUT с kill_tree по зависшему дереву.
fn run_server_installer_sync(
    java_exe: &Path,
    installer: &Path,
    dir: &Path,
    label: &str,
    jvm_extra: &[String],
) -> Result<()> {
    let profiles = dir.join("launcher_profiles.json");
    if !profiles.exists() {
        crate::util::fs::atomic_write(
            &profiles,
            br#"{"profiles": {}, "settings": {}, "version": 3}"#,
        )?;
    }
    let mut cmd = std::process::Command::new(java_exe);
    for arg in jvm_extra {
        cmd.arg(arg);
    }
    cmd.arg("-jar")
        .arg(crate::util::fs::long_path(installer))
        .arg("--installServer")
        .arg(crate::util::fs::long_path(dir))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // ENV-6: cwd получает Win32 (SetCurrentDirectoryW), `\\?\`-пути он не
        // принимает — префикс снимаем; аргументы идут файловыми API.
        .current_dir(crate::util::fs::strip_long_prefix(dir));
    crate::util::win::hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| LauncherError::Internal(format!("запуск инсталлятора {label}: {e}")))?;

    // Пайпы читаем в потоках: болтливый инсталлятор иначе завис на записи в
    // переполнившийся пайп (~64 КБ) до нашего дедлайна.
    use std::io::Read as _;
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout_pipe.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr_pipe.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });

    // Дедлайн с поллингом try_wait: зависший ребёнок убивается по дереву.
    let deadline = std::time::Instant::now() + crate::loaders::INSTALLER_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    crate::loaders::kill_tree(child.id());
                    let _ = child.wait();
                    return Err(LauncherError::Timeout(format!(
                        "установка {label}: инсталлятор завис и остановлен (нет завершения за {:?})",
                        crate::loaders::INSTALLER_TIMEOUT
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                crate::loaders::kill_tree(child.id());
                let _ = child.wait();
                return Err(LauncherError::Internal(format!(
                    "ожидание инсталлятора {label}: {e}"
                )));
            }
        }
    };
    let out_stdout = stdout_reader.join().unwrap_or_default();
    let out_stderr = stderr_reader.join().unwrap_or_default();
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&out_stdout),
        String::from_utf8_lossy(&out_stderr)
    );
    tracing::debug!("лог инсталлятора {label}:\n{}", log.trim_end());
    if !status.success() {
        return Err(LauncherError::Internal(format!(
            "инсталлятор {label} завершился с {status}:\n{}",
            tail_lines(&log, 15)
        )));
    }
    Ok(())
}

/// Тот же запуск в spawn_blocking под общим тайм-гвардом, что и у клиента
/// (loaders::run_installer_guarded): инсталлятор — синхронный процесс на
/// минуты, tokio-воркер блокировать нельзя.
async fn run_server_installer(
    java_exe: PathBuf,
    installer: PathBuf,
    dir: PathBuf,
    label: &'static str,
    jvm_extra: Vec<String>,
) -> Result<()> {
    let handle = tokio::task::spawn_blocking(move || {
        run_server_installer_sync(&java_exe, &installer, &dir, label, &jvm_extra)
    });
    match tokio::time::timeout(crate::loaders::INSTALLER_EXEC_GUARD, handle).await {
        Ok(joined) => {
            joined.map_err(|e| LauncherError::Internal(format!("join инсталлятора {label}: {e}")))?
        }
        Err(_) => Err(LauncherError::Timeout(format!(
            "установка {label}: инсталлятор завис и остановлен (нет завершения за {:?})",
            crate::loaders::INSTALLER_EXEC_GUARD
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --------------------------------------------------------- resolve_kind

    #[test]
    fn resolve_kind_accepts_vanilla_and_loaders() {
        // LauncherError не PartialEq — сверяемся через matches!/связывание.
        assert!(matches!(
            resolve_kind("1.20.1", None, None),
            Ok(ServerKind::Vanilla)
        ));
        assert!(matches!(
            resolve_kind("1.20.1", Some("vanilla"), None),
            Ok(ServerKind::Vanilla)
        ));
        assert!(matches!(
            resolve_kind("1.20.1", Some("fabric"), Some("0.15.11")),
            Ok(ServerKind::Fabric(v)) if v == "0.15.11"
        ));
        assert!(matches!(
            resolve_kind("1.20.1", Some("forge"), Some("47.2.0")),
            Ok(ServerKind::Forge(v)) if v == "47.2.0"
        ));
        assert!(matches!(
            resolve_kind("1.21.1", Some("neoforge"), Some("21.1.251")),
            Ok(ServerKind::NeoForge(v)) if v == "21.1.251"
        ));
    }

    #[test]
    fn resolve_kind_rejects_quilt_unknown_and_broken_input() {
        for (loader, lv) in [
            (Some("quilt"), Some("0.26.0")),
            (Some("weird"), Some("1.0")),
        ] {
            let err = resolve_kind("1.20.1", loader, lv).expect_err("должен быть отказ");
            assert!(
                matches!(err, LauncherError::InvalidInput(_)),
                "{loader:?}: ожидался InvalidInput, получен {err:?}"
            );
        }
        // Версия загрузчика обязательна для всех лоадеров.
        for loader in ["fabric", "forge", "neoforge"] {
            let err = resolve_kind("1.20.1", Some(loader), None).expect_err("нет версии");
            assert!(matches!(err, LauncherError::InvalidInput(_)));
        }
        // Версия без загрузчика — бессмыслица, честный отказ.
        let err = resolve_kind("1.20.1", None, Some("47.2.0")).expect_err("должен быть отказ");
        assert!(matches!(err, LauncherError::InvalidInput(_)));
    }

    /// Аудит 2026-10-06 (тот же принцип, что в loaders): id из удалённых API
    /// попадают в URL и имена файлов — разделители пути отклоняются до сети.
    #[test]
    fn resolve_kind_rejects_unsafe_ids() {
        let err = resolve_kind("../evil", None, None).expect_err("небезопасный id MC");
        assert!(matches!(err, LauncherError::InvalidInput(_)));
        let err = resolve_kind("1.20.1", Some("fabric"), Some("a/b")).expect_err("небезопасный lv");
        assert!(matches!(err, LauncherError::InvalidInput(_)));
    }

    /// Серверы Forge — только 1.17+ (win_args.txt появился там).
    #[test]
    fn resolve_kind_forge_gate_1_17() {
        let err = resolve_kind("1.16.5", Some("forge"), Some("36.2.39"))
            .expect_err("1.16.5 должен быть отклонён");
        assert!(err.to_string().contains("1.17"), "{err}");
        assert!(resolve_kind("1.17", Some("forge"), Some("37.0.0")).is_ok());
        assert!(resolve_kind("1.7.10", Some("forge"), Some("10.13.4")).is_err());
    }

    #[test]
    fn forge_gate_covers_new_version_scheme() {
        // Вне схемы «1.x» (новая нумерация) — считаем поддерживаемой.
        assert!(mc_supports_forge_server("26.3"));
        // «1.x» с мусором вместо минора — консервативный отказ.
        assert!(!mc_supports_forge_server("1.x"));
        // Точная граница: 1.17 — можно, 1.16.x — нет.
        assert!(mc_supports_forge_server("1.17"));
        assert!(mc_supports_forge_server("1.17.1"));
        assert!(!mc_supports_forge_server("1.16.5"));
    }

    // ------------------------------------------------------------- URL-ы

    #[test]
    fn installer_urls_match_maven_layout() {
        assert_eq!(
            forge_installer_url("1.20.1", "47.2.0"),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.2.0/forge-1.20.1-47.2.0-installer.jar"
        );
        assert_eq!(
            neoforge_installer_url("21.1.251"),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.251/neoforge-21.1.251-installer.jar"
        );
        assert_eq!(
            fabric_server_url("1.20.1", "0.15.11"),
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1/0.15.11/1.0.3/server/jar"
        );
    }

    // --------------------------------------------------------- sha1 / файлы

    #[test]
    fn parse_sha1_text_takes_first_token_lowercased() {
        let sha = "a".repeat(40);
        assert_eq!(parse_sha1_text(&format!("{sha}  forge-1.20.1-installer.jar")), Some(sha.clone()));
        assert_eq!(parse_sha1_text("  ABC\n"), Some("abc".into()));
        assert_eq!(parse_sha1_text(" \n\t"), None);
        assert_eq!(parse_sha1_text(""), None);
    }

    #[test]
    fn file_fresh_decides_by_size() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("server.jar");
        // Нет файла — перекачать.
        assert!(!file_fresh(&p, Some(3)));
        assert!(!file_fresh(&p, None));
        std::fs::write(&p, b"abc").unwrap();
        // Размер совпал — файл свежий; не совпал — перекачать.
        assert!(file_fresh(&p, Some(3)));
        assert!(!file_fresh(&p, Some(100)));
        // Размер неизвестен: ненулевой годен (fabric), пустой — нет.
        assert!(file_fresh(&p, None));
        let empty = dir.path().join("empty.jar");
        std::fs::write(&empty, b"").unwrap();
        assert!(!file_fresh(&empty, None), "нулевой размер — перезакачка");
    }

    // ------------------------------------------------- маркер установленности

    #[test]
    fn lib_rel_paths_follow_maven_layout() {
        assert_eq!(
            InstallerKind::Forge.lib_rel("1.20.1", "47.2.0"),
            PathBuf::from("libraries/net/minecraftforge/forge/1.20.1-47.2.0")
        );
        assert_eq!(
            InstallerKind::NeoForge.lib_rel("1.21.1", "21.1.251"),
            PathBuf::from("libraries/net/neoforged/neoforge/21.1.251")
        );
    }

    #[test]
    fn loader_installed_detects_win_and_unix_args() {
        let dir = tempfile::tempdir().unwrap();
        let rel = PathBuf::from("libraries/net/minecraftforge/forge/1.20.1-47.2.0");
        assert!(!loader_installed(dir.path(), &rel));
        std::fs::create_dir_all(dir.path().join(&rel)).unwrap();
        std::fs::write(dir.path().join(&rel).join("win_args.txt"), b"").unwrap();
        assert!(loader_installed(dir.path(), &rel));
        // Linux-случай: только unix_args.txt тоже считается установленным.
        std::fs::remove_file(dir.path().join(&rel).join("win_args.txt")).unwrap();
        std::fs::write(dir.path().join(&rel).join("unix_args.txt"), b"").unwrap();
        assert!(loader_installed(dir.path(), &rel));
        // Каталог есть, маркеров нет — не установлен.
        std::fs::remove_file(dir.path().join(&rel).join("unix_args.txt")).unwrap();
        assert!(!loader_installed(dir.path(), &rel));
    }

    // ------------------------------------------------------- переустановка

    /// Список сноса по вариантам: vanilla / fabric / группы лоадеров целиком.
    #[test]
    fn reinstall_targets_per_kind() {
        assert_eq!(
            reinstall_targets(&ServerKind::Vanilla),
            vec![PathBuf::from("server.jar")]
        );
        assert_eq!(
            reinstall_targets(&ServerKind::Fabric("0.15.11".into())),
            vec![
                PathBuf::from("fabric-server-launch.jar"),
                PathBuf::from("libraries"),
            ]
        );
        // Группа лоадера сносится ЦЕЛИКОМ, а не только каталог одной версии:
        // маркер мог остаться в каталоге другой сборки.
        assert_eq!(
            reinstall_targets(&ServerKind::Forge("47.2.0".into())),
            vec![PathBuf::from("libraries/net/minecraftforge")]
        );
        assert_eq!(
            reinstall_targets(&ServerKind::NeoForge("21.1.251".into())),
            vec![PathBuf::from("libraries/net/neoforged")]
        );
    }

    /// Инвариант безопасности: любые цели сноса остаются внутри каталога
    /// сервера — при любых валидных mc/loader_version.
    #[test]
    fn reinstall_targets_stay_inside_dir() {
        let dir = Path::new("/srv/mc");
        for kind in [
            ServerKind::Vanilla,
            ServerKind::Fabric("0.15.11".into()),
            ServerKind::Forge("47.2.0".into()),
            ServerKind::NeoForge("21.1.251".into()),
        ] {
            for rel in reinstall_targets(&kind) {
                assert!(
                    contained_in(dir, &dir.join(&rel)),
                    "{kind:?}: цель {rel:?} обязана остаться внутри dir"
                );
            }
        }
    }

    /// contained_in отсекает «..», чужие абсолютные пути и строковые
    /// префиксы («C:\srvx» не «внутри» «C:\srv»).
    #[test]
    fn contained_in_rejects_escape_attempts() {
        let dir = Path::new("C:\\srv");
        assert!(contained_in(dir, Path::new("C:\\srv\\server.jar")));
        assert!(contained_in(dir, Path::new("C:\\srv\\libraries\\net\\x")));
        assert!(!contained_in(
            dir,
            Path::new("C:\\srv\\..\\escape\\server.jar")
        ));
        assert!(!contained_in(dir, Path::new("C:\\other\\server.jar")));
        assert!(!contained_in(dir, Path::new("C:\\srvx")));
        assert!(!contained_in(dir, Path::new("D:\\srv\\server.jar")));
    }

    /// Кэш установщика: имена совпадают с приватными installer_path в
    /// loaders/{forge,neoforge}.rs, для vanilla/fabric его нет, и путь
    /// всегда внутри cache_dir.
    #[test]
    fn installer_cache_path_matches_loaders() {
        let paths = Paths::new(PathBuf::from(r"C:\data\mcl"));
        let f = installer_cache_path(&paths, &ServerKind::Forge("47.2.0".into()), "1.20.1")
            .expect("у Forge есть установщик");
        assert_eq!(
            f,
            PathBuf::from(r"C:\data\mcl\cache\installers\forge-1.20.1-47.2.0-installer.jar")
        );
        let n = installer_cache_path(
            &paths,
            &ServerKind::NeoForge("21.1.251".into()),
            "1.21.1",
        )
        .expect("у NeoForge есть установщик");
        assert_eq!(
            n,
            PathBuf::from(r"C:\data\mcl\cache\installers\neoforge-21.1.251-installer.jar")
        );
        assert!(installer_cache_path(&paths, &ServerKind::Vanilla, "1.20.1").is_none());
        assert!(installer_cache_path(
            &paths,
            &ServerKind::Fabric("0.15.11".into()),
            "1.20.1"
        )
        .is_none());
        assert!(contained_in(&paths.cache_dir(), &f));
        assert!(contained_in(&paths.cache_dir(), &n));
    }

    /// Переустановка валидирует вход тем же resolve_kind: malformed-версии —
    /// InvalidInput до любых удалений (и без сети).
    #[test]
    fn reinstall_input_validated_like_install() {
        for (mc, loader, lv) in [
            ("../evil", Some("fabric"), Some("0.15.11")),
            ("1.20.1", Some("neoforge"), Some("a/b")),
            ("1.20.1", Some("neoforge"), Some("")),
            ("1.20.1", Some("quilt"), Some("0.26.0")),
        ] {
            let err = resolve_kind(mc, loader, lv).expect_err("должен быть отказ");
            assert!(
                matches!(err, LauncherError::InvalidInput(_)),
                "{mc}/{loader:?}: ожидался InvalidInput, получен {err:?}"
            );
        }
    }

    // ----------------------------------------------------------- прочее

    #[test]
    fn tail_lines_returns_last_n() {
        let log = "l1\nl2\nl3";
        assert_eq!(tail_lines(log, 2), "l2\nl3");
        assert_eq!(tail_lines(log, 10), log);
        assert_eq!(tail_lines("", 3), "");
    }

    /// Разбор downloads.server из синтетического version JSON (ноль байтов
    /// ассетов Mojang): сервер есть / сервера нет / секции downloads нет.
    #[test]
    fn version_head_parses_server_section() {
        let with: VersionHead = serde_json::from_str(
            r#"{"id":"1.20.1","downloads":{"client":{"url":"c"},"server":{"url":"https://piston-data.mojang.com/s.jar","sha1":"aabb","size":123}}}"#,
        )
        .unwrap();
        let s = with.downloads.server.expect("server должен быть");
        assert_eq!(s.url, "https://piston-data.mojang.com/s.jar");
        assert_eq!(s.sha1.as_deref(), Some("aabb"));
        assert_eq!(s.size, Some(123));

        let without: VersionHead =
            serde_json::from_str(r#"{"id":"a1.0","downloads":{"client":{"url":"c"}}}"#).unwrap();
        assert!(without.downloads.server.is_none(), "старые alpha/beta без server");

        let bare: VersionHead = serde_json::from_str(r#"{"id":"x"}"#).unwrap();
        assert!(bare.downloads.server.is_none());
    }
}
