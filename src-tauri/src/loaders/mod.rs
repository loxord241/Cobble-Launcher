//! Загрузчики модов (спека §6.3): Fabric/Quilt — profile JSON с inheritsFrom,
//! NeoForge — headless-инсталлятор. Все ставятся В ИНСТАНС, не глобально.

pub mod fabric;
pub mod forge;
pub mod neoforge;
pub mod quilt;

/// Общий headless-путь инсталляторов Forge-семейства (NeoForge/Forge):
/// shim `launcher_profiles.json` → `--installClient` → id новой версии.
/// Логика была в neoforge::run_installer; Forge использует ту же.
pub fn run_headless_installer(
    java_exe: &std::path::Path,
    installer: &std::path::Path,
    game_dir: &std::path::Path,
) -> crate::errors::Result<String> {
    neoforge::run_installer(installer, java_exe, game_dir)
}

/// id версии безопасен как имя файла `<id>.json` (аудит 2026-10-06): только
/// [A-Za-z0-9._-]. id приходит из удалённых API (Fabric/Quilt meta) — до
/// склейки пути проверяем набор символов; хранение легитимных id не меняется.
pub(crate) fn is_safe_version_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Аудит 2026-10-06: легитимные id (Fabric «fabric-loader-0.15.11-1.20.1»,
    /// ваниль «1.20.1») проходят; разделители пути и прочая экзотика — нет.
    #[test]
    fn safe_version_id_charset() {
        for ok in [
            "fabric-loader-0.15.11-1.20.1",
            "quilt-loader-0.26.0-1.20.1",
            "1.20.1",
            "1.21.1_neoforge-21.1.100",
            "a",
        ] {
            assert!(is_safe_version_id(ok), "должно пройти: {ok}");
        }
        for bad in ["", "../evil", r"..\evil", "a/b", r"a\b", "a b", "версия", "a\nb"] {
            assert!(!is_safe_version_id(bad), "должно быть отклонено: {bad:?}");
        }
    }
}
