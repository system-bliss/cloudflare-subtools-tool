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

    let family = if address_family == "IPv6" {
        "IPv6"
    } else if address_family == "Auto" {
        if has_ipv6_connectivity() { "IPv6" } else { "IPv4" }
    } else {
        "IPv4"
    };

    let path = if family == "IPv6" { v6_path } else { v4_path };

    (family.to_string(), path)
}

/// Check whether the system has working IPv6 connectivity by attempting a TCP
/// connection to a well-known IPv6 address (Cloudflare DNS). Returns true if
/// the connect succeeds, is refused, or times out — all of which mean the IPv6
/// network stack routed the attempt. Returns false only when the OS immediately
/// reports "network unreachable" or "host unreachable".
fn has_ipv6_connectivity() -> bool {
    use std::net::{SocketAddrV6, TcpStream};
    // Cloudflare DNS IPv6 — reliable, globally anycast
    let addr = match "2606:4700:4700::1111".parse::<std::net::Ipv6Addr>() {
        Ok(ip) => SocketAddrV6::new(ip, 53, 0, 0),
        Err(_) => return false,
    };
    match TcpStream::connect_timeout(&addr.into(), std::time::Duration::from_millis(1200)) {
        Ok(stream) => {
            drop(stream);
            true
        }
        Err(ref e) => {
            matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::WouldBlock
            )
        }
    }
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
        top: options.top,
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

    // Remove stale result.csv from a previous run so the polling loop
    // does not detect it prematurely and kill the new process.
    let _ = tokio::fs::remove_file(&cfst_args.result_path).await;

    let mut child = Command::new(exe)
        .args(&cfst_args.cli_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW on Windows, ignored on other platforms
        .spawn()
        .map_err(|e| format!("Failed to start cfst: {}", e))?;

    let stdout_pipe = child.stdout.take().unwrap();
    let stderr_pipe = child.stderr.take().unwrap();

    // Store child process for stopping
    *running.lock().unwrap() = Some(child);

    let handle = running.clone();
    let app_handle_clone = app_handle.clone();

    // Read stdout in background.
    let stdout_handle = tokio::spawn(read_stdout(stdout_pipe, app_handle_clone));

    // Read stderr in background
    let stderr_handle = tokio::spawn(read_stderr(stderr_pipe, app_handle.clone()));

    let mut child = match handle.lock().unwrap().take() {
        Some(c) => c,
        None => {
            let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
                event_type: "log".into(),
                message: "[DEBUG] child was None — stop_cfst likely called\n".into(),
                data: None,
            });
            return Ok(Vec::new());
        }
    };

    let child_id = child.id().unwrap_or(0);
    let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
        event_type: "log".into(),
        message: format!("[DEBUG] Waiting for cfst process (pid={}) to exit...\n", child_id),
        data: None,
    });

    // Wait for the process to exit or for result.csv to appear.
    // No timeout — let cfst run as long as it needs.
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Path::new(&cfst_args.result_path).exists() {
                    let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
                        event_type: "log".into(),
                        message: "[DEBUG] result.csv exists while process still running, waiting 2s grace then killing\n".into(),
                        data: None,
                    });
                    let grace = tokio::time::sleep(std::time::Duration::from_secs(2));
                    tokio::pin!(grace);
                    let status = tokio::select! {
                        s = child.wait() => {
                            s.map_err(|e| format!("Failed to wait on cfst: {}", e))
                        }
                        _ = &mut grace => {
                            let _ = child.start_kill();
                            let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
                                event_type: "log".into(),
                                message: "[DEBUG] cfst process killed (did not exit after result.csv wrote)\n".into(),
                                data: None,
                            });
                            child.wait().await.map_err(|e| format!("Failed to wait on cfst: {}", e))
                        }
                    };
                    break status?;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Err(e) => return Err(format!("Failed to wait on cfst: {}", e)),
        }
    };

    let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
        event_type: "log".into(),
        message: format!("[DEBUG] cfst process exited with code {:?}\n", exit_status.code()),
        data: None,
    });

    // Give readers a short window to flush any buffered output
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        async { let _ = tokio::join!(stdout_handle, stderr_handle); },
    ).await;

    if !exit_status.success() {
        let code = exit_status.code().unwrap_or(-1);
        // If result file exists, try to parse it anyway — cfst may have
        // completed its work even if the process exit code is non-zero
        if Path::new(&cfst_args.result_path).exists() {
            let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
                event_type: "log".into(),
                message: format!("[DEBUG] result.csv exists despite exit code {}, proceeding to parse\n", code),
                data: None,
            });
        } else {
            return Err(format!("cfst exited with code {}", code));
        }
    }

    // Parse result CSV
    let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
        event_type: "log".into(),
        message: format!("[DEBUG] Parsing result CSV: path={} top={}\n", cfst_args.result_path, cfst_args.top),
        data: None,
    });
    let result = parse_cfst_result(&cfst_args.result_path, cfst_args.top);
    match &result {
        Ok(ips) => {
            let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
                event_type: "log".into(),
                message: format!("[DEBUG] Parsed {} IPs from CSV\n", ips.len()),
                data: None,
            });
        }
        Err(e) => {
            let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
                event_type: "log".into(),
                message: format!("[DEBUG] CSV parse error: {}\n", e),
                data: None,
            });
        }
    }
    result
}

