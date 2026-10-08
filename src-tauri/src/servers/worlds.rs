//! Мир и шаблоны конфигов выделенного сервера: побайтовая копия мира из
//! инстанса (НЕ hardlink — чужой формат стора не нужен), шаблон
//! `server.properties` (пишется только при создании) и `eula.txt`
//! (заглушка `eula=false` при создании, `eula=true` после подтверждения).

use crate::errors::{LauncherError, Result};
use crate::util::fs::{atomic_write, long_path};
use std::path::{Path, PathBuf};

/// Имя каталога мира внутри каталога сервера (совпадает с level-name).
pub const WORLD_DIR_NAME: &str = "world";

/// Файл-лок сеанса клиента: копировать его на сервер нельзя (клиентский артефакт).
const SESSION_LOCK: &str = "session.lock";

/// Ссылка на EULA из ванильного шаблона eula.txt.
const EULA_URL: &str = "https://aka.ms/MinecraftEULA";

/// Имя мира валидно как один компонент пути: 1..=64 символа, без
/// `<>:"/\|?*`, управляющих и `..`, края — не точка/пробел. Кириллица
/// проходит. Те же правила, что у миров инстанса.
fn valid_world_name(world: &str) -> bool {
    const FORBIDDEN: &str = "<>:\"/\\|?*";
    if !(1..=64).contains(&world.chars().count()) {
        return false;
    }
    if world == "." || world == ".." || world.contains("..") {
        return false;
    }
    if world.starts_with('.') || world.starts_with(' ') {
        return false;
    }
    if world.ends_with('.') || world.ends_with(' ') {
        return false;
    }
    !world.chars().any(|c| FORBIDDEN.contains(c) || c.is_control())
}

/// Рекурсивная побайтовая копия дерева (без симлинк-магии): файлы —
/// `fs::copy`, каталоги создаются. `skip_file` — имя файла в ЛЮБОМ каталоге,
/// который не переносится (сейчас — session.lock).
fn copy_tree(src: &Path, dst: &Path, skip_file: &str) -> Result<()> {
    std::fs::create_dir_all(long_path(dst))?;
    for entry in std::fs::read_dir(long_path(src))? {
        let entry = entry?;
        let name = entry.file_name();
        let child_src = entry.path();
        let child_dst = dst.join(&name);
        // P2-ревизии: symlink/junction внутри мира — пропуск с warn, а не
        // падение всей копии (облачные синхронизации и моды так делают).
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            tracing::warn!("copy_world: пропуск symlink {}", entry.path().display());
            continue;
        }
        if ft.is_dir() {
            copy_tree(&child_src, &child_dst, skip_file)?;
        } else if name != skip_file {
            std::fs::copy(long_path(&child_src), long_path(&child_dst))?;
        }
    }
    Ok(())
}

/// Копировать мир `saves/<world_name>` инстанса на сервер: источник —
/// `<instance_dir>/minecraft/saves/<world_name>` (обязан существовать и иметь
/// level.dat), цель — `<server_dir>/world`. Побайтово, без session.lock.
pub fn copy_world(instance_dir: &Path, world_name: &str, server_dir: &Path) -> Result<()> {
    ensure_world_copyable(instance_dir, world_name)?;
    let src = crate::instances::minecraft_dir(instance_dir)
        .join("saves")
        .join(world_name);
    let dst = server_dir.join(WORLD_DIR_NAME);
    copy_tree(&src, &dst, SESSION_LOCK)?;
    tracing::info!("мир {world_name:?} скопирован в {}", dst.display());
    Ok(())
}

