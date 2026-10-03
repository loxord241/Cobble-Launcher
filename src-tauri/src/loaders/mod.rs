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
