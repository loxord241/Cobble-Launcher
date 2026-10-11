//! Фирменный логотип (D38): пять встроенных пиксель-артов (выбор владельца)
//! и пользовательский PNG. Используется как иконка окна (titlebar/таскбар)
//! и в бренд-чипе интерфейса. В игру ничего не попадает.

use crate::errors::{LauncherError, Result};
use crate::paths::Paths;
use tauri::Manager as _;

/// Встроенные логотипы: id → PNG-байты (src-tauri/logos/*.png, 256×256).
pub const BUILTIN: &[(&str, &[u8])] = &[
    ("grass", include_bytes!("../logos/grass.png")),
    ("copper", include_bytes!("../logos/copper.png")),
    ("chest", include_bytes!("../logos/chest.png")),
    ("honey", include_bytes!("../logos/honey.png")),
    ("flat", include_bytes!("../logos/flat.png")),
];

/// Пользовательский PNG, положенный командой logo_set_custom.
fn custom_path(paths: &Paths) -> std::path::PathBuf {
    paths.root().join("logo.png")
}

/// PNG-байты для значения settings.logo. "" и неизвестное = травяной (дефолт),
/// "custom" — файл пользователя (нет файла → дефолт, честно без ошибки).
pub fn resolve_png(logo: &str, paths: &Paths) -> Vec<u8> {
    if logo == "custom" {
        if let Ok(bytes) = std::fs::read(crate::util::fs::long_path(&custom_path(paths))) {
            return bytes;
        }
        return grass().to_vec();
    }
    builtin_png(logo)
        .map(|b| b.to_vec())
        .unwrap_or_else(|| grass().to_vec())
}

/// Дефолтный логотип (владелец выбрал травяной блок с дубом).
pub fn grass() -> &'static [u8] {
    BUILTIN[0].1
}

/// Идентификатор встроенного логотипа по срезу (для include_bytes-поиска).
pub fn builtin_png(logo: &str) -> Option<&'static [u8]> {
    BUILTIN
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(logo))
        .map(|(_, bytes)| *bytes)
}

/// Скопировать выбранный пользователем PNG в каталог данных (атомарно).
/// Только PNG (иконки окна конвертируются нативно); ≤2 МБ.
pub fn set_custom(paths: &Paths, src: &str) -> Result<()> {
    // SEC-1: та же граница доверия, что у иконки инстанса (A16): путь только
    // из домашнего каталога, канонизированный — иначе вебвью могло прочитать
    // любой PNG системы (≤2 МБ) и получить оракул существования файлов.
    crate::instances::content::ensure_icon_source_allowed(std::path::Path::new(src))?;
    let long = crate::util::fs::long_path(std::path::Path::new(src));
    let meta = std::fs::metadata(&long).map_err(LauncherError::from)?;
    if meta.len() > 2 * 1024 * 1024 {
        return Err(LauncherError::InvalidInput(
            "логотип больше 2 МБ — выбери PNG поменьше".into(),
        ));
    }
    let bytes = std::fs::read(&long)?;
    // PNG-магия: не-PNG не пройдёт в иконку окна.
    if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(LauncherError::InvalidInput(
            "ожидается файл PNG (иконка окна конвертируется из PNG)".into(),
        ));
    }
    let dest = custom_path(paths);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(crate::util::fs::long_path(dir))?;
    }
    crate::util::fs::atomic_write(&dest, &bytes)
}

/// Удалить пользовательский логотип (после сброса на встроенный).
pub fn clear_custom(paths: &Paths) {
    let _ = std::fs::remove_file(crate::util::fs::long_path(&custom_path(paths)));
}

/// Применить логотип к иконке главного окна (titlebar/таскбар).
/// Ошибка не роняет настройку — логируем и продолжаем.
pub fn apply_window_icon(app: &tauri::AppHandle, logo: &str, paths: &Paths) {
    let bytes = resolve_png(logo, paths);
    let window = match app.get_webview_window("main") {
        Some(w) => w,
        None => {
            tracing::warn!("branding: главное окно не найдено");
            return;
        }
    };
    match tauri::image::Image::from_bytes(&bytes) {
        Ok(image) => {
            if let Err(e) = window.set_icon(image) {
                tracing::warn!("branding: set_icon: {e}");
            }
        }
        Err(e) => tracing::warn!("branding: PNG не распознан: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_png_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for (id, bytes) in BUILTIN {
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{id}: не PNG");
            assert!(seen.insert(*id), "дубль id {id}");
        }
        assert_eq!(BUILTIN.len(), 5);
    }

    #[test]
    fn empty_and_unknown_fall_back_to_grass() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        let grass = grass();
        assert_eq!(resolve_png("", &paths), grass);
        assert_eq!(resolve_png("чё-то странное", &paths), grass);
        // Встроенный id отдаёт свой PNG.
        assert_eq!(resolve_png("honey", &paths), builtin_png("honey").unwrap());
    }

    #[test]
    fn custom_missing_file_falls_back_to_grass() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        assert_eq!(resolve_png("custom", &paths), grass());
    }

    #[test]
    fn set_custom_copies_png_and_rejects_other() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        let src = dir.path().join("pick.png");
        std::fs::write(&src, b"\x89PNG\r\n\x1a\nrest").unwrap();
        set_custom(&paths, src.to_str().unwrap()).unwrap();
        assert!(custom_path(&paths).exists());
        assert_eq!(resolve_png("custom", &paths), b"\x89PNG\r\n\x1a\nrest");

        let bad = dir.path().join("bad.png");
        std::fs::write(&bad, b"not a png").unwrap();
        assert!(set_custom(&paths, bad.to_str().unwrap()).is_err());
    }
}

#[cfg(test)]
mod sec_tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn set_custom_rejects_paths_outside_home() {
        // SEC-1: чтение PNG ограничено домашним каталогом (граница A16).
        let paths = crate::paths::Paths::new(std::env::temp_dir().join("mcl-sec-branding"));
        let outside = std::path::Path::new("C:\\Windows\\notepad.exe");
        if outside.exists() {
            let err = set_custom(&paths, outside.to_str().unwrap()).unwrap_err();
            assert!(matches!(err, crate::errors::LauncherError::InvalidInput(_)));
        }
    }
}
