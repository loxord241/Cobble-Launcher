//! Windows-хелперы процессов (спека §6.1.9: CREATE_NO_WINDOW для java-инсталляторов).

/// Скрыть консольное окно вспомогательного процесса (Windows CREATE_NO_WINDOW).
/// Сама игра запускается С этим флагом? Нет — игра с окном; флаг только для
/// вспомогательных java (детект версий, инсталляторы).
#[cfg(windows)]
pub fn hide_console(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_cmd: &mut std::process::Command) {}
