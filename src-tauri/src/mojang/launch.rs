//! Сборка команды запуска: JVM/game аргументы, подстановки ${...}, правила,
//! старый формат minecraftArguments, log4j-конфиг (спека §6.1.6–6.1.8).

use crate::errors::{LauncherError, Result};
use crate::mojang::rules::OsContext;
use crate::mojang::version::{flatten_args, ResolvedVersion};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LaunchValues {
    pub auth_player_name: String,
    pub auth_uuid: String,
    pub auth_access_token: String,
    pub user_type: String,
    pub version_name: String,
    pub version_type: String,
    pub game_directory: PathBuf,
    pub assets_root: PathBuf,
    pub assets_index_name: String,
    /// Каталог virtual-ассетов для legacy-версий (спека §6.4).
    pub game_assets: Option<PathBuf>,
    pub natives_directory: PathBuf,
    pub launcher_name: String,
    pub launcher_version: String,
    pub classpath: String,
    pub library_directory: PathBuf,
    pub resolution: Option<(u32, u32)>,
    pub log_config_path: Option<PathBuf>,
    /// Пользовательские JVM-флаги (RAM, GC) — после JSON-аргументов.
    pub jvm_extra: Vec<String>,
    /// Дополнительные игровые аргументы лаунчера (например, --mavenRoot/--mods
    /// для NeoForge) — в конец game-аргументов.
    pub game_args_extra: Vec<String>,
}

/// Готовая команда запуска.
#[derive(Debug, Clone)]
pub struct LaunchCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl LaunchValues {
    fn substitution_map(&self) -> BTreeMap<String, String> {
        let mut m: BTreeMap<&str, String> = BTreeMap::new();
        m.insert("auth_player_name", self.auth_player_name.clone());
        m.insert("auth_uuid", self.auth_uuid.clone());
        m.insert("auth_access_token", self.auth_access_token.clone());
        m.insert("auth_session", self.auth_access_token.clone());
        m.insert("auth_xuid", "0".into());
        m.insert("clientid", self.launcher_name.clone());
        m.insert("user_type", self.user_type.clone());
        m.insert("user_properties", "{}".into());
        m.insert("version_name", self.version_name.clone());
        m.insert("version_type", self.version_type.clone());
        m.insert("game_directory", self.game_directory.to_string_lossy().into_owned());
        m.insert("assets_root", self.assets_root.to_string_lossy().into_owned());
        m.insert("assets_index_name", self.assets_index_name.clone());
        m.insert(
            "game_assets",
            self.game_assets
                .as_ref()
                .unwrap_or(&self.assets_root)
                .to_string_lossy()
                .into_owned(),
        );
        m.insert("natives_directory", self.natives_directory.to_string_lossy().into_owned());
        m.insert("launcher_name", self.launcher_name.clone());
        m.insert("launcher_version", self.launcher_version.clone());
        m.insert("classpath", self.classpath.clone());
        m.insert(
            "classpath_separator",
            if cfg!(windows) { ";" } else { ":" }.into(),
        );
        m.insert("library_directory", self.library_directory.to_string_lossy().into_owned());
        if let Some((w, h)) = self.resolution {
            m.insert("resolution_width", w.to_string());
            m.insert("resolution_height", h.to_string());
        }
        if let Some(p) = &self.log_config_path {
            m.insert("log_config_path", p.to_string_lossy().into_owned());
            // Шаблон logging.client.argument в JSON использует именно ${path}
            // (спека §6.1.8: «-Dlog4j.configurationFile=... (подстановка path)»).
            m.insert("path", p.to_string_lossy().into_owned());
        }
        m.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }
}

pub fn substitute(arg: &str, map: &BTreeMap<String, String>) -> String {
    let mut out = arg.to_string();
    while let Some(start) = out.find("${") {
        let Some(end_rel) = out[start..].find('}') else {
            break;
        };
        let key = &out[start + 2..start + end_rel];
        let replacement = map.get(key).cloned().unwrap_or_default();
        if map.get(key).is_none() {
            tracing::warn!("неизвестная подстановка ${{{key}}} в аргументе: {arg}");
        }
        out.replace_range(start..start + end_rel + 1, &replacement);
    }
    out
}

