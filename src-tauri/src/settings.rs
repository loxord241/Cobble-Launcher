//! Настройки: JSON, атомарная запись, миграции по `schemaVersion` (спека §7).

use crate::errors::{LauncherError, Result};
use crate::util::fs::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;

/// `ui_accent` — только `#RRGGBB`: значение уходит в CSS-переменную, мусор
/// (в т.ч. из правленного вручную settings.json) дальше не пропускаем.
fn is_hex_color(value: &str) -> bool {
    regex::Regex::new(r"^#[0-9a-fA-F]{6}$")
        .map(|re| re.is_match(value))
        .unwrap_or(false)
}

/// Допустимые логотипы (D38). Общий для `validate` и санитизации в `migrate`:
/// рассинхрон списков означал бы, что `load` принимает то, что `save` отвергнет.
const VALID_LOGOS: [&str; 7] = ["", "grass", "copper", "chest", "honey", "flat", "custom"];

/// Битый settings.json отправить в карантин: переименовать в `settings.json.bad`
/// (занятый слот не затираем — тогда `.bad.1`, `.bad.2`, ...). Иначе молча
/// подставленные дефолты при первом же save ПЕРЕЗАПИСАЛИ бы файл вместе с
/// curseforge-ключом и активным аккаунтом — карантин оставляет данные для
/// ручного восстановления.
fn quarantine_broken(path: &Path, reason: &str) {
    let from = crate::util::fs::long_path(path);
    for i in 0..10u32 {
        let mut name = path.as_os_str().to_os_string();
        match i {
            0 => name.push(".bad"),
            _ => name.push(format!(".bad.{i}")),
        }
        let candidate = PathBuf::from(name);
        if candidate.exists() {
            continue;
        }
        match std::fs::rename(&from, crate::util::fs::long_path(&candidate)) {
            Ok(()) => {
                tracing::warn!(
                    "settings.json невалиден ({reason}) — перемещён в {}",
                    candidate.display()
                );
                return;
            }
            Err(e) => {
                tracing::warn!("settings.json невалиден ({reason}), карантин не удался: {e}");
                return;
            }
        }
    }
    tracing::warn!(
        "settings.json невалиден ({reason}); слоты карантина заняты — файл оставлен как есть"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// Настройки лаунчера. Секреты (curseforge_api_key, azure_client_id) живут здесь,
/// файл — вне git (спека §3).
/// Формат файла — camelCase по спеке §7; старые snake_case-файлы читаются
/// через aliases (совместимость с файлами до M5).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub schema_version: u32,
    pub theme: Theme,
    pub language: String,
    pub download_parallelism: u32,
    pub default_ram_mb: u32,
    pub default_jvm_flags: Vec<String>,
    pub default_resolution: Option<Resolution>,
    pub show_snapshots: bool,
    pub show_old_versions: bool,
    pub proxy_url: Option<String>,
    pub mirror_base: Option<String>,
    pub curseforge_api_key: Option<String>,
    pub azure_client_id: Option<String>,
    pub accounts_active_id: Option<String>,
    pub onboarding_done: bool,
    /// Пути к Java, добавленные пользователем вручную (спека §6.5).
    pub extra_java_paths: Vec<String>,
    /// Масштаб интерфейса в процентах (80–150), как zoom в больших лаунчерах.
    pub ui_scale: u32,
    /// Шрифт интерфейса: "" — из темы; иначе system|serif|mono|round (кастомизация).
    pub ui_font: String,
    /// Акцентный цвет поверх палитры темы: "" — из темы; иначе `#RRGGBB`.
    pub ui_accent: String,
    /// Режим «Работать офлайн» (F16): сетевые запросы ядра честно падают
    /// сразу (offline_mode), без ожидания таймаутов; запуск — только из
    /// уже скачанных файлов.
    pub work_offline: bool,
    /// Ограничение скорости загрузки, КБ/с (F17); 0 — без ограничения.
    pub speed_limit_kbps: u32,
    /// Discord Rich Presence (F26): статус «играет в <инстанс>».
    pub discord_rpc: bool,
    /// Логотип (D38): "" | grass | copper | chest | honey | flat | custom.
    /// "" и неизвестное = встроенный травяной (дефолт).
    pub logo: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            theme: Theme::Dark,
            language: "en".into(),
            download_parallelism: 16,
            default_ram_mb: 2048,
            default_jvm_flags: Vec::new(),
            default_resolution: None,
            show_snapshots: false,
            show_old_versions: false,
            proxy_url: None,
            mirror_base: None,
            curseforge_api_key: None,
            azure_client_id: None,
            accounts_active_id: None,
            onboarding_done: false,
            extra_java_paths: Vec::new(),
            ui_scale: 100,
            ui_font: String::new(),
            ui_accent: String::new(),
            work_offline: false,
            speed_limit_kbps: 0,
            discord_rpc: false,
            logo: String::new(),
        }
    }
}

