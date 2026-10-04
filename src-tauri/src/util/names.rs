//! Санитизация имён (спека §3: path traversal в путях инстансов/профилей).

use crate::errors::{LauncherError, Result};

/// Символы, запрещённые в файловых компонентах Windows + `..`.
const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Зарезервированные имена устройств Windows (без расширения).
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Мягкая санитизация пользовательского имени (ник, имя инстанса):
/// вырезает запрещённые символы и `..`, схлопывает пробелы, ограничивает длину.
/// Никогда не падает: при пустом результате — `fallback`.
pub fn sanitize_user_name(name: &str, fallback: &str) -> String {
    let mut cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && !FORBIDDEN.contains(c))
        .collect();
    // Секвенции `..` целиком запрещены — убираем точки-точки
    while cleaned.contains("..") {
        cleaned = cleaned.replace("..", ".");
    }
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = cleaned.trim().chars().take(64).collect::<String>();
    // На Windows имя не может заканчиваться на точку или пробел, и не может быть пустым
    let final_name = trimmed.trim_start().trim_end_matches([' ', '.']).to_string();
    let upper = final_name.to_uppercase();
    if final_name.is_empty() || RESERVED.contains(&upper.as_str()) {
        fallback.to_string()
    } else {
        final_name
    }
}

/// Строгая валидация компонента пути из ЧУЖОГО архива: любое нарушение — ошибка.
pub fn validate_archive_component(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(LauncherError::InvalidInput("пустое имя".into()));
    }
    if name.chars().any(|c| c.is_control() || FORBIDDEN.contains(&c)) {
        return Err(LauncherError::InvalidInput(format!(
            "недопустимые символы в имени: {name:?}"
        )));
    }
    if name.contains("..") {
        return Err(LauncherError::InvalidInput(format!(
            "запрещённая секвенция '..' в имени: {name:?}"
        )));
    }
    let upper = name.split('.').next().unwrap_or("").to_uppercase();
    if RESERVED.contains(&upper.as_str()) {
        return Err(LauncherError::InvalidInput(format!(
            "зарезервированное имя устройства Windows: {name:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_unicode_names() {
        assert_eq!(sanitize_user_name("Мой инстанс №1", "x"), "Мой инстанс №1");
    }

    #[test]
    fn strips_traversal_and_separators() {
        assert_eq!(sanitize_user_name("../../etc", "x"), ".etc");
        assert_eq!(sanitize_user_name("a/b\\c:d", "x"), "abcd");
        assert_eq!(sanitize_user_name("bad:name?", "x"), "badname");
    }

    #[test]
    fn falls_back_on_empty() {
        assert_eq!(sanitize_user_name("   ", "Инстанс"), "Инстанс");
        assert_eq!(sanitize_user_name("../..", "Инстанс"), "Инстанс");
    }

    #[test]
    fn handles_reserved_and_trailing_dots() {
        assert_eq!(sanitize_user_name("CON", "Инстанс"), "Инстанс");
        assert_eq!(sanitize_user_name("nul", "Инстанс"), "Инстанс");
        assert_eq!(sanitize_user_name("test...", "Инстанс"), "test");
        assert_eq!(sanitize_user_name("..test..", "Инстанс"), ".test");
    }

    #[test]
    fn archive_component_rejects_dangerous() {
        // Валидация ОДНОГО компонента (без разделителей — пути проверяет
        // util::zip::safe_relative_path).
        assert!(validate_archive_component("jei.jar").is_ok());
        assert!(validate_archive_component("mods/jei.jar").is_err()); // разделитель — не компонент
        assert!(validate_archive_component("..").is_err());
        assert!(validate_archive_component("a:b").is_err());
        assert!(validate_archive_component("con").is_err());
        assert!(validate_archive_component("COM1").is_err());
        assert!(validate_archive_component("nul.txt").is_err());
    }
}
