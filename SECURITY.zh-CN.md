# 安全策略

[English](SECURITY.md) | [繁體中文](SECURITY.zh-TW.md)

DeepSeek TUI 是一个可直接进行文件操作、shell 执行和网络访问的编码 agent。安全披露会被认真对待。

## 受支持的版本

仅最新的稳定版本接收安全补丁。不对旧版本进行向后移植。

| 版本 | 支持状态 |
|---|---|
| 最新稳定版 | :white_check_mark: |
| 低于最新版 | :x: |

请查看 [releases 页面](https://github.com/Hmbown/DeepSeek-TUI/releases) 获取当前版本。

## 报告漏洞

**请勿就安全漏洞开启公开的 GitHub issue。**

通过以下方式之一私下报告：

- **GitHub 私有安全通告**：[github.com/Hmbown/DeepSeek-TUI/security/advisories/new](https://github.com/Hmbown/DeepSeek-TUI/security/advisories/new)
- **电子邮件**：[security@deepseek-tui.com](mailto:security@deepseek-tui.com) — 请在邮件主题中包含 `[SECURITY]`

报告中请包含：

- 漏洞描述及被利用后的影响
- 复现步骤或概念验证
- 受影响的版本和配置详情
- 任何建议的缓解措施（可选）

## 响应时间线

| 阶段 | 目标时间 |
|---|---|
| 确认收到 | 收到后 48 小时内 |
| 评估 | 5 天内 — 分类严重程度、范围和修复方案 |
| 补丁（严重） | 评估后 14 天内 |
| 补丁（中等/低危） | 下一个功能版本发布时或由维护者安排的另行时间 |
| 披露 | 补丁发布且用户有时间更新之后 |

你将在每个阶段收到状态更新。如果时间线延迟，我们将告知原因和修订后的预估时间。

## 范围

### 在范围内（算作漏洞）

- 通过精心构造的 prompt 或模型响应实现的远程代码执行
- 沙箱逃逸 — 突破 YOLO 模式的工作区边界或 shell `cwd` 限制
- 凭据泄露 — 泄露 API key、token 或环境密钥
- 在预期工作区之外的任意文件读/写（`PathEscape` 绕过）
- 通过 `fetch_url` 或 `web_search` 对内网端点发起的 SSRF
- 未经授权的 MCP 服务器访问或工具调用

### 不在范围内

- 对维护者或贡献者的社会工程攻击
- 对 DeepSeek API 的拒绝服务/速率限制耗尽
- 第三方依赖中的漏洞（请向上游项目报告）
- 需要对受害者机器进行物理访问的攻击
- 未在 DeepSeek TUI 上下文中演示的理论性 ML 模型注入攻击

如果你不确定某个 bug 是否在范围内，仍然请报告。我们会进行分类并回复。

## 名人堂

我们为提交经过验证的安全漏洞的报告者维护一个名人堂。如需被致谢，请在报告中注明你希望使用的名称/handle。

*暂无条目 — 成为第一个吧。*
