# VISION_MODEL 功能实现文档

## 概述

本功能为 DeepSeek-TUI 添加了 VISION_MODEL 支持，允许用户配置专用的视觉模型来处理所有图片和可视化功能。视觉模型以 subagent 模式运行，具有独立的会话管理，与主模型（DeepSeek）的对话上下文完全隔离。

## 快速开始

### 1. 启用功能

在 `~/.deepseek/config.toml` 中添加：

```toml
# 启用 vision_model feature flag
[features]
vision_model = true

# 配置视觉模型
[vision_model]
model = "gpt-4o"                    # 必需：视觉模型ID
api_key = "YOUR_OPENAI_API_KEY"     # 可选：默认继承主配置的api_key
```

### 2. 使用

启动 DeepSeek-TUI 后，模型会自动获得以下工具：

- **`vision_analyze`** — 分析图片内容。当用户发送图片或要求分析图片时，模型会自动调用此工具。
- **`vision_ocr`** — 从图片中提取文字（OCR功能）。
- **`vision_compare`** — 比较多张图片的差异。
- **`vision_session`** — 管理视觉会话（创建/列表/关闭/清理）。

### 3. 使用场景示例

```
用户: 帮我看看这张截图里的错误信息是什么
→ 模型自动调用 vision_analyze 分析截图

用户: 提取这张发票上的文字
→ 模型自动调用 vision_ocr 提取文字

用户: 对比这两张UI设计图有什么不同
→ 模型自动调用 vision_compare 对比图片
```

## 配置详解

### 完整配置项

```toml
[vision_model]
model = "gpt-4o"                         # 必需：视觉模型ID
provider = "openai"                      # 可选：默认继承主配置的provider
api_key = "YOUR_API_KEY"                 # 可选：默认继承主配置的api_key
base_url = "https://api.openai.com/v1"   # 可选：默认继承主配置的base_url
max_tokens = 4096                        # 可选：最大响应token数（默认4096）
temperature = 0.7                        # 可选：采样温度（默认0.7）
subagent_mode = true                     # 可选：独立会话模式（默认true）
timeout_secs = 120                       # 可选：请求超时秒数（默认120）
```

### 配置继承

未指定的配置项自动从主配置继承：
- `provider` → 继承主配置的 `provider`
- `api_key` → 继承主配置的 `api_key`
- `base_url` → 继承主配置的 `base_url`

### 不同模型的配置示例

```toml
# OpenAI GPT-4o（最简单）
[vision_model]
model = "gpt-4o"
api_key = "sk-xxxxx"

# Claude 3 Opus
[vision_model]
model = "claude-3-opus-20240229"
provider = "anthropic"
api_key = "sk-ant-xxxxx"
base_url = "https://api.anthropic.com/v1"

# 复用主配置的API Key（适用于OpenAI兼容API）
[vision_model]
model = "gpt-4o"
# api_key 自动继承主配置
```

### Feature Flag

`vision_model` 功能通过 feature flag 控制，默认关闭。需要在 `[features]` 中显式启用：

```toml
[features]
vision_model = true   # 默认为 false
```

## 架构设计

### Subagent 模式

视觉模型以 subagent 模式运行，具有以下特点：

1. **独立会话**: 每个视觉任务在独立的会话中执行，不污染主模型上下文
2. **上下文隔离**: 视觉对话历史与主模型完全隔离，节省主模型的context window
3. **资源管理**: 独立的token计数和速率限制
4. **生命周期管理**: 支持会话超时和自动清理

### 数据流

```
用户发送图片或请求图片分析
    ↓
DeepSeek 主模型识别需要视觉处理
    ↓
调用 vision_analyze / vision_ocr 工具
    ↓
VisionSessionManager 获取/创建独立会话
    ↓
VisionClient 发送请求到视觉模型API（如OpenAI）
    ↓
视觉模型返回分析结果
    ↓
结果返回给DeepSeek主模型，主模型整合后回复用户
```

### 工具注册机制

Vision工具通过项目的标准 `ToolSpec` trait 注册：

1. `Feature::VisionModel` feature flag 控制是否启用
2. `config.vision_model_enabled()` 检查是否配置了视觉模型
3. 两个条件都满足时，工具在 `build_turn_tool_registry_builder()` 中注册
4. 注册后工具出现在模型的工具目录中，模型可以自动调用

## 文件变更清单

### 新增文件

| 文件 | 说明 |
|------|------|
| `crates/tui/src/vision/mod.rs` | 视觉模块入口 |
| `crates/tui/src/vision/client.rs` | 视觉模型HTTP客户端（OpenAI兼容API） |
| `crates/tui/src/vision/session.rs` | 独立会话管理（subagent模式） |
| `crates/tui/src/vision/tools.rs` | 视觉工具（实现ToolSpec trait） |

### 修改文件

| 文件 | 变更内容 |
|------|----------|
| `crates/tui/src/config.rs` | 添加 `VisionModelConfig` 结构体、Config字段和方法 |
| `crates/tui/src/main.rs` | 添加 `mod vision;` |
| `crates/tui/src/features.rs` | 添加 `Feature::VisionModel` 枚举和FeatureSpec |
| `crates/tui/src/prompts/base.md` | 在Toolbox部分添加Vision工具说明 |
| `crates/tui/src/tools/registry.rs` | 添加 `with_vision_tools()` builder方法 |
| `crates/tui/src/core/engine/tool_setup.rs` | 添加vision工具条件注册逻辑 |
| `config.example.toml` | 添加 `[vision_model]` 配置段和feature flag |

## 支持的视觉模型

| 模型 | 提供商 | 说明 |
|------|--------|------|
| `gpt-4o` | OpenAI | 推荐，性价比最高 |
| `gpt-4-turbo` | OpenAI | 上一代视觉模型 |
| `claude-3-opus-20240229` | Anthropic | 高精度分析 |
| `claude-3-sonnet-20240229` | Anthropic | 平衡性能和成本 |
| `gemini-1.5-pro` | Google | 大context窗口 |
| 任何OpenAI兼容模型 | 自定义 | 通过 `base_url` 配置 |

## 注意事项

1. **额外费用**: 视觉模型API调用会产生额外费用，与DeepSeek API分开计费
2. **图片大小**: 建议图片文件不超过20MB，大图片需要适当压缩
3. **超时设置**: 默认120秒超时，复杂图片分析可能需要更长时间
4. **安全性**: 图片数据通过base64编码通过HTTPS传输
5. **Feature Flag**: 默认关闭，需要手动启用 `vision_model = true`
