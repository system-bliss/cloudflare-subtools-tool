mod api_client;
mod cfst;
mod config_store;
mod crypto_vault;
pub mod models;

use models::*;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

struct AppState {
    settings: Arc<Mutex<Settings>>,
    unlocked_token: Arc<Mutex<String>>,
    running_cfst: Arc<Mutex<Option<tokio::process::Child>>>,
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    settings: Settings,
) -> Result<(), String> {
    let app_data = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Path error: {}", e))?;
    config_store::save_settings(&app_data, &settings)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
fn encrypt_token(token: String, password: String) -> Result<EncryptedSecret, String> {
    crypto_vault::encrypt_secret(&token, &password)
}

#[tauri::command]
fn unlock_token(
    state: State<'_, AppState>,
    encrypted: EncryptedSecret,
    password: String,
) -> Result<bool, String> {
    match crypto_vault::decrypt_secret(&encrypted, &password) {
        Ok(token) => {
            *state.unlocked_token.lock().unwrap() = token;
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

#[tauri::command]
fn lock_token(state: State<'_, AppState>) -> Result<(), String> {
    *state.unlocked_token.lock().unwrap() = String::new();
    Ok(())
}

#[tauri::command]
fn get_token_status(state: State<'_, AppState>) -> Result<TokenStatus, String> {
    let settings = state.settings.lock().unwrap();
    let unlocked = !state.unlocked_token.lock().unwrap().is_empty();
    Ok(TokenStatus {
        has_encrypted: settings.encrypted_token.is_some(),
        is_unlocked: unlocked,
        base_url: settings.base_url.clone(),
    })
}

#[tauri::command]
async fn fetch_groups(
    state: State<'_, AppState>,
    base_url: String,
    api_token: String,
) -> Result<Vec<IpGroup>, String> {
    let token = resolve_token(&state, &api_token)?;
    api_client::fetch_groups(&base_url, &token).await
}

#[tauri::command]
async fn upload_ips(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    base_url: String,
    api_token: String,
    group_id: String,
    group_name: String,
    ips: Vec<String>,
) -> Result<UploadResult, String> {
    let token = resolve_token(&state, &api_token)?;
    let result = api_client::upload_group_ips(&base_url, &token, &group_id, &ips).await?;

    // Add to upload history
    {
        let mut settings = state.settings.lock().unwrap();
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        settings.upload_history.insert(
            0,
            UploadHistoryItem {
                time: now,
                group_id: group_id.clone(),
                group_name,
                ip_count: ips.len(),
                success: result.ok,
                message: if result.ok {
                    format!("OK: {} IPs updated", ips.len())
                } else {
                    result.error.clone()
                },
            },
        );
        // Keep last 50 entries
        settings.upload_history.truncate(50);

        // Persist
        let app_data = app_handle
            .path()
            .app_data_dir()
            .map_err(|e| format!("Path error: {}", e))?;
        config_store::save_settings(&app_data, &settings)?;
    }

    Ok(result)
}

#[tauri::command]
async fn run_cfst(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    cfst_path: String,
    options: CfstOptions,
    ip_file_path: String,
    ipv6_file_path: String,
) -> Result<Vec<CfstIp>, String> {
    let cfst_args = cfst::resolve_cfst_paths(&options, &cfst_path, &ip_file_path, &ipv6_file_path);

    // Emit start event
    let _ = app_handle.emit(
        "cfst:event",
        RunEvent {
            event_type: "log".into(),
            message: format!(
                "Starting CFST ({}): {} {}\n",
                cfst_args.family,
                cfst_args.executable_path,
                cfst_args.cli_args.join(" ")
            ),
            data: None,
        },
    );

    let running = state.running_cfst.clone();
    let result = cfst::run_cfst(app_handle.clone(), cfst_args, running).await;

    match &result {
        Ok(ips) => {
            let _ = app_handle.emit("cfst:event", RunEvent {
                event_type: "log".into(),
                message: format!("[DEBUG] lib::run_cfst got Ok({} IPs), returning to frontend\n", ips.len()),
                data: None,
            });
        }
        Err(e) => {
            let _ = app_handle.emit("cfst:event", RunEvent {
                event_type: "log".into(),
                message: format!("[DEBUG] lib::run_cfst got Err({})\n", e),
                data: None,
            });
        }
    }

    match &result {
        Ok(ips) => {
            // Empty vec means user stopped — frontend already handles the UI
            if !ips.is_empty() {
                let _ = app_handle.emit(
                    "cfst:event",
                    RunEvent {
                        event_type: "done".into(),
                        message: format!("\nCFST completed. Found {} IPs.", ips.len()),
                        data: Some(ips.clone()),
                    },
                );
            }
        }
        Err(e) => {
            let _ = app_handle.emit(
                "cfst:event",
                RunEvent {
                    event_type: "error".into(),
                    message: e.clone(),
                    data: None,
                },
            );
        }
    }

    result
}

#[tauri::command]
fn stop_cfst(state: State<'_, AppState>) -> Result<bool, String> {
    let mut running = state.running_cfst.lock().unwrap();
    if let Some(ref mut child) = *running {
        let _ = child.start_kill();
        *running = None;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
fn preview_command(cfst_path: String, options: CfstOptions, ip_file_path: String, ipv6_file_path: String) -> Result<PreviewResult, String> {
    cfst::preview_command(&options, &cfst_path, &ip_file_path, &ipv6_file_path)
}

#[tauri::command]
async fn select_cfst_path(_app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("CFST Executable", &["exe", ""])
        .set_title("选择 cfst 可执行文件")
        .pick_file()
        .await;

    Ok(file.map(|f| f.path().to_string_lossy().to_string()))
}

#[tauri::command]
async fn select_ip_file(_app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Text Files", &["txt"])
        .set_title("选择 IP 数据文件")
        .pick_file()
        .await;

    Ok(file.map(|f| f.path().to_string_lossy().to_string()))
}

#[tauri::command]
async fn select_output_dir(_app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    let dir = rfd::AsyncFileDialog::new()
        .set_title("选择输出目录")
        .pick_folder()
        .await;

    Ok(dir.map(|d| d.path().to_string_lossy().to_string()))
}

fn resolve_token(state: &State<'_, AppState>, explicit_token: &str) -> Result<String, String> {
    let token = explicit_token.trim();
    if !token.is_empty() {
        return Ok(token.to_string());
    }
    let unlocked = state.unlocked_token.lock().unwrap();
    if unlocked.is_empty() {
        return Err("API token is locked or empty".into());
    }
    Ok(unlocked.clone())
}

#[derive(serde::Serialize)]
struct TokenStatus {
    has_encrypted: bool,
    is_unlocked: bool,
    base_url: String,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            settings: Arc::new(Mutex::new(Settings::default())),
            unlocked_token: Arc::new(Mutex::new(String::new())),
            running_cfst: Arc::new(Mutex::new(None)),
        })
        .setup(|app| {
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("Path error: {}", e))?;
            let settings = config_store::load_settings(&app_data);
            let state = app.state::<AppState>();
            *state.settings.lock().unwrap() = settings;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            encrypt_token,
            unlock_token,
            lock_token,
            get_token_status,
            fetch_groups,
            upload_ips,
            run_cfst,
            stop_cfst,
            preview_command,
            select_cfst_path,
            select_ip_file,
            select_output_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
