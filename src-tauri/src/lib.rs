// `pub` so the head-less CLI binary (and other consumers) can reach the
// processing pipeline and batch runner directly.
pub mod commands;
pub mod image;

use commands::clipboard_commands;
use commands::image_commands;
use commands::path_guard::PathWhitelist;
use commands::update_commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Best-effort cleanup of pasted-image temp files left from previous runs.
    // They are recreated on demand and never referenced again, so clearing
    // the folder at startup keeps disk usage bounded without touching anything
    // else on the system.
    clipboard_commands::cleanup_paste_dir();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(PathWhitelist::default())
        .invoke_handler(tauri::generate_handler![
            image_commands::get_image_files,
            image_commands::process_batch,
            image_commands::detect_file_types,
            image_commands::preview_image,
            image_commands::preview_image_data,
            image_commands::cancel_processing,
            image_commands::export_report,
            clipboard_commands::save_temp_image,
            commands::path_guard::register_allowed_paths,
            update_commands::check_for_updates,
            update_commands::get_current_version,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
