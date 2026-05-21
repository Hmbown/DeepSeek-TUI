# DeepSeek TUI 运维 Runbook

[English](OPERATIONS_RUNBOOK.md) | [繁體中文](OPERATIONS_RUNBOOK.zh-TW.md)

本 runbook 涵盖本地 CLI/TUI 运行时的实用调试和事故响应。

## 快速分类

1. 确认二进制文件 + 配置：
   - `cargo run -- --version`
   - `cat ~/.deepseek/config.toml`（或检查配置的 profile）
2. 启用详细日志：
   - `RUST_LOG=deepseek_cli=debug cargo run`
   - 针对 HTTP 重试/重连：`RUST_LOG=deepseek_cli::client=debug cargo run`
3. 捕获当前状态：
   - `ls ~/.deepseek/sessions`
   - `ls ~/.deepseek/sessions/checkpoints`
   - `ls ~/.deepseek/tasks`

## 事故：回合挂起或流式输出停止

症状：
- TUI 保持在加载状态
- 助手输出不完整，没有完成标记

检查项：
1. 检查重试/健康日志（`deepseek_cli::client`）
2. 验证端点连通性：
   - `curl -sS https://api.deepseek.com/beta/models -H "Authorization: Bearer $DEEPSEEK_API_KEY"`
3. 确认工具输出中没有本地沙箱/权限死锁

操作：
1. 如果有前台 shell 命令正在运行，按 `Ctrl+B` 选择将其放入后台或取消当前回合。
2. 如果命令是在后台启动的，请助手用 `exec_shell_cancel` 和返回的任务 ID 取消它。
3. 当你想停止请求本身时，使用 `Esc` 或 `Ctrl+C` 中断当前回合。
4. 重试 prompt；如果仍然失败，重启 TUI。
5. 重启后，验证前一个已排队/进行中的运行时回合显示为已中断，而非留在运行状态。

## 事故：网络中断 / 离线行为

预期行为：
- 新 prompt 在离线模式激活期间排队
- 队列状态持久化到 `~/.deepseek/sessions/checkpoints/offline_queue.json`

检查项：
1. 在 TUI 中打开队列：`/queue list`
2. 确认队列持久化文件存在且更新时间戳

操作：
1. 恢复连接
2. 重新发送排队条目（通过 `/queue edit <n>` + 回车，或正常输入流程）
3. 确保队列为空时队列文件被清除

## 事故：崩溃恢复

预期行为：
- 检查点存储在 `~/.deepseek/sessions/checkpoints/latest.json`
- 启动时开始一个新的会话，除非提供 `--resume`/`--continue`

操作：
1. 通过 `deepseek --resume <id>` 或 TUI 中的 `Ctrl+R` 显式恢复之前的工作
2. 如需检查点检查，检查 `latest.json` 的 schema 不匹配/详情
3. 如果 schema 比二进制支持的新，升级二进制或移除过时的检查点

## 事故：持久化状态 Schema 错误

症状：
- 类似 `schema vX is newer than supported vY` 的错误

受影响的存储区域：
- sessions（`~/.deepseek/sessions/*.json`）
- runtime 线程/回合/项目记录
- tasks（`~/.deepseek/tasks/tasks/*.json`）

操作：
1. 确认二进制版本和迁移预期
2. 在编辑前备份状态目录
3. 要么：
   - 用兼容的新版本二进制运行，要么
   - 归档不兼容的记录并重新生成状态

## 事故：MCP/工具执行失败

检查项：
1. 验证 `~/.deepseek/mcp.json` schema 和服务器命令路径
2. 确认服务器进程可以手动启动
3. 在 TUI 历史/日志中检查沙箱拒绝记录

操作：
1. 用所需的批准重试（或仅在适当时使用 YOLO）
2. 暂时禁用失败的 MCP 服务器并隔离问题
3. 用 `/mcp` 诊断验证后重新启用

## 事故后检查清单

1. 保留日志和相关状态文件
2. 记录触发条件、影响和缓解措施
3. 添加或更新回归测试（重试/恢复/schema）
4. 如果行为改变，更新本 runbook 和架构文档