/// Собрать полную команду: [java] + jvm + mainClass + game.
pub fn build_command(
    rv: &ResolvedVersion,
    values: &LaunchValues,
    os: &OsContext,
    java_path: &Path,
) -> Result<LaunchCommand> {
    let mut features: BTreeMap<String, bool> = BTreeMap::new();
    features.insert("is_demo_user".to_string(), false);
    features.insert("has_custom_resolution".to_string(), values.resolution.is_some());
    for k in ["is_quick_play_singleplayer", "is_quick_play_multiplayer", "is_quick_play_realms"] {
        features.insert(k.to_string(), false);
    }

    let map = values.substitution_map();
    let new_format = rv.arguments_jvm.is_empty() && rv.arguments_game.is_empty()
        && rv.minecraft_arguments.is_none();

    let (mut jvm_args, game_args);
    if !rv.arguments_jvm.is_empty() || !rv.arguments_game.is_empty() {
        // Новый формат (1.13+)
        jvm_args = flatten_args(&rv.arguments_jvm, os, &features);
        game_args = flatten_args(&rv.arguments_game, os, &features);
        // Спека §6.1.6: java.library.path обязан указывать на каталог нативов.
        if !jvm_args.iter().any(|a| a.contains("java.library.path")) {
            jvm_args.push("-Djava.library.path=${natives_directory}".into());
        }
    } else if new_format && rv.main_class.is_empty() {
        return Err(LauncherError::InvalidInput(format!(
            "версия {} без аргументов и mainClass",
            rv.id
        )));
    } else {
        // Старый формат (до 1.6): minecraftArguments (спека §6.1.7). Класспас и
        // java.library.path лаунчер добавляет сам — в JSON их нет.
        let mc_args = rv.minecraft_arguments.clone().ok_or_else(|| {
            LauncherError::InvalidInput(format!("у версии {} нет ни arguments, ни minecraftArguments", rv.id))
        })?;
        jvm_args = vec![
            "-Djava.library.path=${natives_directory}".into(),
            "-cp".into(),
            "${classpath}".into(),
        ];
        game_args = mc_args.split_whitespace().map(str::to_string).collect();
    }

    // A49: консоль Windows по умолчанию в cp866/cp1251 — кириллица в путях
    // (%LOCALAPPDATA%), в логах и в чате модов превращается в кракозябры,
    // а часть модов на этом падает. Флаг ставится ПЕРВЫМ JVM-аргументом и
    // только на Windows: на Linux/macOS UTF-8 и так по умолчанию (JEP 400).
    if cfg!(windows) {
        jvm_args.insert(0, "-Dfile.encoding=UTF-8".into());
    }

    // log4j-конфиг: только если есть в JSON (спека §6.1.8).
    if let (Some(path), Some(log)) = (&values.log_config_path, &rv.logging) {
        if let Some(client) = &log.client {
            let arg = substitute(&client.argument, &map);
            if !arg.is_empty() {
                jvm_args.push(arg);
                let _ = path; // путь уже внутри substitution map (log_config_path)
            }
        }
    }

    // Пользовательские флаги — после JSON-аргументов.
    jvm_args.extend(values.jvm_extra.iter().cloned());

    let mut args: Vec<String> = jvm_args
        .iter()
        .map(|a| substitute(a, &map))
        .collect();
    args.push(rv.main_class.clone());
    args.extend(game_args.iter().map(|a| substitute(a, &map)));
    // Аргументы лаунчера (--mavenRoot/--mods у NeoForge) — в самый конец.
    args.extend(values.game_args_extra.iter().cloned());

    Ok(LaunchCommand {
        program: java_path.to_path_buf(),
        args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mojang::version::VersionJson;

    fn os() -> OsContext {
        OsContext {
            name: crate::mojang::rules::OsName::Windows,
            arch_bits: 64,
            is_arm: false,
        }
    }

    fn base_values(game_dir: &Path) -> LaunchValues {
        LaunchValues {
            auth_player_name: "Steve".into(),
            auth_uuid: "00000000-0000-3000-8000-000000000000".into(),
            auth_access_token: "token0".into(),
            user_type: "legacy".into(),
            version_name: "1.20.1".into(),
            version_type: "release".into(),
            game_directory: game_dir.to_path_buf(),
            assets_root: PathBuf::from("/root/assets"),
            assets_index_name: "5".into(),
            game_assets: None,
            natives_directory: PathBuf::from("/root/bin/1.20.1"),
            launcher_name: "mc-launcher-v2".into(),
            launcher_version: "0.2.0".into(),
            classpath: "/a.jar;/b.jar".into(),
            library_directory: PathBuf::from("/root/store/libraries"),
            resolution: Some((1280, 720)),
            log_config_path: None,
            jvm_extra: vec!["-Xmx2048M".into()],
            game_args_extra: Vec::new(),
        }
    }

    fn resolved_of(json: &str) -> ResolvedVersion {
        let v: VersionJson = serde_json::from_str(json).unwrap();
        VersionJson::resolve_chain(Path::new("/nonexistent"), &v).unwrap()
    }

    #[test]
    fn build_modern_command_1_20_1() {
        let rv = resolved_of(include_str!("../../tests/fixtures/version_1_20_1.json"));
        let dir = tempfile::tempdir().unwrap();
        let values = base_values(dir.path());
        let cmd = build_command(&rv, &values, &os(), Path::new("java")).unwrap();
        assert_eq!(cmd.program, PathBuf::from("java"));
        let joined = cmd.args.join(" ");
        assert!(joined.contains("net.minecraft.client.main.Main"));
        assert!(joined.contains("--username Steve"));
        assert!(joined.contains("--width 1280"));
        assert!(joined.contains("--height 720"));
        assert!(joined.contains("-Xmx2048M"));
        assert!(joined.contains("java.library.path"));
        assert!(joined.contains("--assetIndex 5"));
        assert!(!joined.contains("${"), "все подстановки обязаны разрешиться");
    }

    #[test]
    fn build_legacy_command_1_12_2() {
        let rv = resolved_of(include_str!("../../tests/fixtures/version_1_12_2.json"));
        let dir = tempfile::tempdir().unwrap();
        let mut values = base_values(dir.path());
        values.resolution = None;
        let cmd = build_command(&rv, &values, &os(), Path::new("java")).unwrap();
        let joined = cmd.args.join(" ");
        assert!(joined.contains("net.minecraft.client.main.Main"));
        assert!(joined.contains("--username Steve"));
        assert!(joined.contains("--accessToken token0"));
        assert!(joined.contains("-Djava.library.path="));
        // Legacy: classpath добавляется лаунчером вручную
        assert!(joined.contains("-cp /a.jar;/b.jar"));
        assert!(joined.contains("-Xmx2048M"));
        // Без resolution --width не появляется
        assert!(!joined.contains("--width"));
    }

    #[test]
    fn substitution_replaces_all_occurrences() {
        let mut map = BTreeMap::new();
        map.insert("a".to_string(), "1".to_string());
        map.insert("b".to_string(), "2".to_string());
        assert_eq!(substitute("${a}-${b}-${a}", &map), "1-2-1");
        assert_eq!(substitute("plain", &map), "plain");
    }

    /// A49-регресс: на Windows кодировка файлов JVM обязана быть задана явно
    /// (кириллица в путях/логах), и это первый JVM-флаг.
    #[cfg(windows)]
    #[test]
    fn windows_pins_utf8_file_encoding() {
        let rv = resolved_of(include_str!("../../tests/fixtures/version_1_20_1.json"));
        let dir = tempfile::tempdir().unwrap();
        let values = base_values(dir.path());
        let cmd = build_command(&rv, &values, &os(), Path::new("java")).unwrap();
        assert_eq!(
            cmd.args.first().map(String::as_str),
            Some("-Dfile.encoding=UTF-8"),
            "первый JVM-аргумент: {:?}",
            &cmd.args[..3.min(cmd.args.len())]
        );
        // Legacy-ветка (до 1.13) собирает jvm_args вручную — флаг обязан быть и там.
        let legacy = resolved_of(include_str!("../../tests/fixtures/version_1_12_2.json"));
        let mut lvalues = base_values(dir.path());
        lvalues.resolution = None;
        let lcmd = build_command(&legacy, &lvalues, &os(), Path::new("java")).unwrap();
        assert_eq!(lcmd.args.first().map(String::as_str), Some("-Dfile.encoding=UTF-8"));
    }

    /// На не-Windows поведение не меняется (там UTF-8 по умолчанию, JEP 400).
    #[cfg(not(windows))]
    #[test]
    fn non_windows_has_no_encoding_flag() {
        let rv = resolved_of(include_str!("../../tests/fixtures/version_1_20_1.json"));
        let dir = tempfile::tempdir().unwrap();
        let values = base_values(dir.path());
        let cmd = build_command(&rv, &values, &os(), Path::new("java")).unwrap();
        assert!(!cmd
            .args
            .iter()
            .any(|a| a.starts_with("-Dfile.encoding")));
    }
}
