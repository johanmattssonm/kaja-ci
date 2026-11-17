use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
async fn pick_project_folder(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .set_title("Choose a project folder")
        .blocking_pick_folder()
        .map(|path| path.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            ctrlc::set_handler(move || {
                app_handle.exit(0);
            })
            .expect("failed to install Ctrl+C handler");

            std::thread::spawn(|| {
                let system = actix_web::rt::System::new();
                if let Err(err) = system.block_on(kaja_ci_backend::run_server()) {
                    eprintln!("Failed to start backend server: {err}");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![pick_project_folder])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
