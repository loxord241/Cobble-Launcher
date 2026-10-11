//! Модель version JSON Mojang (спека §6.2) + inheritsFrom-резолвинг (§6.4).

use crate::errors::{LauncherError, Result};
use crate::mojang::rules::{evaluate, OsContext, Rule};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Ссылка на скачивание (клиент).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadInfo {
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

/// Артефакт библиотеки (url, path, sha1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibArtifact {
    pub url: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<LibArtifact>,
    #[serde(default)]
    pub classifiers: Option<BTreeMap<String, LibArtifact>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractRules {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub natives: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub extract: Option<ExtractRules>,
}

/// Элемент arguments — строка ИЛИ {rules, value} (спека §6.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgElem {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        value: ArgValue,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgElem>,
    #[serde(default)]
    pub jvm: Vec<ArgElem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(rename = "totalSize", default)]
    pub total_size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaVersionRef {
    #[serde(default)]
    pub component: Option<String>,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogFileRef {
    pub id: String,
    pub sha1: String,
    #[serde(default)]
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingClient {
    pub argument: String,
    pub file: LogFileRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingClient>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Downloads {
    #[serde(rename = "client", default)]
    pub client: Option<DownloadInfo>,
}

/// Version JSON (спека §6.2 — «знать наизусть»).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VersionJson {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default)]
    pub version_type: Option<String>,
    #[serde(rename = "mainClass", default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(rename = "assetIndex", default)]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub downloads: Downloads,
    #[serde(rename = "javaVersion", default)]
    pub java_version: Option<JavaVersionRef>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(rename = "minecraftArguments", default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub logging: Option<Logging>,
    #[serde(rename = "inheritsFrom", default)]
    pub inherits_from: Option<String>,
    #[serde(rename = "minimumLauncherVersion", default)]
    pub minimum_launcher_version: Option<u32>,
    #[serde(rename = "releaseTime", default)]
    pub release_time: Option<String>,
    /// Редкое поле: id клиента-jar отличается от id версии.
    #[serde(default)]
    pub jar: Option<String>,
}

/// Результат inheritsFrom-резолвинга: всё разрешено, готово к планированию.
#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub id: String,
    pub version_type: Option<String>,
    pub main_class: String,
    pub assets: String,
    pub asset_index: Option<AssetIndexRef>,
    pub client_download: Option<DownloadInfo>,
    pub java_version: Option<JavaVersionRef>,
    /// Новый формат аргументов (конкатенация родитель → ребёнок, спека §6.4).
    pub arguments_jvm: Vec<ArgElem>,
    pub arguments_game: Vec<ArgElem>,
    /// Старый формат (до 1.6): minecraftArguments.
    pub minecraft_arguments: Option<String>,
    pub libraries: Vec<Library>,
    pub logging: Option<Logging>,
    pub inherits_from: Option<String>,
}

pub const MAX_INHERIT_DEPTH: usize = 5;

impl VersionJson {
    /// Загрузить и распарсить JSON версии из файла.
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read(crate::util::fs::long_path(path))?;
        Ok(serde_json::from_slice(&data)?)
    }

    /// Скачать JSON версии с обязательной проверкой sha1 (спека §3, §6.2).
    pub async fn fetch(
        client: &crate::net::http::HttpClient,
        url: &str,
        expected_sha1: &str,
    ) -> Result<Self> {
        // F16: офлайн — версия не в кэше, качать нельзя: честный отказ.
        if client.offline() {
            return Err(LauncherError::OfflineMode(url.to_string()));
        }
        let resp = client.raw().get(url).send().await?;
        if !resp.status().is_success() {
            return Err(LauncherError::network(format!(
                "HTTP {} для version JSON {url}",
                resp.status()
            )));
        }
        let bytes = resp.bytes().await?;
        let actual = crate::util::fs::sha1_bytes(&bytes);
        if actual != expected_sha1 {
            return Err(LauncherError::HashMismatch {
                path: url.into(),
                expected: expected_sha1.into(),
                actual,
            });
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Разрешить цепочку inheritsFrom (макс. 5 уровней, защита от циклов — §6.4).
    /// Родительские JSON ищутся в `versions_dirs` по порядку (инстанс → кэш).
    pub fn resolve_chain_dirs(versions_dirs: &[PathBuf], root: &VersionJson) -> Result<ResolvedVersion> {
        let mut chain: Vec<VersionJson> = vec![root.clone()];
        let mut visited: Vec<String> = vec![root.id.clone()];
        while let Some(parent_id) = chain
            .last()
            .and_then(|current| current.inherits_from.clone())
        {
            if visited.contains(&parent_id) {
                return Err(LauncherError::InvalidInput(format!(
                    "цикл inheritsFrom: {parent_id}"
                )));
            }
            if chain.len() >= MAX_INHERIT_DEPTH {
                return Err(LauncherError::InvalidInput(
                    "слишком глубокая цепочка inheritsFrom (>5)".into(),
                ));
            }
            let mut parent = None;
            for dir in versions_dirs {
                let path = dir.join(format!("{parent_id}.json"));
                if path.exists() {
                    parent = Some(VersionJson::load(&path)?);
                    break;
                }
            }
            let parent = parent.ok_or_else(|| {
                LauncherError::VersionNotFound(format!(
                    "родительская версия {parent_id} не найдена в кэше"
                ))
            })?;
            visited.push(parent_id);
            chain.push(parent);
        }
        let refs: Vec<&VersionJson> = chain.iter().collect();
        // mainClass обязателен в итоге
        let acc = merge_chain(refs)?;
        if acc.main_class.is_empty() {
            return Err(LauncherError::InvalidInput(format!(
                "в версии {} нет mainClass",
                acc.id
            )));
        }
        Ok(acc)
    }

    /// Разрешить цепочку в одном каталоге (совместимость).
    pub fn resolve_chain(versions_dir: &Path, root: &VersionJson) -> Result<ResolvedVersion> {
        Self::resolve_chain_dirs(std::slice::from_ref(&versions_dir.to_path_buf()), root)
    }
}

/// Ключ дедупа библиотек: gradle-суффикс `@ext` (NeoForge) игнорируется.
fn lib_key(name: &str) -> String {
    name.split('@').next().unwrap_or(name).to_string()
}

/// Слияние цепочки: ребёнок приоритетнее, аргументы конкатенируются (родитель → ребёнок).
fn merge_chain(chain: Vec<&VersionJson>) -> Result<ResolvedVersion> {
    let mut id = String::new();
    let mut version_type: Option<String> = None;
    let mut main_class = String::new();
    let mut assets: Option<String> = None;
    let mut asset_index: Option<AssetIndexRef> = None;
    let mut client_download: Option<DownloadInfo> = None;
    let mut java_version: Option<JavaVersionRef> = None;
    let mut minecraft_arguments: Option<String> = None;
    let mut logging: Option<Logging> = None;
    let mut inherits_from: Option<String> = None;
    let mut jvm_args: Vec<ArgElem> = Vec::new();
    let mut game_args: Vec<ArgElem> = Vec::new();
    // Библиотеки: ребёнок приоритетнее (первым в списке), дедуп по имени.
    let mut libraries: Vec<Library> = Vec::new();

    for v in chain.iter().rev() {
        // От корня к ребёнку: ребёнок перезаписывает скалярные поля.
        if !v.id.is_empty() {
            id = v.id.clone();
        }
        if v.version_type.is_some() {
            version_type = v.version_type.clone();
        }
        if let Some(mc) = &v.main_class {
            main_class = mc.clone();
        }
        if let Some(a) = &v.assets {
            assets = Some(a.clone());
        }
        if v.asset_index.is_some() {
            asset_index = v.asset_index.clone();
        }
        if v.downloads.client.is_some() {
            client_download = v.downloads.client.clone();
        }
        if v.java_version.is_some() {
            java_version = v.java_version.clone();
        }
        if v.minecraft_arguments.is_some() {
            minecraft_arguments = v.minecraft_arguments.clone();
        }
        if v.logging.is_some() {
            logging = v.logging.clone();
        }
        inherits_from = v.inherits_from.clone();
        // Аргументы: родитель сначала, ребёнок потом (спека §6.4).
        if let Some(args) = &v.arguments {
            jvm_args.extend(args.jvm.iter().cloned());
            game_args.extend(args.game.iter().cloned());
        }
        // Дочерние библиотеки приоритетнее: вставляем в начало, дедуп по имени.
        // NeoForge пишет gradle-координаты с суффиксом `@jar` — нормализуем,
        // иначе один jar попадает в classpath дважды (реальный кейс M3).
        let mut merged: Vec<Library> = Vec::with_capacity(v.libraries.len() + libraries.len());
        for lib in v.libraries.clone() {
            let key = lib_key(&lib.name);
            if !merged.iter().any(|l| lib_key(&l.name) == key) {
                merged.push(lib);
            }
        }
        for lib in libraries.drain(..) {
            let key = lib_key(&lib.name);
            if !merged.iter().any(|l| lib_key(&l.name) == key) {
                merged.push(lib);
            }
        }
        libraries = merged;
    }

    Ok(ResolvedVersion {
        id,
        version_type,
        main_class,
        assets: assets.unwrap_or_else(|| "legacy".into()),
        asset_index,
        client_download,
        java_version,
        arguments_jvm: jvm_args,
        arguments_game: game_args,
        minecraft_arguments,
        libraries,
        logging,
        inherits_from,
    })
}

/// Развернуть элемент аргументов: правила → строки (value строка или массив).
pub fn flatten_args(
    elems: &[ArgElem],
    os: &OsContext,
    features: &BTreeMap<String, bool>,
) -> Vec<String> {
    let mut out = Vec::new();
    for e in elems {
        match e {
            ArgElem::Plain(s) => out.push(s.clone()),
            ArgElem::Conditional { rules, value } => {
                if evaluate(rules, os, features) {
                    match value {
                        ArgValue::One(s) => out.push(s.clone()),
                        ArgValue::Many(list) => out.extend(list.iter().cloned()),
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os() -> OsContext {
        OsContext {
            name: crate::mojang::rules::OsName::Windows,
            arch_bits: 64,
            is_arm: false,
        }
    }

    #[test]
    fn parse_modern_version_json() {
        let v: VersionJson = serde_json::from_str(include_str!(
            "../../tests/fixtures/version_1_20_1.json"
        ))
        .unwrap();
        assert_eq!(v.id, "1.20.1");
        assert_eq!(v.main_class.as_deref(), Some("net.minecraft.client.main.Main"));
        let ai = v.asset_index.unwrap();
        assert_eq!(ai.id, "5");
        let dl = v.downloads.client.unwrap();
        assert!(dl.sha1.is_some());
        assert_eq!(v.java_version.unwrap().major_version, 17);
        assert!(v.arguments.is_some());
        assert!(v.minecraft_arguments.is_none());
    }

    #[test]
    fn parse_legacy_version_json() {
        let v: VersionJson =
            serde_json::from_str(include_str!("../../tests/fixtures/version_1_12_2.json")).unwrap();
        assert_eq!(v.id, "1.12.2");
        assert!(v.minecraft_arguments.is_some());
        assert!(v.arguments.is_none());
        // 1.12.2 имеет нативы-классификаторы
        let natives_lib = v
            .libraries
            .iter()
            .find(|l| l.name.contains("lwjgl-platform"))
            .expect("lwjgl-platform должен быть в фикстуре 1.12.2");
        assert!(natives_lib.natives.is_some());
    }

    #[test]
    fn inherits_from_merge_child_priority() {
        let dir = tempfile::tempdir().unwrap();
        let parent: VersionJson = serde_json::from_str(include_str!(
            "../../tests/fixtures/version_1_20_1.json"
        ))
        .unwrap();
        let child: VersionJson = serde_json::from_str(include_str!(
            "../../tests/fixtures/version_fabric_like.json"
        ))
        .unwrap();
        std::fs::write(
            dir.path().join(format!("{}.json", parent.id)),
            serde_json::to_vec(&parent).unwrap(),
        )
        .unwrap();
        let resolved = VersionJson::resolve_chain(dir.path(), &child).unwrap();
        assert_eq!(resolved.id, "fabric-loader-0.19.5-1.20.1");
        assert_eq!(resolved.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
        // Аргументы: родитель потом ребёнок
        assert!(!resolved.arguments_jvm.is_empty());
        // Библиотеки обеих версий присутствуют
        let has_parent_lib = resolved
            .libraries
            .iter()
            .any(|l| l.name == "com.google.guava:guava:31.1-jre");
        let has_child_lib = resolved
            .libraries
            .iter()
            .any(|l| l.name.starts_with("net.fabricmc:fabric-loader:"));
        assert!(has_parent_lib, "библиотеки родителя должны остаться");
        assert!(has_child_lib, "библиотеки ребёнка должны быть добавлены");
        let dup_count = resolved
            .libraries
            .iter()
            .filter(|l| l.name == "com.google.guava:guava:31.1-jre")
            .count();
        assert_eq!(dup_count, 1, "дедуп библиотек по имени");
        assert_eq!(resolved.inherits_from.as_deref(), Some("1.20.1"));
    }

    #[test]
    fn inherits_cycle_detected() {
        let dir = tempfile::tempdir().unwrap();
        let a_json = r#"{"id":"a","inheritsFrom":"b","mainClass":"X"}"#;
        let b_json = r#"{"id":"b","inheritsFrom":"a","mainClass":"X"}"#;
        std::fs::write(dir.path().join("b.json"), b_json).unwrap();
        let a: VersionJson = serde_json::from_str(a_json).unwrap();
        assert!(VersionJson::resolve_chain(dir.path(), &a).is_err());
    }

    #[test]
    fn inherits_depth_limit() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..8 {
            let j = format!(
                r#"{{"id":"v{i}","inheritsFrom":"v{}","mainClass":"X"}}"#,
                i + 1
            );
            std::fs::write(dir.path().join(format!("v{i}.json")), j).unwrap();
        }
        let root: VersionJson =
            serde_json::from_str(r#"{"id":"v0","inheritsFrom":"v1","mainClass":"X"}"#).unwrap();
        assert!(VersionJson::resolve_chain(dir.path(), &root).is_err());
    }

    #[test]
    fn flatten_args_rules_and_values() {
        let elems: Vec<ArgElem> = serde_json::from_value(serde_json::json!([
            "-Xmx1G",
            {"rules": [{"action": "allow", "os": {"name": "windows"}}], "value": ["-Xss1M", "-XX:+UseG1GC"]},
            {"rules": [{"action": "allow", "features": {"is_demo_user": true}}], "value": "--demo"}
        ]))
        .unwrap();
        let out = flatten_args(&elems, &os(), &BTreeMap::new());
        assert_eq!(out, vec!["-Xmx1G", "-Xss1M", "-XX:+UseG1GC"]);
    }

    #[test]
    fn inherits_dedups_gradle_at_jar_names() {
        // Реальный кейс NeoForge: имена с `@jar` и без — одна библиотека.
        let dir = tempfile::tempdir().unwrap();
        let parent: VersionJson = serde_json::from_str(
            r#"{"id":"1.21.1","mainClass":"M","libraries":[
                {"name":"org.apache.logging.log4j:log4j-core:2.22.1",
                 "downloads":{"artifact":{"url":"u","path":"org/apache/logging/log4j/log4j-core/2.22.1/log4j-core-2.22.1.jar"}}}
            ]}"#,
        )
        .unwrap();
        let child: VersionJson = serde_json::from_str(
            r#"{"id":"neoforge-21.1.99","inheritsFrom":"1.21.1","mainClass":"B","libraries":[
                {"name":"org.apache.logging.log4j:log4j-core:2.22.1@jar",
                 "downloads":{"artifact":{"url":"u2","path":"org/apache/logging/log4j/log4j-core/2.22.1/log4j-core-2.22.1.jar"}}}
            ]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("1.21.1.json"),
            serde_json::to_vec(&parent).unwrap(),
        )
        .unwrap();
        let resolved = VersionJson::resolve_chain(dir.path(), &child).unwrap();
        let dup_count = resolved
            .libraries
            .iter()
            .filter(|l| l.name.starts_with("org.apache.logging.log4j:log4j-core"))
            .count();
        assert_eq!(dup_count, 1, "@jar и обычное имя — одна библиотека");
    }
}