/// Экранирование значения для server.properties (формат Java Properties):
/// `\` перед спецсимволами, `=`/`:`/`#`/`!`/ведущие пробелы экранируются,
/// переводы строк схлопываются в пробел, не-ASCII и управляющие — как
/// `\uXXXX` (Properties.load декодирует их всегда, независимо от кодировки
/// файла). Секции цвета (`§X`) из motd вырезаются.
pub fn escape_properties_value(value: &str) -> String {
    // Цветовые секции: '§' + один кодовый символ.
    let mut cleaned = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{a7}' {
            chars.next(); // код секции съедается вместе с параграфом
            continue;
        }
        cleaned.push(c);
    }

    let mut out = String::with_capacity(cleaned.len());
    for (i, c) in cleaned.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '=' | ':' | '#' | '!' => {
                out.push('\\');
                out.push(c);
            }
            ' ' if i == 0 => out.push_str("\\ "),
            '\n' | '\r' | '\t' => out.push(' '),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                // native2ascii: не-ASCII и управляющие → \uXXXX. P2-ревизии:
                // кодпойнты вне BMP (эмодзи) — только суррогатной парой UTF-16,
                // иначе Properties.read срезает \uXXXX до 4 hex и портит строку.
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Шаблон `server.properties` при создании сервера: только важное + разумные
/// дефолты. Существующий файл НЕ перезаписывается (create — единственный
/// писатель; повторный вызов идемпотентен).
pub fn write_properties(
    server_dir: &Path,
    port: u16,
    online_mode: bool,
    level_name: &str,
    motd: &str,
) -> Result<()> {
    let path = server_dir.join("server.properties");
    if long_path(&path).exists() {
        tracing::debug!("server.properties уже есть — не перезаписываю");
        return Ok(());
    }
    let motd = escape_properties_value(motd);
    let text = format!(
        "# server.properties — создано Cobble Launcher при создании сервера\n\
         server-port={port}\n\
         online-mode={online_mode}\n\
         level-name={}\n\
         motd={motd}\n\
         max-players=20\n\
         view-distance=10\n\
         spawn-protection=16\n\
         enable-rcon=false\n",
        escape_properties_value(level_name)
    );
    atomic_write(&path, text.as_bytes())
}

/// Прочитать eula.txt как список строк (файла нет → None).
fn read_eula_lines(dir: &Path) -> Option<Vec<String>> {
    let data = std::fs::read_to_string(long_path(&dir.join("eula.txt"))).ok()?;
    Some(data.lines().map(str::to_string).collect())
}

/// Записать eula.txt с заданным значением `eula=`: строки существующего файла
/// сохраняются (кроме самой `eula=`), при отсутствии файла — ванильный
/// шаблон с комментарием-ссылкой на EULA. Подтверждение (`accepted=true`)
/// вызывает server_accept_eula, заглушка (`false`) — создание.
pub fn write_eula(dir: &Path, accepted: bool) -> Result<()> {
    let mut lines = read_eula_lines(dir).unwrap_or_else(|| {
        vec![
            format!(
                "#By changing the setting below to TRUE you are indicating your agreement to our EULA ({EULA_URL})."
            ),
            format!("#{}", crate::instances::now_secs()),
        ]
    });
    lines.retain(|l| !l.trim_start().to_lowercase().starts_with("eula="));
    lines.push(format!("eula={accepted}"));
    let mut text = lines.join("\n");
    text.push('\n');
    atomic_write(&dir.join("eula.txt"), text.as_bytes())
}

/// Заглушка eula.txt при создании сервера: `eula=false` — сервер без
/// подтверждения лицензии не стартует, это честный контракт Mojang.
pub fn write_eula_stub(dir: &Path) -> Result<()> {
    write_eula(dir, false)
}

/// Принята ли EULA (последняя строка `eula=` равна `true`; файла/строки нет
/// или значение иное → false).
pub fn read_eula_accepted(dir: &Path) -> bool {
    // P2-ревизии: единый парсер для UI и гейта старта — толерантный к регистру
    // и пробелам вокруг «=», побеждает последняя подходящая строка (как
    // Properties у самой игры).
    read_eula_lines(dir)
        .unwrap_or_default()
        .iter()
        .rev()
        .find_map(|l| {
            let (key, value) = l.trim().split_once('=')?;
            key.trim()
                .eq_ignore_ascii_case("eula")
                .then(|| value.trim().to_ascii_lowercase())
        })
        .is_some_and(|v| v == "true")
}

