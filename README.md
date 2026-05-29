# CFST GUI —— CloudflareSpeedTest 优选工具

基于 [Tauri v2](https://v2.tauri.app/) 构建的 Windows 桌面应用，为 [XIU2/CloudflareSpeedTest](https://github.com/XIU2/CloudflareSpeedTest) 提供图形化操作界面，并集成 Cloudflare Workers 后端，支持测速结果一键上传至 IP 分组管理服务。

## 功能概览

- **测速执行** —— 选择预设模式或自定义参数，启动 CloudflareSpeedTest 进行延迟与下载速度测试
- **实时日志** —— 终端风格的日志面板，实时输出 CFST 运行过程
- **结果解析** —— 自动解析 `result.csv`，以表格展示 IP、端口、延迟、下载速度、丢包率
- **IP 分组上传** —— 将优选 IP 上传至 Cloudflare Workers 后端，分配至指定分组
- **Token 加密存储** —— API Token 使用 PBKDF2 + AES-256-GCM 加密落盘，主密码解锁后方可使用
- **命令预览** —— 实际执行前可预览完整的 CLI 命令
- **上传历史** —— 保留最近 50 条上传记录，便于回溯

## 界面预览

应用窗口标题为 **CFST GUI - CloudflareSpeedTest 优选工具**（1100×750，最小 900×650），主要分为以下几个面板：

| 面板 | 说明 |
|------|------|
| 服务设置 | 配置后端地址、API Token、CFST 可执行文件路径、输出目录 |
| 测速模式 | 预设模式选择（标准 HTTPing / 快速 TCPing / HTTPS 高延迟 / 自定义）及参数调整 |
| 上传到 Workers | 从后端拉取 IP 分组，将选中的优选 IP 上传 |
| 上传历史 | 最近的上传操作记录 |
| 命令预览 | 当前将要执行的 CLI 命令 |
| 测速日志 | CFST 进程的实时 stdout 输出 |
| 测速结果 | 表格展示测速结果，支持全选/反选/清除 |

## 技术栈

| 层 | 技术 |
|----|------|
| 桌面框架 | Tauri v2（Rust） |
| 前端 | 原生 HTML + CSS + Vanilla JavaScript |
| HTTP 客户端 | reqwest（Rust） |
| 加密 | PBKDF2-SHA256 + AES-256-GCM |
| 数据解析 | csv crate（Rust） |
| 异步运行时 | Tokio |

## 前置依赖

- Windows 10 或更高版本
- [CloudflareSpeedTest](https://github.com/XIU2/CloudflareSpeedTest/releases) 可执行文件（本仓库已内置 `cfst.exe`）
- 可选的 Cloudflare Workers 后端服务（用于 IP 上传功能）

## 快速开始

### 1. 克隆仓库

```bash
git clone https://github.com/system-bliss/cloudflare-subtools-tool.git
cd cloudflare-subtools-tool
```

### 2. 安装依赖

确保已安装：

- [Rust](https://www.rust-lang.org/tools/install)（1.77+）
- [Node.js](https://nodejs.org/)（用于 Tauri CLI）

```bash
cd cfst-gui
npm install
```

### 3. 开发模式

```bash
npm run dev
```

### 4. 构建发布包

```bash
npm run build
```

构建产物位于 `cfst-gui/src-tauri/target/release/`。

## 使用说明

### 测速流程

1. 在 **服务设置** 中配置 CFST 可执行文件路径和输出目录
2. 在 **测速模式** 中选择预设或调整参数（端口、线程数、延迟上限、地址族等）
3. 点击 **开始测速**，实时查看日志和结果
4. 在结果表格中勾选需要上传的 IP

### Token 加密

1. 在 **服务设置** 中输入 API Token
2. 输入主密码并点击加密 —— Token 将以密文保存至本地配置文件
3. 后续使用时，输入主密码解锁即可

### 上传到后端

1. 确保已完成 Token 解锁
2. 点击 **拉取分组** 获取后端 IP 分组列表
3. 选择目标分组，点击 **上传选中 IP**

## 项目结构

```
cfst_windows_amd64/
├── cfst.exe                     # CloudflareSpeedTest 可执行文件
├── cfst-gui/                    # Tauri 应用源码
│   ├── src/
│   │   ├── index.html           # 主页面
│   │   ├── app.js               # 前端逻辑
│   │   └── styles.css           # 样式
│   └── src-tauri/
│       ├── Cargo.toml           # Rust 依赖
│       ├── tauri.conf.json      # Tauri 配置
│       └── src/
│           ├── main.rs          # 程序入口
│           ├── lib.rs           # 命令注册与状态管理
│           ├── cfst.rs          # CFST 进程管理与结果解析
│           ├── models.rs        # 数据结构定义
│           ├── api_client.rs    # Workers HTTP API 客户端
│           ├── config_store.rs  # 配置文件读写
│           └── crypto_vault.rs  # Token 加解密
├── ip.txt                       # Cloudflare IPv4 CIDR 列表
├── ipv6.txt                     # Cloudflare IPv6 CIDR 列表
└── output/                      # 测速结果输出目录
```

## 相关资源

- [CloudflareSpeedTest](https://github.com/XIU2/CloudflareSpeedTest) —— 底层测速工具
- [Tauri v2 文档](https://v2.tauri.app/) —— 桌面框架
- Cloudflare Workers 后端 —— 配套的 IP 分组管理服务（详见 `cf-tools.md`）

## License

MIT
