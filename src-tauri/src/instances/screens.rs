//! Скриншоты инстанса (F3, D37-B): список png из `minecraft/screenshots/`,
//! удаление, открытие папки.
//!
//! Миниатюра — честный компромисс без внешних крейтов: маленький файл (до
//! `THUMB_MAX_BYTES`) целиком уходит в data-URL, большой — `None`, UI покажет
//! плейсхолдер. Читаем только `.png` (Minecraft пишет и старые `.jpg` — их
//! не трогаем и не показываем).

use crate::errors::{LauncherError, Result};
use crate::instances::{instance_dir, minecraft_dir, valid_id};
use crate::paths::Paths;
use crate::util::fs::long_path;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::{Path, PathBuf};

/// Файлы больше этого размера уходят в список без превью (`thumb: None`):
/// полный data-URL многометровой png раздувает IPC-пакет без пользы.
const THUMB_MAX_BYTES: u64 = 300_000;

/// Скриншот инстанса для UI (зеркало ShotInfo в src/api/types.ts).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotInfo {
    pub name: String,
    pub bytes: u64,
    pub modified: i64,
    pub thumb: Option<String>,
}

/// Каталог `minecraft/screenshots/` инстанса.
fn screenshots_dir(paths: &Paths, id: &str) -> PathBuf {
    minecraft_dir(&instance_dir(paths, id)).join("screenshots")
}

/// Имя файла скриншота: стебель 1–128, суффикс `.png` (строчно — `.PNG`/.jpg
/// не скриншоты Minecraft по умолчанию). ENV-12: прежний ASCII-регекс делал
/// скриншоты с кириллицей невидимыми и неудаляемыми — Minecraft пишет их с
/// любыми именами. Чёрный список по образцу worlds::valid_world_name:
/// запрещённые Windows `<>:"/\|?*`, управляющие символы, точки/пробелы по
/// краям и `..` — разделителей нет, путь всегда внутри screenshots/.
fn valid_shot_name(name: &str) -> bool {
    const FORBIDDEN: &str = "<>:\"/\\|?*";
    let Some(stem) = name.strip_suffix(".png") else {
        return false;
    };
    if !(1..=128).contains(&stem.chars().count()) {
        return false;
    }
    if stem.contains("..") {
        return false;
    }
    if stem.starts_with('.') || stem.starts_with(' ') {
        return false;
    }
    if stem.ends_with('.') || stem.ends_with(' ') {
        return false;
    }
    !stem.chars().any(|c| FORBIDDEN.contains(c) || c.is_control())
}

/// modified в секундах UNIX (0, если недоступно — скриншот всё равно показываем).
fn modified_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Превью: маленький png целиком в data-URL (base64 как в auth::skins),
/// большой или нечитаемый — None (UI покажет плейсхолдер).
fn thumb_data_url(path: &Path, bytes: u64) -> Option<String> {
    if bytes > THUMB_MAX_BYTES {
        return None;
    }
    let data = std::fs::read(long_path(path)).ok()?;
    Some(format!("data:image/png;base64,{}", STANDARD.encode(&data)))
}

/// Список скриншотов инстанса (F3): png-файлы `screenshots/`, свежие сверху.
/// Нет каталога — пустой список (ещё не играли, не ошибка). Мусор (каталоги,
/// не-png, имена, не прошедшие валидацию) молча пропускается.
pub fn list(paths: &Paths, instance_id: &str) -> Result<Vec<ShotInfo>> {
    valid_id(instance_id)?;
    let dir = screenshots_dir(paths, instance_id);
    let Ok(entries) = std::fs::read_dir(long_path(&dir)) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        // Только обычные файлы: каталоги и симлинки — не скриншоты.
        if !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if !valid_shot_name(&name) {
            continue;
        }
        let Ok(meta) = e.metadata() else {
            continue;
        };
        let bytes = meta.len();
        let modified = modified_secs(&meta);
        let thumb = thumb_data_url(&dir.join(&name), bytes);
        out.push(ShotInfo {
            name,
            bytes,
            modified,
            thumb,
        });
    }
    // Свежие сверху; при равном времени — по имени, порядок стабильный.
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// Удаление скриншота: имя валидируется до обращения к диску, отсутствие —
/// NotFound. Скриншоты — пересоздаваемый мусор (F2), в отличие от миров не
/// идём в корзину ОС: remove_file без PowerShell на каждый файл. Работающий
/// инстанс не блокируем: игра не держит старые скриншоты открытыми.
pub fn delete(paths: &Paths, instance_id: &str, name: &str) -> Result<()> {
    valid_id(instance_id)?;
    if !valid_shot_name(name) {
        return Err(LauncherError::InvalidInput(format!(
            "некорректное имя скриншота: {name:?} (png, запрещены <>:\"/\\|?*, до 128)"
        )));
    }
    // Валидация запрещает разделители и `..` — join не выходит за screenshots/.
    let path = screenshots_dir(paths, instance_id).join(name);
    if !long_path(&path).is_file() {
        return Err(LauncherError::NotFound(format!("скриншот {name}")));
    }
    std::fs::remove_file(long_path(&path))?;
    Ok(())
}

