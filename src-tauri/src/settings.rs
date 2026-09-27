//! Настройки: JSON, атомарная запись, миграции по `schemaVersion` (спека §7).

use crate::errors::Result;
use crate::util::fs::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;

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

    /// Атомарно сохранить.
    pub fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_vec_pretty(self)?;
        atomic_write(path, &data)
    }

    /// Миграции схемы: 0/N → текущая. Пока схема одна — приведение версии.
    fn migrate(&mut self) {
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
}
