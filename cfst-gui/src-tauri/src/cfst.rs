use crate::models::{CfstArgs, CfstIp, CfstOptions, PreviewResult};
use csv::ReaderBuilder;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Detect platform-specific executable name
pub fn platform_exe_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "cfst.exe"
    } else {
        "cfst"
    }
}

/// Select IP input file based on address family
pub fn select_input_file(
    tool_dir: &str,
    address_family: &str,
    custom_ip_file: &str,
    custom_ipv6_file: &str,
) -> (String, String) {
    let family = if address_family == "IPv6" {
        "IPv6"
    } else {
        "IPv4"
    };

    let default_v4 = Path::new(tool_dir).join("ip.txt");
    let default_v6 = Path::new(tool_dir).join("ipv6.txt");

    let v4_path = if custom_ip_file.is_empty() {
        default_v4.to_string_lossy().to_string()
    } else {
        custom_ip_file.to_string()
    };

    let v6_path = if custom_ipv6_file.is_empty() {
        default_v6.to_string_lossy().to_string()
    } else {
        custom_ipv6_file.to_string()
    };

    let path = if family == "IPv6" { v6_path } else { v4_path };

    (family.to_string(), path)
}

/// Resolve all paths needed to run cfst
pub fn resolve_cfst_paths(options: &CfstOptions, cfst_path: &str, ip_file_path: &str, ipv6_file_path: &str) -> CfstArgs {
    let exe_path = if cfst_path.is_empty() {
        String::new()
    } else {
        cfst_path.to_string()
    };

    let tool_dir = Path::new(&exe_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let (family, input_path) = select_input_file(
        &tool_dir,
        &options.address_family,
        ip_file_path,
        ipv6_file_path,
    );

    let output_dir = Path::new(&tool_dir).join("output");
    let result_path = output_dir.join("result.csv");

    let args = build_cfst_args(
        &input_path,
        &result_path.to_string_lossy(),
        options,
        &family,
    );

    CfstArgs {
        executable_path: exe_path,
        cli_args: args,
        result_path: result_path.to_string_lossy().to_string(),
        family,
    }
}

/// Build the full argument list for cfst
pub fn build_cfst_args(
    input_path: &str,
    result_path: &str,
    options: &CfstOptions,
    _family: &str,
) -> Vec<String> {
    let extra = if options.extra_args.is_empty() {
        vec![]
    } else {
        parse_extra_args(&options.extra_args)
    };

    let extra_has = |flag: &str| -> bool {
        extra.iter().any(|a| a == flag)
    };

    let mut args = vec![
        "-f".to_string(),
        input_path.to_string(),
        "-o".to_string(),
        result_path.to_string(),
    ];

    if !extra_has("-p") {
        args.push("-p".to_string());
        args.push(options.top.to_string());
    }
    if !extra_has("-dn") {
        args.push("-dn".to_string());
        args.push(options.top.to_string());
    }
    if !extra_has("-tp") {
        args.push("-tp".to_string());
        args.push(options.port.to_string());
    }
    if !extra_has("-n") {
        args.push("-n".to_string());
        args.push(options.thread_count.to_string());
    }
    if !extra_has("-tl") {
        args.push("-tl".to_string());
        args.push(options.latency_limit.to_string());
    }

    if options.httping && !extra_has("-httping") {
        args.push("-httping".to_string());
    }

    args.extend(extra);
    args
}

/// Parse a string of extra arguments into individual tokens
fn parse_extra_args(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut quote_char = ' ';

    for ch in value.chars() {
        if in_quote {
            if ch == quote_char {
                in_quote = false;
            } else {
                current.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            in_quote = true;
            quote_char = ch;
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                result.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

/// Run cfst in background, emitting events to the frontend
pub async fn run_cfst(
    app_handle: tauri::AppHandle,
    cfst_args: CfstArgs,
    running: Arc<Mutex<Option<tokio::process::Child>>>,
) -> Result<Vec<CfstIp>, String> {
    let exe = &cfst_args.executable_path;
    if exe.is_empty() || !Path::new(exe).exists() {
        return Err(format!("cfst executable not found: {}", exe));
    }

    let output_dir = Path::new(&cfst_args.result_path)
        .parent()
        .unwrap_or(Path::new("."));
    tokio::fs::create_dir_all(output_dir)
        .await
        .map_err(|e| format!("Failed to create output dir: {}", e))?;

    let mut child = Command::new(exe)
        .args(&cfst_args.cli_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW on Windows, ignored on other platforms
        .spawn()
        .map_err(|e| format!("Failed to start cfst: {}", e))?;

    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

    // Store child process for stopping
    *running.lock().unwrap() = Some(child);

    let handle = running.clone();
    let app_handle_clone = app_handle.clone();

    // Read stdout in background, handling \r for progress bars
    // cfst.exe uses \r to update progress in-place in terminals; in a pipe
    // these are just bytes, so we debounce: capture the latest \r-delimited
    // segment and emit it periodically, while \n-delimited lines go to log.
    let latest_progress = Arc::new(Mutex::new(String::new()));
    let progress_for_timer = latest_progress.clone();
    let timer_app = app_handle.clone();

    // Timer task: emit latest progress every 100ms
    let progress_timer = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let text = progress_for_timer.lock().unwrap().clone();
            if !text.is_empty() {
                let _ = timer_app.emit("cfst:event", crate::models::RunEvent {
                    event_type: "progress".into(),
                    message: text,
                });
            }
        }
    });

    let stdout_handle = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        let mut line_buf: Vec<u8> = Vec::new();
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    for &byte in &buf[..n] {
                        if byte == b'\r' {
                            if !line_buf.is_empty() {
                                let text = String::from_utf8_lossy(&line_buf).to_string();
                                *latest_progress.lock().unwrap() = text;
                                line_buf.clear();
                            }
                        } else if byte == b'\n' {
                            if !line_buf.is_empty() {
                                let text = String::from_utf8_lossy(&line_buf).to_string();
                                let _ = app_handle_clone.emit("cfst:event", crate::models::RunEvent {
                                    event_type: "log".into(),
                                    message: text + "\n",
                                });
                                line_buf.clear();
                            }
                        } else {
                            line_buf.push(byte);
                        }
                    }
                }
                Err(_) => break,
            }
        }
        // Emit any final progress
        let remaining = line_buf;
        if !remaining.is_empty() {
            let text = String::from_utf8_lossy(&remaining).to_string();
            *latest_progress.lock().unwrap() = text;
        }
    });

    // Read stderr in background
    let stderr_app = app_handle.clone();
    let stderr_handle = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    let _ = stderr_app.emit("cfst:event", crate::models::RunEvent {
                        event_type: "log".into(),
                        message: text,
                    });
                }
                Err(_) => break,
            }
        }
    });

    let _ = tokio::join!(stdout_handle, stderr_handle);

    // Stop the progress timer and clear the progress bar
    progress_timer.abort();
    let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
        event_type: "progress".into(),
        message: String::new(),
    });

    // Get the child back and wait for it
    let mut child = handle.lock().unwrap().take().unwrap();
    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed to wait on cfst: {}", e))?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        return Err(format!("cfst exited with code {}", code));
    }

    // Parse result CSV
    parse_cfst_result(&cfst_args.result_path, cfst_args.cli_args.len().saturating_sub(1))
}