/// Открыть папку скриншотов в проводнике (как instance_open_dir, но каталог
/// НЕ создаём: скриншотов нет — открывать нечего, это NotFound).
pub fn open_folder(paths: &Paths, instance_id: &str) -> Result<()> {
    valid_id(instance_id)?;
    let dir = screenshots_dir(paths, instance_id);
    if !long_path(&dir).is_dir() {
        return Err(LauncherError::NotFound(format!(
            "скриншоты инстанса {instance_id}"
        )));
    }
    tauri_plugin_opener::open_path(dir, None::<&str>).map_err(|e| LauncherError::Internal(format!("открытие папки: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::Instance;
    use std::time::Duration;

    fn setup() -> (tempfile::TempDir, Paths, Instance) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let inst = Instance::new("Тест", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        (dir, paths, inst)
    }

    /// Фейковый скриншот: файл `len` байт в screenshots/ инстанса.
    fn make_shot(paths: &Paths, id: &str, name: &str, len: usize) -> PathBuf {
        let dir = screenshots_dir(paths, id);
        std::fs::create_dir_all(long_path(&dir)).unwrap();
        let path = dir.join(name);
        std::fs::write(long_path(&path), vec![0u8; len]).unwrap();
        path
    }

    /// Явно выставить mtime (секунды UNIX) — сортировка не зависит от
    /// разрешения часов файловой системы.
    fn set_mtime(path: &Path, secs: i64) {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(long_path(path))
            .unwrap();
        f.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(secs as u64))
            .unwrap();
    }

    #[test]
    fn list_empty_without_screenshots_dir() {
        let (_d, paths, inst) = setup();
        assert!(list(&paths, &inst.id).unwrap().is_empty());
    }

    /// A1: некорректный id инстанса отклоняется — traversal невозможен.
    #[test]
    fn list_rejects_bad_instance_id() {
        let (_d, paths, _inst) = setup();
        for id in ["../x", "a/b", ""] {
            let err = list(&paths, id).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "id {id:?} отклонён");
        }
    }

    #[test]
    fn list_two_shots_skips_junk_and_sorts_desc() {
        let (_d, paths, inst) = setup();
        let old = make_shot(&paths, &inst.id, "2026-09-28_16.41.12.png", 100);
        set_mtime(&old, 100);
        let new = make_shot(&paths, &inst.id, "shot-two.png", 200);
        set_mtime(&new, 200);
        // ENV-12: кириллическое имя теперь валидно — скриншот виден.
        let cyr = make_shot(&paths, &inst.id, "снимок.png", 150);
        set_mtime(&cyr, 150);

        // Мусор: не-png, каталог, верхний регистр расширения.
        let dir = screenshots_dir(&paths, &inst.id);
        std::fs::write(long_path(&dir.join("note.jpg")), b"j").unwrap();
        std::fs::create_dir_all(long_path(&dir.join("folder.png"))).unwrap();
        std::fs::write(long_path(&dir.join("upper.PNG")), b"j").unwrap();

        let shots = list(&paths, &inst.id).unwrap();
        assert_eq!(shots.len(), 3, "png с валидными именами (вкл. кириллицу)");
        assert_eq!(shots[0].name, "shot-two.png", "свежие сверху");
        assert_eq!(shots[1].name, "снимок.png");
        assert_eq!(shots[2].name, "2026-09-28_16.41.12.png");
        assert_eq!(shots[0].bytes, 200);
        assert_eq!(shots[1].bytes, 150);
        assert_eq!(shots[2].bytes, 100);
        assert_eq!(shots[0].modified, 200);
        assert_eq!(shots[2].modified, 100);
        // Маленькие файлы получают превью целиком в data-URL.
        let thumb = shots[0].thumb.as_deref().unwrap();
        assert!(thumb.starts_with("data:image/png;base64,"), "превью {thumb}");
    }

    /// Честная миниатюра: ровно 300_000 байт — ещё есть превью, на байт
    /// больше — уже None (UI покажет плейсхолдер).
    #[test]
    fn thumb_none_for_big_file() {
        let (_d, paths, inst) = setup();
        make_shot(&paths, &inst.id, "edge.png", 300_000);
        make_shot(&paths, &inst.id, "big.png", 300_001);

        let shots = list(&paths, &inst.id).unwrap();
        assert_eq!(shots.len(), 2);
        let edge = shots.iter().find(|s| s.name == "edge.png").unwrap();
        let big = shots.iter().find(|s| s.name == "big.png").unwrap();
        assert!(edge.thumb.is_some(), "граница 300_000 включительно");
        assert!(big.thumb.is_none(), "больше порога — без превью");
        assert_eq!(big.bytes, 300_001, "сам файл в списке остаётся");
    }

    #[test]
    fn delete_removes_file_then_not_found() {
        let (_d, paths, inst) = setup();
        let shot = make_shot(&paths, &inst.id, "shot.png", 10);
        delete(&paths, &inst.id, "shot.png").unwrap();
        assert!(!long_path(&shot).exists(), "файл удалён");
        assert!(list(&paths, &inst.id).unwrap().is_empty());

        // Повторное удаление — NotFound, а не ошибка диска.
        let err = delete(&paths, &inst.id, "shot.png").unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    /// Имя валидируется ДО обращения к диску: traversal, разделители,
    /// не-.png, пустое, запрещённые символы и слишком длинный стебель —
    /// InvalidInput; допустимые имена (включая кириллицу, ENV-12) проходят
    /// дальше и дают NotFound (файла нет).
    #[test]
    fn delete_name_validation() {
        let (_d, paths, inst) = setup();
        let too_long = format!("{}.png", "x".repeat(129));
        for bad in [
            "../evil.png",
            "a/b.png",
            r"a\b.png",
            "",
            "bad:name.png",
            "bad<w>name.png",
            "shot.jpg",
            "shot.PNG",
            ".png",
            "..png",
            "shot..png",
            " .png",
            "shot .png",
            too_long.as_str(),
        ] {
            let err = delete(&paths, &inst.id, bad).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "имя {bad:?} отклонено");
        }
        // ENV-12: кириллица и юникод валидны — файл отсутствует, значит NotFound.
        for good in ["снимок.png", "скриншот 2026.png", "скрин.png"] {
            let err = delete(&paths, &inst.id, good).unwrap_err();
            assert_eq!(err.code(), "not_found", "имя {good:?} допустимо");
        }
        // На границе (стебель 128) имя допустимо — файла нет, NotFound.
        let ok = format!("{}.png", "x".repeat(128));
        let err = delete(&paths, &inst.id, ok.as_str()).unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn open_folder_without_dir_is_not_found() {
        let (_d, paths, inst) = setup();
        // Каталога screenshots/ нет — не создаём, а сообщаем NotFound.
        let err = open_folder(&paths, &inst.id).unwrap_err();
        assert_eq!(err.code(), "not_found");
        // Некорректный id отклоняется до каких-либо обращений к диску.
        let err = open_folder(&paths, "../x").unwrap_err();
        assert_eq!(err.code(), "invalid_input");
    }

    /// Контракт IPC: ключи JSON совпадают с ShotInfo в src/api/types.ts
    /// (camelCase; все поля однословные, thumb — строка или null).
    #[test]
    fn shotinfo_serde_matches_ui_contract() {
        let plain = ShotInfo {
            name: "a.png".into(),
            bytes: 12,
            modified: 34,
            thumb: None,
        };
        let json = serde_json::to_string(&plain).unwrap();
        assert!(json.contains("\"name\":\"a.png\""), "json: {json}");
        assert!(json.contains("\"bytes\":12"), "json: {json}");
        assert!(json.contains("\"modified\":34"), "json: {json}");
        assert!(json.contains("\"thumb\":null"), "json: {json}");

        let with_thumb = ShotInfo {
            name: "b.png".into(),
            bytes: 1,
            modified: 2,
            thumb: Some("data:image/png;base64,QQ==".into()),
        };
        let json = serde_json::to_string(&with_thumb).unwrap();
        assert!(
            json.contains("\"thumb\":\"data:image/png;base64,QQ==\""),
            "json: {json}"
        );
    }
}
