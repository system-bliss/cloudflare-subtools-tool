use crate::models::{AppConfig, IpGroup, UploadResult};

fn normalize_base_url(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err("Base URL is required".into());
    }
    let url = if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        format!("https://{}", trimmed)
    } else {
        trimmed.to_string()
    };
    // Strip trailing slash
    Ok(url.trim_end_matches('/').to_string())
}

pub async fn fetch_remote_config(
    base_url: &str,
    api_token: &str,
) -> Result<AppConfig, String> {
    let api_token = api_token.trim();
    if api_token.is_empty() {
        return Err("API token is required".into());
    }

    let url = format!("{}/api/config", normalize_base_url(base_url)?);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_token))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, body));
    }

    response
        .json::<AppConfig>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}

pub async fn fetch_groups(
    base_url: &str,
    api_token: &str,
) -> Result<Vec<IpGroup>, String> {
    let config = fetch_remote_config(base_url, api_token).await?;
    Ok(config.groups)
}

pub async fn upload_group_ips(
    base_url: &str,
    api_token: &str,
    group_id: &str,
    ips: &[String],
) -> Result<UploadResult, String> {
    let api_token = api_token.trim();
    if api_token.is_empty() {
        return Err("API token is required".into());
    }

    let body = ips.join("\n");
    let url = format!(
        "{}/api/groups/{}",
        normalize_base_url(base_url)?,
        group_id
    );

    let client = reqwest::Client::new();
    let response = client
        .put(&url)
        .header("Authorization", format!("Bearer {}", api_token))
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, body));
    }

    response
        .json::<UploadResult>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}
