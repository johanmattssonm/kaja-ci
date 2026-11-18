use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone)]
struct SessionKeyState(String);

#[tauri::command]
async fn pick_project_folder(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .set_title("Choose a project folder")
        .blocking_pick_folder()
        .map(|path| path.to_string())
}

#[tauri::command]
fn get_session_key(session_key: State<'_, SessionKeyState>) -> String {
    session_key.0.clone()
}

fn generate_session_key() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();

    let mut bytes = [0u8; 32];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = file.read_exact(&mut bytes);
    } else {
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = ((timestamp >> ((index % 8) * 8)) & 0xff) as u8;
        }
    }

    let mut key = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut key, "{byte:02x}");
    }
    key
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let session_key = generate_session_key();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(SessionKeyState(session_key.clone()))
        .setup(move |app| {
            let app_handle = app.handle().clone();
            ctrlc::set_handler(move || {
                app_handle.exit(0);
            })
            .expect("failed to install Ctrl+C handler");

            let session_key = session_key.clone();
            std::thread::spawn(move || {
                let system = actix_web::rt::System::new();
                if let Err(err) = system.block_on(kaja_ci_backend::run_server(session_key)) {
                    eprintln!("Failed to start backend server: {err}");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pick_project_folder,
            get_session_key
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
