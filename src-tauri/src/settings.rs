//! Настройки: JSON, атомарная запись, миграции по `schemaVersion` (спека §7).

use crate::errors::{LauncherError, Result};
use crate::util::fs::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;

/// `ui_accent` — только `#RRGGBB`: значение уходит в CSS-переменную, мусор
/// (в т.ч. из правленного вручную settings.json) дальше не пропускаем.
fn is_hex_color(value: &str) -> bool {
    regex::Regex::new(r"^#[0-9a-fA-F]{6}$")
        .map(|re| re.is_match(value))
        .unwrap_or(false)
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
        }
    }
}

impl Settings {
    /// Загрузить; при отсутствии файла — дефолт. Миграции по schemaVersion.
    pub fn load(path: &Path) -> Result<Self> {
        let long = crate::util::fs::long_path(path);
        if !long.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read(&long)?;
        let mut settings: Settings = serde_json::from_slice(&data)?;
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
        if self.schema_version < SCHEMA_VERSION {
            // Будущие миграции: match self.schema_version { 0 => {...}, ... }
            self.schema_version = SCHEMA_VERSION;
        }
    }
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
}
