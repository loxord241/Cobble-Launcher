// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(e) = mc_launcher_v2_lib::run() {
        eprintln!("Ошибка запуска приложения: {e}");
        std::process::exit(1);
    }
}
