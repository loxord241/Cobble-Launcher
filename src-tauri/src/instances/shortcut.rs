//! Ярлык инстанса на рабочем столе (F4): `.lnk` через PowerShell (WScript.Shell).
//! Цель — текущий exe лаунчера (в dev-сборке это target/debug — ожидаемо),
//! аргументы — `--launch <instance_id>` (парсит bin/mcl.rs). Новые крейты
//! запрещены, поэтому COM делаем штатным PowerShell; экранирование путей —
//! `ps_quote` из instances/mod.rs (A2: удвоение одинарных кавычек).

use crate::errors::Result;

/// Имя файла ярлыка: запрещённые в именах файлов Windows символы
/// `/ \ : * ? " < > |` → `_`, кириллица и пробелы остаются. Пустой результат
/// (в том числе пустое имя инстанса) → «Instance», чтобы `.lnk` не остался
/// без имени.
pub fn sanitize_shortcut_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    if cleaned.is_empty() {
        "Instance".into()
    } else {
        cleaned
    }
}

/// Создать ярлык на рабочем столе, вернуть путь к `.lnk` строкой.
/// Один вызов PowerShell одновременно возвращает путь рабочего стола
/// (`GetFolderPath('Desktop')` — с учётом перенаправления OneDrive) и создаёт
/// ярлык. Существование `.lnk` проверяем по факту: COM может отвалиться без
/// ненулевого кода выхода.
#[cfg(windows)]
pub fn create_desktop(paths: &crate::paths::Paths, instance_id: &str) -> Result<String> {
    use crate::errors::LauncherError;

    // load вызывает valid_id: барьер против path traversal на IPC-границе (A1),
    // плюс даёт имя инстанса для имени файла.
    let inst = crate::instances::load(paths, instance_id)?;
    let name = sanitize_shortcut_name(&inst.name);
    let exe = std::env::current_exe()?;

    // [Console]::OutputEncoding=UTF8 — чтобы путь рабочего стола с кириллицей
    // (локализованные Windows) пришёл в stdout как UTF-8, а не в OEM-кодировке.
    // id уже прошёл valid_id (ASCII-буквы/цифры/-/_) — в литерал безопасен.
    let script = format!(
        "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; \
         $ErrorActionPreference='Stop'; \
         $desk=[Environment]::GetFolderPath('Desktop'); \
         $ws=New-Object -ComObject WScript.Shell; \
         $s=$ws.CreateShortcut((Join-Path $desk '{}.lnk')); \
         $s.TargetPath='{}'; \
         $s.Arguments='instance-launch --id {} --no-wait'; \
         $s.Save(); \
         Write-Output $desk",
        super::ps_quote(&name),
        super::ps_quote(&exe.display().to_string()),
        super::ps_quote(instance_id),
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    crate::util::win::hide_console(&mut cmd);
    let out = cmd.output()?;
    if !out.status.success() {
        return Err(LauncherError::Internal(format!(
            "ярлык: PowerShell: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let desk = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if desk.is_empty() {
        return Err(LauncherError::Internal(
            "ярлык: PowerShell не вернул путь рабочего стола".into(),
        ));
    }
    let lnk = std::path::Path::new(&desk).join(format!("{name}.lnk"));
    if !crate::util::fs::long_path(&lnk).exists() {
        return Err(LauncherError::NotFound(format!("ярлык {}", lnk.display())));
    }
    Ok(lnk.display().to_string())
}

/// Вне Windows ярлыки `.lnk` не поддерживаются (лаунчер целится в Windows,
/// ветка оставлена для компиляции тестов на других ОС).
#[cfg(not(windows))]
pub fn create_desktop(_paths: &crate::paths::Paths, _instance_id: &str) -> Result<String> {
    use crate::errors::LauncherError;
    Err(LauncherError::Internal(
        "ярлык на рабочий стол поддерживается только в Windows".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F4: кириллица и пробелы сохраняются, служебные символы имени файла → `_`.
    #[test]
    fn sanitize_keeps_cyrillic_and_replaces_path_chars() {
        assert_eq!(sanitize_shortcut_name("Мой сервер"), "Мой сервер");
        assert_eq!(
            sanitize_shortcut_name("a/b\\c:d*e?f\"g<h>i|j"),
            "a_b_c_d_e_f_g_h_i_j"
        );
        assert_eq!(sanitize_shortcut_name("Мой <тест>: инстанс?"), "Мой _тест__ инстанс_");
    }

    /// F4: пустое имя не оставляет файлу безымянный `.lnk`.
    #[test]
    fn sanitize_empty_falls_back_to_instance() {
        assert_eq!(sanitize_shortcut_name(""), "Instance");
    }

    // ВАЖНО: create_desktop в юнит-тестах не вызывается — он исполняет
    // PowerShell и пишет реальный .lnk на рабочий стол пользователя
    // (side effects вне временного каталога; проверяется вручную).
}