/// Parse cfst result.csv to extract IP list
fn parse_cfst_result(result_path: &str, top: usize) -> Result<Vec<CfstIp>, String> {
    let content =
        std::fs::read_to_string(result_path).map_err(|e| format!("Failed to read result: {}", e))?;

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_reader(content.as_bytes());

    let headers = reader
        .headers()
        .map_err(|e| format!("CSV header error: {}", e))?
        .clone();

    // Find the IP column
    let ip_col = headers
        .iter()
        .position(|h| {
            let h = h.to_lowercase();
            h.contains("ip") || h.contains("地址") || h.contains("address")
        })
        .ok_or("CFST result is missing an IP column")?;

    // Find optional columns
    let port_col = headers.iter().position(|h| {
        let h = h.to_lowercase();
        h.contains("port") || h.contains("端口")
    });

    let latency_col = headers.iter().position(|h| {
        let h = h.to_lowercase();
        h.contains("latency")
            || h.contains("延迟")
            || h.contains("avg")
            || h.contains("average")
    });

    let speed_col = headers.iter().position(|h| {
        let h = h.to_lowercase();
        h.contains("speed") || h.contains("速度") || h.contains("download") || h.contains("下载")
    });

    let loss_col = headers.iter().position(|h| {
        let h = h.to_lowercase();
        h.contains("loss")
            || h.contains("丢包")
            || h.contains("packet")
            || h.contains("drop")
    });

    let mut results = Vec::new();
    for record in reader.records().flatten() {
        let ip = record
            .get(ip_col)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if ip.is_empty() {
            continue;
        }
        let port: u16 = port_col
            .and_then(|i| record.get(i))
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(443);
        let latency: f64 = latency_col
            .and_then(|i| record.get(i))
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0);
        let download = speed_col
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "-".into());
        let loss = loss_col
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "-".into());

        results.push(CfstIp {
            ip,
            port,
            latency_ms: latency,
            download_speed: download,
            packet_loss: loss,
        });
    }

    if results.is_empty() {
        return Err("CFST result did not contain any IP rows".into());
    }

    let limit = if top > 0 { top.min(results.len()) } else { results.len() };
    results.truncate(limit);

    Ok(results)
}

/// Build a command preview for the UI
pub fn preview_command(options: &CfstOptions, cfst_path: &str, ip_file_path: &str, ipv6_file_path: &str) -> Result<PreviewResult, String> {
    let exe_path = if cfst_path.is_empty() {
        platform_exe_name().to_string()
    } else {
        cfst_path.to_string()
    };

    let tool_dir = Path::new(&exe_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let (family, input_path) = select_input_file(&tool_dir, &options.address_family, ip_file_path, ipv6_file_path);
    let output_dir = Path::new(&tool_dir).join("output");
    let result_path = output_dir.join("result.csv");

    let args = build_cfst_args(
        &input_path,
        &result_path.to_string_lossy(),
        options,
        &family,
    );

    let command_line = format!("{} {}", exe_path, args.join(" "));

    Ok(PreviewResult {
        executable_path: exe_path,
        args,
        result_path: result_path.to_string_lossy().to_string(),
        family,
        command_line,
    })
}
