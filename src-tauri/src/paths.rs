//! Все пути лаунчера в одном месте (спека §4.2 paths.rs, §7).

use std::path::PathBuf;

/// Корень данных лаунчера и стандартные подкаталоги.
#[derive(Debug, Clone)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    /// Явный корень (CLI-флаг или настройка онбординга).
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Каталог по умолчанию: `%LOCALAPPDATA%\mc-launcher-v2` (спека §8: AppData).
    pub fn default_root() -> PathBuf {
        if let Some(local) = dirs::data_local_dir() {
            return local.join("mc-launcher-v2");
        }
        // Fallback: домашний каталог
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".mc-launcher-v2")
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Кэш манифестов (TTL 1 час — спека §7).
    pub fn manifests_cache(&self) -> PathBuf {
        self.cache_dir().join("manifests")
    }

    /// Контентный стор: клиенты/библиотеки/ассеты по хэшу один раз (спека §6.12).
    pub fn store_dir(&self) -> PathBuf {
        self.root.join("store")
    }

    pub fn clients_store(&self) -> PathBuf {
        self.store_dir().join("clients")
    }

    pub fn libraries_store(&self) -> PathBuf {
        self.store_dir().join("libraries")
    }

    pub fn assets_store(&self) -> PathBuf {
        self.store_dir().join("assets")
    }

    /// Виртуальные (legacy) ассеты: `assets/virtual/<id>/`.
    pub fn assets_virtual(&self, assets_id: &str) -> PathBuf {
        self.root.join("assets").join("virtual").join(assets_id)
    }

    /// Распакованные нативы версии: `bin/<versionId>/` относительно root.
    pub fn natives_dir(&self, version_id: &str) -> PathBuf {
        self.root.join("bin").join(version_id)
    }

    /// Скачанные JRE: `runtime/jdk{major}` (спека §6.5).
    pub fn runtime_dir(&self) -> PathBuf {
        self.root.join("runtime")
    }

    pub fn java_runtime(&self, major: u32) -> PathBuf {
        self.runtime_dir().join(format!("jdk{major}"))
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn accounts_file(&self) -> PathBuf {
        self.root.join("accounts.json")
    }

    /// Недокачанная очередь загрузок (спека §4.4).
    pub fn queue_file(&self) -> PathBuf {
        self.store_dir().join("queue.json")
    }

    /// Инстансы (M2+).
    pub fn instances_dir(&self) -> PathBuf {
        self.root.join("instances")
    }

    /// Каталог игры для CLI-прогона M1 (до появления инстансов).
    pub fn cli_run_dir(&self, mc_version: &str) -> PathBuf {
        self.root.join("cli-run").join(mc_version)
    }

    /// Создать стандартные каталоги.
    pub fn ensure_dirs(&self) -> crate::errors::Result<()> {
        for d in [
            self.cache_dir(),
            self.manifests_cache(),
            self.store_dir(),
            self.clients_store(),
            self.libraries_store(),
            self.assets_store(),
            self.runtime_dir(),
            self.logs_dir(),
        ] {
            std::fs::create_dir_all(crate::util::fs::long_path(&d))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdirs_are_under_root() {
        let p = Paths::new(PathBuf::from(r"C:\data\mcl"));
        assert!(p.libraries_store().starts_with(p.store_dir()));
        assert!(p.queue_file().starts_with(p.store_dir()));
        assert_eq!(
            p.assets_virtual("legacy"),
            PathBuf::from(r"C:\data\mcl\assets\virtual\legacy")
        );
    }

    #[test]
    fn default_root_is_absolute() {
        assert!(Paths::default_root().is_absolute());
    }
}
