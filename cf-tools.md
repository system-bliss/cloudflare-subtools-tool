# cloudflare-subtools

这是一个部署在 Cloudflare Workers 上的订阅集合与优选 IP 更换工具。它支持在后台维护代理节点模板和 IP 分组，并为 Clash Verge/Mihomo、v2rayN 生成可直接订阅的链接。

## 功能

- 单管理员登录，使用 `ADMIN_PASSWORD` 保护后台。
- 支持维护 VLESS、VMess、Trojan 节点模板。
- 支持 IPv4、`IPv4:端口`、IPv6、`[IPv6]:端口` 格式的 IP 分组。
- 每个 IP 分组都会生成随机订阅 Token，订阅链接无需登录即可由客户端拉取。
- 支持 Clash Verge/Mihomo YAML 订阅和 v2rayN Base64 订阅。
- 支持通过 Bearer Token API 读取完整配置、自动更新 IP 分组内容，方便对接独立客户端或自动化工具。

## 环境准备

安装依赖：

```powershell
npm install
```

创建 KV 命名空间：

```powershell
npx wrangler kv namespace create SUBTOOLS_KV
```

将命令返回的 `id` 填入 `wrangler.jsonc` 的 `kv_namespaces[0].id`。

本地开发时，可以复制示例环境变量文件：

```powershell
Copy-Item .dev.vars.example .dev.vars
```

然后把 `.dev.vars` 里的值改成强密码或随机密钥。

线上部署前，需要设置三个 Worker Secret：

```powershell
npx wrangler secret put ADMIN_PASSWORD
npx wrangler secret put API_TOKEN
npx wrangler secret put SESSION_SECRET
```

线上访问域名请使用你自己的 Worker 域名或自定义域名。密码、Token 和域名等部署配置应保存在 Cloudflare Worker Secret 或本地 `.dev.vars` 中，不要提交到仓库。

修改 Wrangler 绑定或 Secret 声明后，重新生成类型：

```powershell
npm run typegen
```

## 常用命令

- `npm run dev`：启动本地 Worker。
- `npm run test`：运行 Vitest 测试。
- `npm run check`：运行 TypeScript 类型检查。
- `npm run typegen`：生成 Worker 绑定类型。
- `npm run deploy`：部署到 Cloudflare Workers。

## 页面与接口

- `GET /`：返回项目元信息。
- `GET /health`：健康检查。
- `GET /login`、`POST /login`：管理员登录。
- `GET /admin`：节点模板和 IP 分组管理后台。
- `GET /api/config`：通过 Bearer Token 返回完整配置。
- `GET /sub/:groupId/:token?format=clash`：返回 Clash Verge/Mihomo YAML 订阅。
- `GET /sub/:groupId/:token?format=v2rayn`：返回 v2rayN Base64 订阅。
- `PUT /api/groups/:groupId/ips`：通过 API 更新 IP 分组。

## API 示例

读取完整配置：

```powershell
Invoke-RestMethod `
  -Method Get `
  -Uri "https://<your-worker-domain>/api/config" `
  -Headers @{ Authorization = "Bearer <API_TOKEN>" }
```

更新 IP 分组：

```powershell
$body = @{
  ips = @(
    "1.1.1.1",
    "1.0.0.1:8443",
    "2606:4700:4700::1111",
    "[2606:4700:4700::1001]:2053"
  )
} | ConvertTo-Json

Invoke-RestMethod `
  -Method Put `
  -Uri "https://<your-worker-domain>/api/groups/<groupId>/ips" `
  -Headers @{ Authorization = "Bearer <API_TOKEN>" } `
  -ContentType "application/json" `
  -Body $body
```

也可以直接提交纯文本：

```powershell
Invoke-RestMethod `
  -Method Put `
  -Uri "https://<your-worker-domain>/api/groups/<groupId>/ips" `
  -Headers @{
    Authorization = "Bearer <API_TOKEN>"
    "Content-Type" = "text/plain"
  } `
  -Body "1.1.1.1`n1.0.0.1:8443"
```

成功响应示例：

```json
{
  "ok": true,
  "groupId": "group-id",
  "count": 2,
  "updatedAt": "2026-05-06T03:00:00.000Z"
}
```

## 订阅说明

在后台创建 IP 分组后，每个分组会显示两条订阅链接：

- Clash Verge/Mihomo：`https://<your-worker-domain>/sub/:groupId/:token?format=clash`
- v2rayN：`https://<your-worker-domain>/sub/:groupId/:token?format=v2rayn`

Clash Verge/Mihomo 订阅会生成完整配置骨架：DNS、HTTP/SOCKS 端口、外部控制器、节点列表、`🚀 节点选择`、按节点模板拆分的 `♻️ 自动选择 <模板名称>`、`🔯 故障转移`、`🔮 负载均衡`、`🎯 全球直连`、`🛑 全球拦截`、`🐟 漏网之鱼`、`☁️ CloudFlareCDN` 分组，以及广告拦截、常用代理、国内直连、Cloudflare CDN、`GEOIP,CN` 和 `MATCH` 兜底规则。TLS 节点默认保留证书校验，并输出 `client-fingerprint: chrome`、WebSocket Host/Path 和 ECH 选项。

订阅链接不需要登录，但必须携带正确的随机 Token。如果链接泄露，可以在后台刷新该分组的订阅 Token。

## 部署说明

部署流程：

```powershell
npm run check
npm run test
npm run deploy
```

如果尚未登录 Cloudflare，先执行：

```powershell
npx wrangler login
```

KV 使用最终一致性模型。通过 API 更新 IP 后，不同 Cloudflare 边缘节点可能需要短暂时间才能都读到最新内容。