/// Каталог мира сервера (для UI/диагностики).
pub fn world_dir(server_dir: &Path) -> PathBuf {
    server_dir.join(WORLD_DIR_NAME)
}

/// Раннее подтверждение, что мир инстанса скопируется: имя валидно и в
/// `saves/<world>` есть level.dat. Вызывается ДО скачивания серверного ПО,
/// чтобы не качать его ради отказа на кривом мире.
pub fn ensure_world_copyable(instance_dir: &Path, world_name: &str) -> Result<()> {
    if !valid_world_name(world_name) {
        return Err(LauncherError::InvalidInput(format!(
            "некорректное имя мира: {world_name:?} (запрещены <>:\"/\\|?* и управляющие символы, до 64)"
        )));
    }
    let src = crate::instances::minecraft_dir(instance_dir)
        .join("saves")
        .join(world_name);
    if !long_path(&src.join("level.dat")).is_file() {
        return Err(LauncherError::NotFound(format!("мир {world_name}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_name_validation_rejects_traversal() {
        for bad in ["", ".", "..", "../x", "a/b", "a\\b", "a:b", "мир..2", "x ", " x", ".x", "x."] {
            assert!(!valid_world_name(bad), "{bad:?} должно быть отклонено");
        }
        for good in ["Новый мир", "world_1", "my.world", "м".repeat(64).as_str()] {
            assert!(valid_world_name(good), "{good:?} валидно");
        }
        assert!(!valid_world_name(&"x".repeat(65)));
    }

    /// Фикстура мира: level.dat, region-файл, клиентский session.lock.
    /// Мир живёт в `minecraft/saves/` каталога инстанса.
    fn make_world(root: &Path, name: &str) -> PathBuf {
        let w = root.join("minecraft").join("saves").join(name);
        std::fs::create_dir_all(long_path(&w.join("region"))).unwrap();
        std::fs::create_dir_all(long_path(&w.join("DIM-1"))).unwrap();
        std::fs::write(long_path(&w.join("level.dat")), b"LEVEL").unwrap();
        std::fs::write(long_path(&w.join("region/r.0.mca")), b"R").unwrap();
        std::fs::write(long_path(&w.join("DIM-1/r.0.mca")), b"N").unwrap();
        std::fs::write(long_path(&w.join(SESSION_LOCK)), b"lock").unwrap();
        w
    }

    #[test]
    fn copy_world_bytes_without_session_lock() {
        let d = tempfile::tempdir().unwrap();
        let inst_dir = d.path().join("inst");
        make_world(&inst_dir, "Новый мир");

        let srv = d.path().join("srv");
        std::fs::create_dir_all(long_path(&srv)).unwrap();
        copy_world(&inst_dir, "Новый мир", &srv).unwrap();

        let w = srv.join(WORLD_DIR_NAME);
        assert_eq!(std::fs::read(w.join("level.dat")).unwrap(), b"LEVEL");
        assert!(w.join("region/r.0.mca").is_file());
        assert!(w.join("DIM-1/r.0.mca").is_file());
        assert!(
            !w.join(SESSION_LOCK).exists(),
            "session.lock не переносится"
        );

        // Несуществующий мир — NotFound; traversal-имя — InvalidInput.
        assert_eq!(
            copy_world(&inst_dir, "ghost", &srv).unwrap_err().code(),
            "not_found"
        );
        assert_eq!(
            copy_world(&inst_dir, "../escape", &srv).unwrap_err().code(),
            "invalid_input"
        );
        // Каталог без level.dat — не мир.
        std::fs::create_dir_all(long_path(&inst_dir.join("saves/junk"))).unwrap();
        assert_eq!(
            copy_world(&inst_dir, "junk", &srv).unwrap_err().code(),
            "not_found"
        );
    }

    #[test]
    fn ensure_world_copyable_gates_before_download() {
        let d = tempfile::tempdir().unwrap();
        let inst_dir = d.path().join("inst");
        make_world(&inst_dir, "w1");
        assert!(ensure_world_copyable(&inst_dir, "w1").is_ok());
        assert_eq!(
            ensure_world_copyable(&inst_dir, "ghost").unwrap_err().code(),
            "not_found"
        );
        assert_eq!(
            ensure_world_copyable(&inst_dir, "../x").unwrap_err().code(),
            "invalid_input"
        );
    }

    #[test]
    fn properties_template_and_motd_escaping() {
        let d = tempfile::tempdir().unwrap();
        write_properties(d.path(), 25565, false, "world", "Привет: мир = §cTest\nстрока2").unwrap();
        let text = std::fs::read_to_string(d.path().join("server.properties")).unwrap();
        assert!(text.contains("server-port=25565"), "{text}");
        assert!(text.contains("online-mode=false"), "{text}");
        assert!(text.contains("level-name=world"), "{text}");
        assert!(text.contains("max-players=20"), "{text}");
        assert!(text.contains("view-distance=10"), "{text}");
        assert!(text.contains("spawn-protection=16"), "{text}");
        // motd: секции цвета вырезаны, `:` и `=` экранированы, перевод
        // строки — пробел, кириллица — \uXXXX (native2ascii).
        let motd_line = text
            .lines()
            .find(|l| l.starts_with("motd="))
            .unwrap()
            .to_string();
        assert_eq!(
            motd_line,
            "motd=\\u041f\\u0440\\u0438\\u0432\\u0435\\u0442\\: \\u043c\\u0438\\u0440 \\= Test \\u0441\\u0442\\u0440\\u043e\\u043a\\u04302",
            "{motd_line}"
        );
        assert!(!motd_line.contains('\u{a7}'), "секций цвета нет");

        // Повторная запись существующего файла — no-op (контент не меняется).
        write_properties(d.path(), 1234, true, "other", "other").unwrap();
        let again = std::fs::read_to_string(d.path().join("server.properties")).unwrap();
        assert!(again.contains("server-port=25565"), "файл не перезаписан");
    }

    #[test]
    fn escape_keeps_ascii_and_escapes_specials() {
        assert_eq!(escape_properties_value("plain"), "plain");
        assert_eq!(escape_properties_value("a=b:c"), "a\\=b\\:c");
        assert_eq!(escape_properties_value(" back"), "\\ back");
        assert_eq!(escape_properties_value("a\\b"), "a\\\\b");
        assert_eq!(escape_properties_value("A Minecraft Server"), "A Minecraft Server");
        assert_eq!(escape_properties_value("\u{a7}ared plain"), "red plain");
    }

    #[test]
    fn eula_stub_then_accept_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        write_eula_stub(d.path()).unwrap();
        let text = std::fs::read_to_string(d.path().join("eula.txt")).unwrap();
        assert!(text.contains(EULA_URL), "ссылка на EULA в шаблоне: {text}");
        assert!(!read_eula_accepted(d.path()));

        write_eula(d.path(), true).unwrap();
        let text = std::fs::read_to_string(d.path().join("eula.txt")).unwrap();
        assert!(text.contains("eula=true"), "{text}");
        assert!(text.contains(EULA_URL), "комментарий сохранён");
        assert!(text.lines().filter(|l| l.trim().starts_with("eula=")).count() == 1);
        assert!(read_eula_accepted(d.path()));

        // Повторное подтверждение идемпотентно.
        write_eula(d.path(), true).unwrap();
        assert!(read_eula_accepted(d.path()));
    }

    #[test]
    fn read_eula_missing_or_garbage_is_false() {
        let d = tempfile::tempdir().unwrap();
        assert!(!read_eula_accepted(d.path()), "файла нет");
        std::fs::write(d.path().join("eula.txt"), b"eula=false\n#x\neula=nope\n").unwrap();
        assert!(!read_eula_accepted(d.path()));
        // Vanilla смотрит на последнюю строку eula=.
        std::fs::write(d.path().join("eula.txt"), b"eula=false\neula=true\n").unwrap();
        assert!(read_eula_accepted(d.path()));
    }
}
