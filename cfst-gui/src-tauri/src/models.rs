use serde::{Deserialize, Serialize};

// ---- Settings (persisted to disk) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub selected_group_id: String,
    #[serde(default)]
    pub encrypted_token: Option<EncryptedSecret>,
    #[serde(default)]
    pub ip_file_path: String,
    #[serde(default)]
    pub ipv6_file_path: String,
    #[serde(default)]
    pub output_dir: String,
    #[serde(default)]
    pub cfst_path: String,
    #[serde(default)]
    pub cfst: CfstOptions,
    #[serde(default)]
    pub presets: Vec<PresetItem>,
    #[serde(default)]
    pub auto_upload: bool,
    #[serde(default)]
    pub upload_history: Vec<UploadHistoryItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfstOptions {
    #[serde(default = "default_address_family")]
    pub address_family: String,
    #[serde(default = "default_top")]
    pub top: usize,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_thread_count")]
    pub thread_count: usize,
    #[serde(default = "default_latency_limit")]
    pub latency_limit: u32,
    #[serde(default = "default_httping")]
    pub httping: bool,
    #[serde(default)]
    pub extra_args: String,
    #[serde(default = "default_preset_index")]
    pub preset_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetItem {
    pub name: String,
    pub description: String,
    pub args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadHistoryItem {
    pub time: String,
    pub group_id: String,
    pub group_name: String,
    pub ip_count: usize,
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedSecret {
    pub version: u32,
    pub algorithm: String,
    pub kdf: String,
    pub iterations: u32,
    pub salt: String,
    pub iv: String,
    pub ciphertext: String,
    #[serde(rename = "authTag")]
    pub auth_tag: String,
}

fn default_address_family() -> String {
    "Auto".into()
}
fn default_top() -> usize {
    10
}
fn default_port() -> u16 {
    443
}
fn default_thread_count() -> usize {
    100
}
fn default_latency_limit() -> u32 {
    150
}
fn default_httping() -> bool {
    true
}
fn default_preset_index() -> usize {
    0
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            selected_group_id: String::new(),
            encrypted_token: None,
            ip_file_path: String::new(),
            ipv6_file_path: String::new(),
            output_dir: String::new(),
            cfst_path: String::new(),
            cfst: CfstOptions::default(),
            presets: default_presets(),
            auto_upload: false,
            upload_history: Vec::new(),
        }
    }
}

impl Default for CfstOptions {
    fn default() -> Self {
        Self {
            address_family: default_address_family(),
            top: default_top(),
            port: default_port(),
            thread_count: default_thread_count(),
            latency_limit: default_latency_limit(),
            httping: default_httping(),
            extra_args: String::new(),
            preset_index: default_preset_index(),
        }
    }
}

pub fn default_presets() -> Vec<PresetItem> {
    vec![
        PresetItem {
            name: "HTTPing 标准".into(),
            description: "HTTP 延迟测速，最高延迟 150ms，端口 443，线程 100".into(),
            args: "-httping -tl 150 -tp 443 -n 100".into(),
        },
        PresetItem {
            name: "TCPing 快速".into(),
            description: "TCP 延迟测速，最高延迟 100ms，端口 443，线程 200".into(),
            args: "-tl 100 -tp 443 -n 200".into(),
        },
        PresetItem {
            name: "HTTPS 高延迟".into(),
            description: "HTTPing 测速，延迟上限 300ms，端口 2053".into(),
            args: "-httping -tl 300 -tp 2053 -n 50".into(),
        },
        PresetItem {
            name: "自定义".into(),
            description: "手动输入参数，自由组合".into(),
            args: String::new(),
        },
    ]
}

// ---- CFST execution ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfstArgs {
    pub executable_path: String,
    pub cli_args: Vec<String>,
    pub result_path: String,
    pub family: String,
    pub top: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfstIp {
    pub ip: String,
    pub port: u16,
    pub latency_ms: f64,
    pub download_speed: String,
    pub packet_loss: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<CfstIp>>,
}

// ---- Workers API ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub templates: Vec<NodeTemplate>,
    pub groups: Vec<IpGroup>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTemplate {
    pub id: String,
    pub remark: String,
    pub protocol: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub sni: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub tls: bool,
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub ech: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub ips: Vec<ParsedEndpoint>,
    #[serde(default)]
    pub template_ids: Vec<String>,
    #[serde(default)]
    pub ip_text: String,
    #[serde(default)]
    pub subscription_token: String,
    #[serde(default)]
    pub updated_at: String,
    pub count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedEndpoint {
    pub host: String,
    pub port: Option<u16>,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResult {
    pub ok: bool,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub error: String,
}

// ---- Frontend request/response types ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewResult {
    pub executable_path: String,
    pub args: Vec<String>,
    pub result_path: String,
    pub family: String,
    pub command_line: String,
}