async fn read_stdout(mut stdout: tokio::process::ChildStdout, app_handle: tauri::AppHandle) {
    let mut buf = [0u8; 4096];
    let mut line_buf: Vec<u8> = Vec::new();
    loop {
        match stdout.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                for &byte in &buf[..n] {
                    if byte == b'\r' || byte == b'\n' {
                        if !line_buf.is_empty() {
                            let text = String::from_utf8_lossy(&line_buf).to_string();
                            let trimmed = text.trim().to_string();
                            if !trimmed.is_empty() {
                                let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
                                    event_type: "log".into(),
                                    message: trimmed + "\n",
                                    data: None,
                                });
                            }
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
    if !line_buf.is_empty() {
        let text = String::from_utf8_lossy(&line_buf).to_string();
        let trimmed = text.trim().to_string();
        if !trimmed.is_empty() {
            let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
                event_type: "log".into(),
                message: trimmed + "\n",
                data: None,
            });
        }
    }
}

async fn read_stderr(mut stderr: tokio::process::ChildStderr, app_handle: tauri::AppHandle) {
    let mut buf = [0u8; 4096];
    loop {
        match stderr.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let _ = app_handle.emit("cfst:event", crate::models::RunEvent {
                    event_type: "log".into(),
                    message: text,
                    data: None,
                });
            }
            Err(_) => break,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cfst_result_with_cfst_output_format() {
        // Simulate the exact CSV format that XIU2/CloudflareSpeedTest produces.
        // Chinese headers: IP 地址,已发送,已接收,丢包率,平均延迟,下载速度(MB/s),地区码
        let csv_content = "\
IP \u{5730}\u{5740},\u{5df2}\u{53d1}\u{9001},\u{5df2}\u{63a5}\u{6536},\u{4e22}\u{5305}\u{7387},\u{5e73}\u{5747}\u{5ef6}\u{8fdf},\u{4e0b}\u{8f7d}\u{901f}\u{5ea6}(MB/s),\u{5730}\u{533a}\u{7801}
104.18.127.221,4,4,0.00,141.48,58.91,LAX
104.20.35.77,4,4,0.00,141.68,57.93,LAX
104.20.2.4,4,4,0.00,143.04,56.04,LAX
";

        let dir = std::env::temp_dir();
        let path = dir.join("cfst_test_result.csv");
        std::fs::write(&path, csv_content).unwrap();

        let result =
            parse_cfst_result(&path.to_string_lossy(), 10).expect("parse should succeed");

        // Cleanup
        let _ = std::fs::remove_file(&path);

        assert_eq!(result.len(), 3, "should parse all 3 rows");
        assert_eq!(result[0].ip, "104.18.127.221");
        assert_eq!(result[0].port, 443); // no port column, defaults to 443
        assert!((result[0].latency_ms - 141.48).abs() < 0.01);
        assert_eq!(result[0].download_speed, "58.91");
        assert_eq!(result[0].packet_loss, "0.00");
    }

    #[test]
    fn test_parse_cfst_result_top_limit() {
        let csv_content = "\
IP \u{5730}\u{5740},\u{5e73}\u{5747}\u{5ef6}\u{8fdf},\u{4e0b}\u{8f7d}\u{901f}\u{5ea6}(MB/s)
1.1.1.1,10.0,100.0
2.2.2.2,20.0,200.0
3.3.3.3,30.0,300.0
4.4.4.4,40.0,400.0
5.5.5.5,50.0,500.0
";

        let dir = std::env::temp_dir();
        let path = dir.join("cfst_test_top.csv");
        std::fs::write(&path, csv_content).unwrap();

        let result =
            parse_cfst_result(&path.to_string_lossy(), 3).expect("parse should succeed");

        let _ = std::fs::remove_file(&path);

        assert_eq!(result.len(), 3, "should truncate to top=3");
        assert_eq!(result[0].ip, "1.1.1.1");
        assert_eq!(result[2].ip, "3.3.3.3");
    }

    #[test]
    fn test_parse_cfst_result_empty_file() {
        let csv_content = "IP \u{5730}\u{5740},\u{5e73}\u{5747}\u{5ef6}\u{8fdf}\n";

        let dir = std::env::temp_dir();
        let path = dir.join("cfst_test_empty.csv");
        std::fs::write(&path, csv_content).unwrap();

        let result = parse_cfst_result(&path.to_string_lossy(), 10);

        let _ = std::fs::remove_file(&path);

        assert!(result.is_err(), "should fail when no data rows exist");
        assert!(
            result.unwrap_err().contains("did not contain any IP rows"),
            "error message should mention missing IP rows"
        );
    }

    #[test]
    fn test_parse_cfst_result_file_not_found() {
        let result = parse_cfst_result("nonexistent_file.csv", 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read result"));
    }

    // ---- parse_extra_args tests ----

    #[test]
    fn test_parse_extra_args_simple() {
        let result = parse_extra_args("-a -b -c");
        assert_eq!(result, vec!["-a", "-b", "-c"]);
    }

    #[test]
    fn test_parse_extra_args_with_quotes() {
        let result = parse_extra_args("-name \"hello world\" -flag 'single quoted'");
        assert_eq!(result, vec!["-name", "hello world", "-flag", "single quoted"]);
    }

    #[test]
    fn test_parse_extra_args_empty() {
        let result = parse_extra_args("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_extra_args_whitespace_only() {
        let result = parse_extra_args("   \t  ");
        assert!(result.is_empty());
    }

    // ---- build_cfst_args tests ----

    #[test]
    fn test_build_cfst_args_defaults() {
        let options = CfstOptions::default();
        let args = build_cfst_args("ip.txt", "output/result.csv", &options, "IPv4");
        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&"ip.txt".to_string()));
        assert!(args.contains(&"-o".to_string()));
        assert!(args.contains(&"output/result.csv".to_string()));
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"10".to_string())); // default top
        assert!(args.contains(&"-httping".to_string()));
    }

    #[test]
    fn test_build_cfst_args_extra_overrides_default() {
        let mut options = CfstOptions::default();
        options.extra_args = "-p 20 -n 50".to_string();
        let args = build_cfst_args("ip.txt", "out.csv", &options, "IPv4");
        // Extra args should prevent adding -p and -n (they're already in extra)
        assert_eq!(args.iter().filter(|a| *a == "-p").count(), 1, "only one -p flag");
        assert_eq!(args.iter().filter(|a| *a == "-n").count(), 1, "only one -n flag");
        // Should have the overridden values
        let p_idx = args.iter().position(|a| a == "-p").unwrap();
        assert_eq!(args[p_idx + 1], "20");
        let n_idx = args.iter().position(|a| a == "-n").unwrap();
        assert_eq!(args[n_idx + 1], "50");
    }

    #[test]
    fn test_build_cfst_args_no_httping() {
        let mut options = CfstOptions::default();
        options.httping = false;
        let args = build_cfst_args("ip.txt", "out.csv", &options, "IPv4");
        assert!(!args.contains(&"-httping".to_string()));
    }

    // ---- select_input_file tests ----

    #[test]
    fn test_select_input_file_default_v4() {
        let tool_dir = "C:\\test\\tools";
        let (family, path) = select_input_file(tool_dir, "IPv4", "", "");
        assert_eq!(family, "IPv4");
        assert!(path.contains("ip.txt"));
        assert!(!path.contains("ipv6"));
    }

    #[test]
    fn test_select_input_file_default_v6() {
        let tool_dir = "C:\\test\\tools";
        let (family, path) = select_input_file(tool_dir, "IPv6", "", "");
        assert_eq!(family, "IPv6");
        assert!(path.contains("ipv6.txt"));
    }

    #[test]
    fn test_select_input_file_custom_paths() {
        let (family, path) = select_input_file("C:\\tools", "IPv4", "D:\\custom.txt", "D:\\custom6.txt");
        assert_eq!(family, "IPv4");
        assert_eq!(path, "D:\\custom.txt");
    }

    #[test]
    fn test_select_input_file_custom_v6_path() {
        let (family, path) = select_input_file("C:\\tools", "IPv6", "D:\\custom.txt", "D:\\custom6.txt");
        assert_eq!(family, "IPv6");
        assert_eq!(path, "D:\\custom6.txt");
    }

    // ---- parse_cfst_result additional edge cases ----

    #[test]
    fn test_parse_cfst_result_minimal_columns() {
        // CSV with only an IP column — no optional columns at all
        let csv_content = "IP\n1.1.1.1\n2.2.2.2\n";
        let dir = std::env::temp_dir();
        let path = dir.join("cfst_test_minimal.csv");
        std::fs::write(&path, csv_content).unwrap();
        let result = parse_cfst_result(&path.to_string_lossy(), 10).expect("parse should succeed");
        let _ = std::fs::remove_file(&path);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].ip, "1.1.1.1");
        assert_eq!(result[0].port, 443); // default
        assert_eq!(result[0].latency_ms, 0.0); // default
        assert_eq!(result[0].download_speed, "-"); // default
        assert_eq!(result[0].packet_loss, "-"); // default
    }

    #[test]
    fn test_parse_cfst_result_english_headers() {
        let csv_content = "IP Address,Port,Latency(ms),Download Speed(MB/s),Packet Loss(%)\n\
                           1.1.1.1,443,10.5,100.2,0.1\n\
                           2.2.2.2,8080,20.3,50.0,1.5\n";
        let dir = std::env::temp_dir();
        let path = dir.join("cfst_test_english.csv");
        std::fs::write(&path, csv_content).unwrap();
        let result = parse_cfst_result(&path.to_string_lossy(), 10).expect("parse should succeed");
        let _ = std::fs::remove_file(&path);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].ip, "1.1.1.1");
        assert_eq!(result[0].port, 443);
        assert!((result[0].latency_ms - 10.5).abs() < 0.01);
        assert_eq!(result[0].download_speed, "100.2");
        assert_eq!(result[0].packet_loss, "0.1");
        assert_eq!(result[1].port, 8080);
    }

    #[test]
    fn test_parse_cfst_result_top_zero_means_all() {
        let csv_content = "IP,Latency\n1.1.1.1,10.0\n2.2.2.2,20.0\n3.3.3.3,30.0\n";
        let dir = std::env::temp_dir();
        let path = dir.join("cfst_test_top0.csv");
        std::fs::write(&path, csv_content).unwrap();
        let result = parse_cfst_result(&path.to_string_lossy(), 0).expect("parse should succeed");
        let _ = std::fs::remove_file(&path);
        assert_eq!(result.len(), 3, "top=0 should return all results");
    }

    #[test]
    fn test_parse_cfst_result_top_exceeds_rows() {
        let csv_content = "IP,Latency\n1.1.1.1,10.0\n2.2.2.2,20.0\n";
        let dir = std::env::temp_dir();
        let path = dir.join("cfst_test_top100.csv");
        std::fs::write(&path, csv_content).unwrap();
        let result = parse_cfst_result(&path.to_string_lossy(), 100).expect("parse should succeed");
        let _ = std::fs::remove_file(&path);
        assert_eq!(result.len(), 2, "top > row count should return all rows");
    }

    #[test]
    fn test_parse_cfst_result_blank_lines_ignored() {
        let csv_content = "IP,Latency\n1.1.1.1,10.0\n\n2.2.2.2,20.0\n\n";
        let dir = std::env::temp_dir();
        let path = dir.join("cfst_test_blank.csv");
        std::fs::write(&path, csv_content).unwrap();
        let result = parse_cfst_result(&path.to_string_lossy(), 10).expect("parse should succeed");
        let _ = std::fs::remove_file(&path);
        assert_eq!(result.len(), 2, "blank lines should be ignored");
    }

    // ---- preview_command tests ----

    #[test]
    fn test_preview_command_basic() {
        let options = CfstOptions::default();
        let result = preview_command(&options, "C:\\cfst\\cfst.exe", "", "");
        assert!(result.is_ok());
        let preview = result.unwrap();
        assert!(preview.command_line.contains("cfst.exe"));
        assert!(preview.command_line.contains("-f"));
        assert!(preview.command_line.contains("-o"));
    }

    #[test]
    fn test_preview_command_empty_path_uses_default_name() {
        let options = CfstOptions::default();
        let result = preview_command(&options, "", "", "");
        assert!(result.is_ok());
        let preview = result.unwrap();
        assert!(preview.command_line.contains(platform_exe_name()));
    }

    #[test]
    fn test_resolve_cfst_paths_basic() {
        let options = CfstOptions::default();
        let result = resolve_cfst_paths(&options, "C:\\cfst\\cfst.exe", "", "");
        assert_eq!(result.executable_path, "C:\\cfst\\cfst.exe");
        assert_eq!(result.family, "IPv4"); // default when no IPv6
        assert_eq!(result.top, 10);
        assert!(result.result_path.contains("result.csv"));
    }

    #[test]
    fn test_resolve_cfst_paths_empty_exe() {
        let options = CfstOptions::default();
        let result = resolve_cfst_paths(&options, "", "", "");
        assert_eq!(result.executable_path, "");
        // Even with empty exe path, args and result_path should still be generated
        assert!(!result.cli_args.is_empty());
        assert!(result.result_path.contains("result.csv"));
    }
}
