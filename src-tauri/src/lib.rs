// `pub` so the head-less CLI binary (and other consumers) can reach the
// processing pipeline and batch runner directly.
pub mod commands;
pub mod image;

use commands::batch;
use commands::canvas;
use commands::clipboard_commands;
use commands::detect;
use commands::path_guard::PathWhitelist;
use commands::preview;
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
            batch::get_image_files,
            batch::process_batch,
            batch::cancel_processing,
            detect::detect_file_types,
            preview::preview_image,
            preview::preview_image_data,
            canvas::canvas_layer_thumbnail,
            canvas::compose_canvas,
            clipboard_commands::save_temp_image,
            commands::path_guard::register_allowed_paths,
            update_commands::check_for_updates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
