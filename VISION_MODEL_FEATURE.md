# VISION_MODEL 功能实现文档

## 概述

本功能为 DeepSeek-TUI 添加了 VISION_MODEL 支持，允许用户配置专用的视觉模型来处理所有图片和可视化功能。视觉模型以 subagent 模式运行，具有独立的会话管理。

## 功能特性

### 1. 配置支持

在 `config.toml` 中添加 `[vision_model]` 配置段：

```toml
[vision_model]
model = "gpt-4o"                    # 必需：视觉模型ID
provider = "openai"                 # 可选：默认为主配置provider
api_key = "YOUR_API_KEY"            # 可选：默认为主配置api_key
base_url = "https://api.openai.com/v1"  # 可选：默认为主配置base_url
max_tokens = 4096                   # 可选：默认4096
temperature = 0.7                   # 可选：默认0.7
subagent_mode = true                # 可选：默认true
timeout_secs = 120                  # 可选：默认120秒
```

### 2. 核心组件

#### VisionClient (`crates/tui/src/vision/client.rs`)
- HTTP客户端，用于与视觉模型API通信
- 支持OpenAI兼容的API格式
- 自动重试机制和错误处理
- 支持base64编码的图片传输

#### VisionSession (`crates/tui/src/vision/session.rs`)
- 独立的会话管理
- 维护对话历史
- 支持多轮对话上下文
- 会话元数据跟踪（token使用量、请求次数等）
- 空闲会话清理

#### VisionSessionManager (`crates/tui/src/vision/session.rs`)
- 管理多个视觉会话
- 会话生命周期管理
- 支持会话列表、查询和清理

#### Vision Tools (`crates/tui/src/vision/tools.rs`)
- `vision_analyze`: 分析图片
- `vision_compare`: 比较多张图片
- `vision_ocr`: 从图片提取文字
- `vision_session`: 管理视觉会话

### 3. 配置继承

视觉模型配置支持从主配置继承：
- 如果未指定 `provider`，继承主配置的 provider
- 如果未指定 `api_key`，继承主配置的 api_key
- 如果未指定 `base_url`，继承主配置的 base_url

### 4. 支持的模型

- GPT-4o (OpenAI)
- Claude 3 系列 (Anthropic)
- Gemini Pro Vision (Google)
- 任何支持OpenAI兼容API的视觉模型

### 5. 使用示例

#### 配置示例

```toml
# 基础配置（使用OpenAI GPT-4o）
[vision_model]
model = "gpt-4o"

# 完整配置（使用不同的provider）
[vision_model]
model = "claude-3-opus-20240229"
provider = "anthropic"
api_key = "sk-ant-xxxxx"
base_url = "https://api.anthropic.com/v1"
max_tokens = 4096
temperature = 0.5
subagent_mode = true
timeout_secs = 120
```

#### 代码使用示例

```rust
use deepseek_tui::vision::{VisionClient, VisionSessionManager, VisionRequest};

// 创建会话管理器
let session_manager = VisionSessionManager::with_config(config);

// 创建新会话
let session = session_manager.create_session(None, Some("Image analysis".to_string())).await?;

// 分析图片
let response = session.analyze_image(
    image_base64,
    "image/png",
    "Describe this image in detail"
).await?;

println!("Analysis: {}", response.content);
```

## 文件变更

### 新增文件

1. `crates/tui/src/vision/mod.rs` - 模块入口
2. `crates/tui/src/vision/client.rs` - 视觉模型客户端
3. `crates/tui/src/vision/session.rs` - 会话管理
4. `crates/tui/src/vision/tools.rs` - 视觉工具

### 修改文件

1. `crates/tui/src/config.rs`
   - 添加 `VisionModelConfig` 结构体
   - 添加配置解析和继承方法
   - 添加 `vision_model_enabled()` 和 `resolve_vision_model_config()` 方法

2. `crates/tui/src/main.rs`
   - 添加 `mod vision;`

3. `config.example.toml`
   - 添加 `[vision_model]` 配置示例

## 架构设计

### Subagent 模式

视觉模型以 subagent 模式运行，具有以下特点：

1. **独立会话**: 每个视觉任务在独立的会话中执行
2. **上下文隔离**: 视觉对话历史与主模型隔离
3. **资源管理**: 独立的token计数和速率限制
4. **生命周期管理**: 支持会话超时和自动清理

### 数据流

```
用户请求图片分析
    ↓
工具注册表路由到 vision_analyze
    ↓
VisionSessionManager 获取/创建会话
    ↓
VisionSession 处理请求
    ↓
VisionClient 发送API请求
    ↓
返回结果并更新会话历史
```

## 测试

运行单元测试：

```bash
cargo test -p deepseek-tui vision
```

## 未来扩展

1. 支持更多的视觉模型提供商
2. 添加批量图片处理功能
3. 支持视频分析
4. 集成到现有的工具链中
5. 添加视觉模型性能监控

## 注意事项

1. 视觉模型API调用可能产生额外费用
2. 大图片文件需要适当的压缩和预处理
3. 建议设置合理的超时时间避免长时间等待
4. 敏感图片数据应注意安全传输