impl Settings {
    /// Загрузить; при отсутствии файла — дефолт. Битый JSON не молча
    /// подменяется дефолтом (которые затем затёрли бы файл при первом save):
    /// сначала карантин в `settings.json.bad`, потом дефолт. Миграции по
    /// schemaVersion.
    pub fn load(path: &Path) -> Result<Self> {
        let long = crate::util::fs::long_path(path);
        if !long.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read(&long)?;
        let mut settings: Settings = match serde_json::from_slice(&data) {
            Ok(s) => s,
            Err(e) => {
                quarantine_broken(&long, &e.to_string());
                return Ok(Self::default());
            }
        };
        settings.migrate();
        Ok(settings)
    }

    /// Атомарно сохранить. Значения, попадающие в CSS, проверяются до записи:
    /// невалидный `ui_accent` не должен ни уехать в файл, ни дойти до UI.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let data = serde_json::to_vec_pretty(self)?;
        atomic_write(path, &data)
    }

    /// Проверка настроек кастомизации. `ui_accent`: "" — «из темы», иначе `#RRGGBB`.
    pub fn validate(&self) -> Result<()> {
        if !self.ui_accent.is_empty() && !is_hex_color(&self.ui_accent) {
            return Err(LauncherError::InvalidInput(format!(
                "ui_accent: ожидается цвет вида #RRGGBB, получено {}",
                self.ui_accent
            )));
        }
        if !VALID_LOGOS.contains(&self.logo.as_str()) {
            return Err(LauncherError::InvalidInput(format!(
                "logo: неизвестное значение {:?}",
                self.logo
            )));
        }
        Ok(())
    }

    /// Миграции схемы: 0/N → текущая. Пока схема одна — приведение версии.
    fn migrate(&mut self) {
        // Санитизация: невалидный акцент в правленом вручную файле отбрасываем
        // в значение по умолчанию, иначе он блокировал бы любое сохранение (validate).
        if !self.ui_accent.is_empty() && !is_hex_color(&self.ui_accent) {
            tracing::warn!("ui_accent в settings.json невалиден — сброшен в значение темы");
            self.ui_accent.clear();
        }
        // Тот же принцип для logo: невалидное значение бьёт validate() и
        // блокировало бы ВСЕ сохранения (смена аккаунта и пр.) — сбрасываем
        // во встроенный дефолт, как ui_accent.
        if !VALID_LOGOS.contains(&self.logo.as_str()) {
            tracing::warn!(
                "logo в settings.json невалиден ({:?}) — сброшен в значение по умолчанию",
                self.logo
            );
            self.logo.clear();
        }
        if self.schema_version < SCHEMA_VERSION {
            // Будущие миграции: match self.schema_version { 0 => {...}, ... }
            self.schema_version = SCHEMA_VERSION;
        }
    }
}

/// `scheme://user:pass@host:port` → `scheme://host:port`. Креды прокси —
/// такие же секреты, как ключи: в экспорт не вывозим. URL без userinfo
/// (и вовсе без схемы) возвращается как есть.
fn strip_proxy_credentials(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let rest = &url[scheme_end + "://".len()..];
    // authority = до первого '/', '?' или '#'; userinfo — до последнего '@'.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let Some(at) = authority.rfind('@') else {
        return url.to_string();
    };
    let mut clean = String::with_capacity(url.len() - at - 1);
    clean.push_str(&url[..scheme_end + "://".len()]);
    clean.push_str(&authority[at + 1..]);
    clean.push_str(&rest[authority_end..]);
    clean
}

/// Экспорт настроек в JSON без секретов (F20): `curseforge_api_key`,
/// `azure_client_id` и креды из `proxy_url` обнуляются/вычищаются — файл
/// может уйти другому человеку/машине, секреты наружу не вывозим.
pub fn export_json(s: &Settings) -> String {
    let mut clean = s.clone();
    clean.curseforge_api_key = None;
    clean.azure_client_id = None;
    if let Some(url) = clean.proxy_url.take() {
        clean.proxy_url = Some(strip_proxy_credentials(&url));
    }
    // Pretty-формат: файл читают и правят руками. Сериализация структуры без
    // карт/чисел с плавающей точкой не падает, но паниковать запрещено.
    serde_json::to_string_pretty(&clean).unwrap_or_else(|_| "{}".into())
}

