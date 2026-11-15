#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|_| {
            std::thread::spawn(|| {
                let system = actix_web::rt::System::new();
                if let Err(err) = system.block_on(kaja_ci_backend::run_server()) {
                    eprintln!("Failed to start backend server: {err}");
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
