//! mc-launcher-v2: ядро лаунчера. Весь бизнес живёт здесь; UI — тонкий слой
//! поверх IPC (спека §4.3).

pub mod auth;
pub mod errors;
pub mod events;
pub mod instances;
pub mod java;
pub mod mojang;
pub mod net;
pub mod paths;
pub mod settings;
pub mod util;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
}
