//! Библиотеки: артефакты, классификаторы нативов, maven-пути (спека §6.1.3).

use crate::errors::{LauncherError, Result};
use crate::mojang::rules::{evaluate, OsContext};
use crate::mojang::version::ResolvedVersion;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Скачиваемый артефакт: URL + относительный maven-путь + хэш.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedArtifact {
    pub url: String,
    /// Относительный путь (maven layout) внутри каталога библиотек.
    pub rel_path: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct PlannedLib {
    pub name: String,
    /// Основной jar для classpath. `None` — у библиотеки есть только нативы
    /// (реальный случай: jinput-platform в 1.12.2 — Mojang не выкладывает jar).
    pub artifact: Option<PlannedArtifact>,
    /// Классификатор нативов для текущей ОС (если есть у библиотеки).
    pub native: Option<PlannedArtifact>,
}

/// Путь maven-координаты: `g:a:v[:classifier]` → `g/a/v/a-v[-classifier].jar`.
/// Gradle-суффиксы `@jar` (NeoForge) отрезаются.
pub fn maven_path(coord: &str) -> Result<String> {
    let clean = |s: &str| s.split('@').next().unwrap_or(s).to_string();
    let parts: Vec<&str> = coord.split(':').collect();
    match parts.as_slice() {
        [g, a, v] => Ok(format!(
            "{g_dir}/{a}/{v}/{a}-{v}.jar",
            g_dir = g.replace('.', "/"),
            a = clean(a),
            v = clean(v)
        )),
        [g, a, v, c] => Ok(format!(
            "{g_dir}/{a}/{v}/{a}-{v}-{c}.jar",
            g_dir = g.replace('.', "/"),
            a = clean(a),
            v = clean(v),
            c = clean(c)
        )),
        _ => Err(LauncherError::InvalidInput(format!(
            "не maven-координата: {coord}"
        ))),
    }
}

/// Спланировать загрузку библиотек версии: фильтр по rules, артефакты,
/// нативы по классификаторам (`${arch}` заменяется на битность JVM).
/// Возвращает (библиотеки, отдельные классификаторы-нативы не нужны —
/// они внутри PlannedLib.native).
pub fn plan_libraries(
    rv: &ResolvedVersion,
    os: &OsContext,
    features: &BTreeMap<String, bool>,
) -> Result<Vec<PlannedLib>> {
    let mut out = Vec::new();
    for lib in &rv.libraries {
        if let Some(rules) = &lib.rules {
            if !evaluate(rules, os, features) {
                continue;
            }
        }
        // Основной артефакт. Некоторые старые библиотеки не имеют jar вовсе —
        // только классификаторы нативов (jinput-platform в 1.12.2).
        let has_native_for_os = lib
            .natives
            .as_ref()
            .is_some_and(|n| n.contains_key(os.name.as_str()));
        let artifact = match lib.downloads.as_ref().and_then(|d| d.artifact.clone()) {
            Some(a) => Some(PlannedArtifact {
                url: a.url,
                rel_path: a.path.unwrap_or_else(|| {
                    maven_path(&lib.name).unwrap_or_else(|_| lib.name.replace(':', "-"))
                }),
                sha1: a.sha1,
                size: a.size,
            }),
            None if has_native_for_os => None,
            None => {
                // Старый формат: только `name` + опциональный maven-base `url`.
                let rel = maven_path(&lib.name)?;
                let base = lib.url.clone().unwrap_or_else(|| "https://libraries.minecraft.net/".into());
                let base = if base.ends_with('/') { base } else { format!("{base}/") };
                Some(PlannedArtifact {
                    url: format!("{base}{rel}"),
                    rel_path: rel,
                    // У старых библиотек хэша в JSON нет: источник — официальный
                    // maven Mojang по HTTPS (спека §3).
                    sha1: None,
                    size: None,
                })
            }
        };
        // Нативы: карта natives по имени ОС; `${arch}` → битность.
        let native = lib
            .natives
            .as_ref()
            .and_then(|n| n.get(os.name.as_str()))
            .map(|classifier_tpl| {
                let classifier = classifier_tpl.replace("${arch}", &os.arch_bits.to_string());
                let cls = lib
                    .downloads
                    .as_ref()
                    .and_then(|d| d.classifiers.as_ref())
                    .and_then(|c| c.get(&classifier))
                    .ok_or_else(|| {
                        LauncherError::InvalidInput(format!(
                            "нет классификатора {classifier} у {}",
                            lib.name
                        ))
                    })?;
                Ok::<PlannedArtifact, LauncherError>(PlannedArtifact {
                    url: cls.url.clone(),
                    rel_path: cls
                        .path
                        .clone()
                        .unwrap_or_else(|| classifier_to_path(&classifier, &lib.name)),
                    sha1: cls.sha1.clone(),
                    size: cls.size,
                })
            })
            .transpose()?;
        out.push(PlannedLib {
            name: lib.name.clone(),
            artifact,
            native,
        });
    }
    Ok(out)
}

fn classifier_to_path(classifier: &str, coord: &str) -> String {
    // a:b:v + classifier c → a/b/v/b-v-c.jar
    let parts: Vec<&str> = coord.split(':').collect();
    if let [g, a, v] = parts.as_slice() {
        return format!("{}/{}/{}/{}-{}-{}.jar", g.replace('.', "/"), a, v, a, v, classifier);
    }
    format!("{classifier}.jar")
}

/// Распаковать нативы классификатора в `natives_dir` (спека §6.1.6):
/// только `.dll/.so/.jnilib`, без META-INF, zip-slip-защита.
pub fn extract_native(native_zip: &Path, natives_dir: &Path, exclude: &[String]) -> Result<usize> {
    let file = std::fs::File::open(crate::util::fs::long_path(native_zip))?;
    let exclude_lower: Vec<String> = exclude.iter().map(|e| e.to_ascii_lowercase()).collect();
    let keep = |name: &str| -> bool {
        let lower = name.to_ascii_lowercase();
        if lower.contains("meta-inf/") {
            return false;
        }
        if !(lower.ends_with(".dll") || lower.ends_with(".so") || lower.ends_with(".jnilib")) {
            return false;
        }
        !exclude_lower.iter().any(|ex| lower.starts_with(ex.as_str()))
    };
    let extracted = crate::util::zip::extract_zip_filtered(file, natives_dir, keep, |_| {})?;
    Ok(extracted.len())
}

/// Вычислить classpath: клиент + библиотеки + дополнительные jar (universal и
/// binpatch-клиент NeoForge/Forge, которые инсталлятор кладёт вне version JSON —
/// официальный лаунчер добавляет их сам, см. `-DignoreList` с префиксами).
pub fn build_classpath(
    libs: &[PlannedLib],
    lib_store: &Path,
    client_jar: &Path,
    extra_jars: &[std::path::PathBuf],
) -> String {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut items: Vec<String> = Vec::with_capacity(libs.len() + 1 + extra_jars.len());
    items.push(client_jar.to_string_lossy().into_owned());
    for l in libs {
        if let Some(a) = &l.artifact {
            items.push(lib_store.join(&a.rel_path).to_string_lossy().into_owned());
        }
    }
    for j in extra_jars {
        items.push(j.to_string_lossy().into_owned());
    }
    items.join(sep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_paths_classic_and_classified() {
        assert_eq!(
            maven_path("com.google.guava:guava:32.0.1-jre").unwrap(),
            "com/google/guava/guava/32.0.1-jre/guava-32.0.1-jre.jar"
        );
        assert_eq!(
            maven_path("org.lwjgl.lwjgl:lwjgl-platform:2.9.4:natives-windows").unwrap(),
            "org/lwjgl/lwjgl/lwjgl-platform/2.9.4/lwjgl-platform-2.9.4-natives-windows.jar"
        );
        assert!(maven_path("not-a-coordinate").is_err());
    }

    #[test]
    fn classpath_uses_semicolon_on_windows() {
        let libs = vec![PlannedLib {
            name: "a:b:v".into(),
            artifact: Some(PlannedArtifact {
                url: "http://x/a.jar".into(),
                rel_path: "a/b/v/b-v.jar".into(),
                sha1: None,
                size: None,
            }),
            native: None,
        }];
        let cp = build_classpath(
            &libs,
            Path::new("/store/lib"),
            Path::new("/store/client.jar"),
            &[],
        );
        let expected_sep = if cfg!(windows) { ";" } else { ":" };
        let expected = format!(
            "/store/client.jar{expected_sep}/store/lib{}a/b/v/b-v.jar",
            std::path::MAIN_SEPARATOR
        );
        assert!(cp.contains(&expected), "classpath: {cp}");
    }

    #[test]
    fn plan_1_12_2_natives_windows() {
        let v: crate::mojang::version::VersionJson = serde_json::from_str(include_str!(
            "../../tests/fixtures/version_1_12_2.json"
        ))
        .unwrap();
        let resolved = crate::mojang::version::ResolvedVersion {
            id: v.id.clone(),
            version_type: v.version_type.clone(),
            main_class: v.main_class.clone().unwrap_or_default(),
            assets: v.assets.clone().unwrap_or_default(),
            asset_index: v.asset_index.clone(),
            client_download: None,
            java_version: v.java_version.clone(),
            arguments_jvm: Vec::new(),
            arguments_game: Vec::new(),
            minecraft_arguments: v.minecraft_arguments.clone(),
            libraries: v.libraries.clone(),
            logging: v.logging.clone(),
            inherits_from: None,
        };
        let os = OsContext {
            name: crate::mojang::rules::OsName::Windows,
            arch_bits: 64,
            is_arm: false,
        };
        let planned = plan_libraries(&resolved, &os, &BTreeMap::new()).unwrap();
        let lwjgl = planned
            .iter()
            .find(|l| l.name.contains("lwjgl-platform"))
            .expect("lwjgl-platform");
        let native = lwjgl.native.as_ref().expect("натив windows обязателен");
        assert!(native.rel_path.contains("natives-windows"));
        // На 64-бит JVM правило arch:x86 должно исключить 32-битные библиотеки,
        // если бы они были (в 1.12.2 их нет — просто проверяем отсутствие паники).
        assert!(planned.iter().any(|l| l.name.contains("guava")));
    }

    #[test]
    fn plan_1_20_1_has_no_natives() {
        let v: crate::mojang::version::VersionJson = serde_json::from_str(include_str!(
            "../../tests/fixtures/version_1_20_1.json"
        ))
        .unwrap();
        let resolved = crate::mojang::version::ResolvedVersion {
            id: v.id.clone(),
            version_type: v.version_type.clone(),
            main_class: v.main_class.clone().unwrap_or_default(),
            assets: v.assets.clone().unwrap_or_default(),
            asset_index: v.asset_index.clone(),
            client_download: None,
            java_version: v.java_version.clone(),
            arguments_jvm: Vec::new(),
            arguments_game: Vec::new(),
            minecraft_arguments: v.minecraft_arguments.clone(),
            libraries: v.libraries.clone(),
            logging: v.logging.clone(),
            inherits_from: None,
        };
        let os = OsContext {
            name: crate::mojang::rules::OsName::Windows,
            arch_bits: 64,
            is_arm: false,
        };
        let planned = plan_libraries(&resolved, &os, &BTreeMap::new()).unwrap();
        // 1.19+: классификаторов нет (спека §6.1.6)
        assert!(planned.iter().all(|l| l.native.is_none()));
        // но lwjgl 3 присутствует
        assert!(planned.iter().any(|l| l.name.contains("lwjgl")));
    }
}