/// Импорт настроек из JSON (F20): парс + санитизация/миграции как в `load`,
/// затем `validate`. Секреты из файла НЕ переносим (экспорт их не пишет,
/// а чужой файл с вписанным ключом принимать не хотим), `schema_version`
/// принудительно текущий — файл более старой/новой схемы не ломает ядро.
pub fn import_json(text: &str) -> Result<Settings> {
    let mut s: Settings = serde_json::from_str(text)?;
    s.migrate();
    s.schema_version = SCHEMA_VERSION;
    s.curseforge_api_key = None;
    s.azure_client_id = None;
    s.validate()?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let s = Settings::load(&p).unwrap();
        assert_eq!(s.download_parallelism, 16);
        assert!(!s.onboarding_done);
        s.save(&p).unwrap();
        let s2 = Settings::load(&p).unwrap();
        assert_eq!(s2.schema_version, SCHEMA_VERSION);
        assert_eq!(s2.theme, Theme::Dark);
    }

    #[test]
    fn old_schema_without_new_fields_fills_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(
            &p,
            br#"{"schemaVersion": 0, "theme": "light"}"#,
        )
        .unwrap();
        let s = Settings::load(&p).unwrap();
        assert_eq!(s.theme, Theme::Light);
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        assert_eq!(s.download_parallelism, 16);
    }

    #[test]
    fn customization_fields_default_to_theme() {
        let s = Settings::default();
        assert_eq!(s.ui_font, "");
        assert_eq!(s.ui_accent, "");
        assert!(s.validate().is_ok());
        // Файл без новых полей читается: кастомизация = «из темы».
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, br#"{"theme": "dark"}"#).unwrap();
        let s2 = Settings::load(&p).unwrap();
        assert_eq!(s2.ui_font, "");
        assert_eq!(s2.ui_accent, "");
    }

    #[test]
    fn accent_validation_accepts_hex_and_empty() {
        let mut s = Settings::default();
        for ok in ["", "#7FB2F0", "#7fb2f0", "#000000", "#ffffff"] {
            s.ui_accent = ok.into();
            assert!(s.validate().is_ok(), "{ok:?} должен проходить");
        }
    }

    #[test]
    fn accent_validation_rejects_non_hex() {
        let mut s = Settings::default();
        for bad in [
            "7FB2F0",
            "#7FB2F",
            "#7FB2F0F",
            "#7FB2FG",
            "red",
            "#7fb2f0; background: red",
            " #7FB2F0",
            "#7FB2F0 ",
            "var(--accent)",
        ] {
            s.ui_accent = bad.into();
            assert!(
                matches!(s.validate(), Err(LauncherError::InvalidInput(_))),
                "{bad:?} должен быть отклонён"
            );
        }
    }

    #[test]
    fn save_rejects_invalid_accent_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let s = Settings {
            ui_accent: "url(javascript:alert(1))".into(),
            ..Default::default()
        };
        assert!(matches!(s.save(&p), Err(LauncherError::InvalidInput(_))));
        assert!(!p.exists(), "файл не должен появиться");
    }

    #[test]
    fn load_sanitizes_invalid_accent_from_hand_edited_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, br##"{"uiAccent": "#zzz", "uiFont": "mono"}"##).unwrap();
        let s = Settings::load(&p).unwrap();
        assert_eq!(s.ui_accent, "", "мусор из файла сброшен в «из темы»");
        assert_eq!(s.ui_font, "mono", "валидный шрифт из файла сохранён");
        // Санитизация разблокирует сохранение (иначе не переключить аккаунт).
        s.save(&p).unwrap();
    }

    #[test]
    fn work_offline_defaults_false_and_roundtrips() {
        // Старый файл без поля → false; после сохранения поле читается обратно.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, br#"{"theme": "dark"}"#).unwrap();
        let mut s = Settings::load(&p).unwrap();
        assert!(!s.work_offline);
        s.work_offline = true;
        s.save(&p).unwrap();
        let s2 = Settings::load(&p).unwrap();
        assert!(s2.work_offline);
    }

    #[test]
    fn export_json_omits_secret_values() {
        let s = Settings {
            curseforge_api_key: Some("cf-secret-value".into()),
            azure_client_id: Some("azure-client-value".into()),
            language: "ru".into(),
            ui_scale: 125,
            ..Default::default()
        };
        let json = export_json(&s);
        assert!(!json.contains("cf-secret-value"), "ключ CurseForge не вывозится");
        assert!(!json.contains("azure-client-value"), "azure client id не вывозится");
        // Несекретные поля в файле есть (roundtrip-основа).
        assert!(json.contains("\"language\": \"ru\""));
        assert!(json.contains("\"uiScale\": 125"));
    }

    #[test]
    fn import_json_roundtrips_main_fields() {
        let s = Settings {
            theme: Theme::Light,
            language: "uk".into(),
            download_parallelism: 8,
            default_ram_mb: 4096,
            ui_scale: 110,
            ui_font: "mono".into(),
            ui_accent: "#7FB2F0".into(),
            work_offline: true,
            curseforge_api_key: Some("cf-secret-value".into()),
            azure_client_id: Some("azure-client-value".into()),
            ..Default::default()
        };
        let imported = import_json(&export_json(&s)).expect("свой экспорт импортируется");
        assert_eq!(imported.theme, Theme::Light);
        assert_eq!(imported.language, "uk");
        assert_eq!(imported.download_parallelism, 8);
        assert_eq!(imported.default_ram_mb, 4096);
        assert_eq!(imported.ui_scale, 110);
        assert_eq!(imported.ui_font, "mono");
        assert_eq!(imported.ui_accent, "#7FB2F0");
        assert!(imported.work_offline);
        // Секреты через import не переносятся даже из СВОЕГО экспорта.
        assert_eq!(imported.curseforge_api_key, None);
        assert_eq!(imported.azure_client_id, None);
    }

    #[test]
    fn import_json_sanitizes_bad_accent_and_forces_schema() {
        let text = r##"{"schemaVersion": 0, "theme": "dark", "uiAccent": "#zzz", "uiFont": "mono"}"##;
        let s = import_json(text).expect("битый акцент санитизируется, а не отклоняется");
        assert_eq!(s.ui_accent, "", "мусор в uiAccent сброшен в «из темы», как в load");
        assert_eq!(s.ui_font, "mono", "валидное поле из файла сохранено");
        assert_eq!(s.schema_version, SCHEMA_VERSION, "версия схемы принудительно текущая");
        assert!(s.validate().is_ok(), "после санитизации настройки проходят validate");
    }

    #[test]
    fn import_json_does_not_carry_secrets() {
        let text = r##"{"theme": "light", "curseForgeApiKey": "leak", "azureClientId": "leak2"}"##;
        let s = import_json(text).expect("файл с вписанными секретами читается");
        assert_eq!(s.curseforge_api_key, None, "чужие секреты не переносим");
        assert_eq!(s.azure_client_id, None);
    }

    #[test]
    fn import_json_rejects_broken_json() {
        assert!(import_json("не json").is_err());
    }

    #[test]
    fn load_quarantines_broken_file_and_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, b"{ broken json").unwrap();
        let s = Settings::load(&p).unwrap();
        assert_eq!(s.download_parallelism, 16, "битый файл → дефолт");
        let bad = dir.path().join("settings.json.bad");
        assert!(bad.exists(), "битый файл сохранён в карантин");
        assert!(!p.exists(), "оригинал переименован, не оставлен на месте");
        // Дефолт можно сохранить: Settings::load больше не молча затирает
        // битый файл (карантин уже забрал данные).
        s.save(&p).unwrap();
    }

    #[test]
    fn quarantine_does_not_overwrite_previous_bad() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, b"{ first broken").unwrap();
        Settings::load(&p).unwrap();
        assert!(dir.path().join("settings.json.bad").exists());
        std::fs::write(&p, b"{ second broken").unwrap();
        Settings::load(&p).unwrap();
        let first = std::fs::read_to_string(dir.path().join("settings.json.bad")).unwrap();
        assert_eq!(first, "{ first broken", "прежний карантин не затёрт");
        assert!(dir.path().join("settings.json.bad.1").exists());
    }

    #[test]
    fn load_sanitizes_invalid_logo_from_hand_edited_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, br##"{"logo": "javascript:alert(1)", "uiFont": "mono"}"##).unwrap();
        let s = Settings::load(&p).unwrap();
        assert_eq!(s.logo, "", "мусор в logo сброшен в дефолт, как ui_accent");
        assert_eq!(s.ui_font, "mono", "валидное поле из файла сохранено");
        // Санитизация разблокирует сохранение: logo больше не бьёт validate.
        s.save(&p).unwrap();
    }

    #[test]
    fn export_json_strips_proxy_credentials() {
        let mut s = Settings {
            proxy_url: Some("http://user:pa%40ss@proxy.local:8080".into()),
            ..Default::default()
        };
        let json = export_json(&s);
        assert!(
            json.contains("\"proxyUrl\": \"http://proxy.local:8080\""),
            "креды прокси вычищены, хост оставлен: {json}"
        );
        assert!(!json.contains("pa%40ss"), "пароль прокси не вывозится");

        // Прокси без userinfo не трогаем.
        s.proxy_url = Some("socks5://proxy.local:1080".into());
        let json = export_json(&s);
        assert!(json.contains("\"proxyUrl\": \"socks5://proxy.local:1080\""));
    }
}
