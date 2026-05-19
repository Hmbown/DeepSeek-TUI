# 上下文与记忆系统架构重构方案

> 解决当前全量历史消息导致的 token 过度消耗问题，引入分层上下文管理与检索式记忆系统。

## 问题陈述

当前架构中，每次 API 请求都携带**完整对话历史**（`self.session.messages.clone()`），导致：

- 第 N 轮请求消耗 O(N) token，随对话线性增长
- 100 轮对话后，单次请求可达 500K+ tokens
- 自动压缩阈值 50 万 token，普通对话几乎永不触发
- 项目知识、用户偏好无法跨会话积累
- `memory.md` 扁平注入，无检索，无分层

## 总体架构

```
┌──────────────────────────────────────────────────────────────────┐
│                      Tier 1: 即时窗口                             │
│                  Verbatim Window (最近 N 轮)                       │
│       精度优先：tool call/result 的配对不能被检索替代              │
│       精确保留，全量发送                                          │
├──────────────────────────────────────────────────────────────────┤
│                      Tier 2: 历史摘要                             │
│                  Compaction + Embedding                           │
│       旧轮次 → LLM 摘要 + 向量化                                  │
│       按需检索，注入 system prompt                                │
├──────────────────────────────────────────────────────────────────┤
│                      Tier 3: 持久记忆                             │
│                  User Memory (Vector DB)                          │
│       用户偏好、项目约定、架构决策                                │
│       跨会话持久化，检索式注入                                    │
│       替换当前扁平的 memory.md                                    │
├──────────────────────────────────────────────────────────────────┤
│                      Tier 4: 代码知识                             │
│                  Code Embedding                                   │
│       代码库向量化 + 文件结构索引                                 │
│       编辑/读取相关文件时自动检索上下文                           │
└──────────────────────────────────────────────────────────────────┘
```

## Tier 1: 即时窗口 (Verbatim Window)

### 目标
确保模型能精确定位到最近的操作链，不遗漏 tool call/result 的精确内容。

### 设计

```
最近 K 条消息 → 全量保留，直接发送
K = min(config.verbatim_window_size, session.messages.len())
默认 K = 8（可根据模型上下文窗口调节）
```

### 关键规则

- Tool call 和其对应的 tool result **必须同时存在于窗口中**（二者要么都在，要么都被摘要化）
- 窗口内消息不做任何截断/摘要
- 用户显式 `/pin` 的消息强制留在窗口内
- 最后一次编辑的源代码文件路径标记为"热路径"，相关操作保留

### 实现位置

`crates/tui/src/core/engine/turn_loop.rs` — 修改 `messages_with_turn_metadata()`

```rust
fn messages_for_request(&self) -> Vec<Message> {
    let window_size = self.config.verbatim_window_size;
    let (window, history) = self.split_messages(window_size);
    let pins = self.collect_pins(&history);
    let retrieved = self.retrieve_relevant(&history, &pins);
    self.assemble_request(window, retrieved)
}
```

## Tier 2: 历史摘要 (Compaction + Embedding)

### 目标
窗口之外的历史消息不做全量保留，而是通过摘要+向量化按需注入。

### 设计

```
旧消息 → 阶段式摘要
         ↓
   分层摘要树（类似文档大纲）
         ↓
   向量化 → 写入向量库
         ↓
   每次请求前检索相关摘要 → 注入 system prompt
```

### 阶段式摘要策略

按**阶段**自动压缩，而不是等 token 堆到 50 万：

| 阶段 | 触发条件 | 操作 |
|------|---------|------|
| Phase 0 | 每 10 轮 | 对最早 10 轮做 LLM 摘要 |
| Phase 1 | 摘要累积超过预算 | 合并多个摘要为更高层摘要 |
| Phase 2 | 跨会话 | 提取可复用的偏好/约定→写入 Tier 3 |

### 检索逻辑

```rust
fn retrieve_history_context(&self, current_turn: &str, k: usize) -> Vec<ContextItem> {
    // 1. 当前用户消息 → embedding
    let query_vec = self.embed(current_turn);

    // 2. 在 LanceDB 历史摘要表中检索 top-k
    let table = self.lancedb.open_table("history_summaries").execute().await?;
    let results = table.search(&query_vec)
        .limit(k)
        .execute()
        .await?;

    // 3. 过滤：已存在于即时窗口中的不重复注入
    results.into_iter()
        .filter(|r| !self.verbatim_window.contains(&r.turn_index))
        .collect()
}
```

### 实现位置

`crates/tui/src/compaction.rs` — 改造现有 compaction 流程：

- 压缩时同时写入 LanceDB `history_summaries` 表，而不是只做 LLM 摘要
- 添加 `HistoryIndexService` 封装 LanceDB 读写

## Tier 3: 持久记忆 (Vector DB)

### 目标
替代当前扁平的 `memory.md`，实现跨会话的检索式记忆。

### 设计

LanceDB `memories` 表 schema：

```python
# LanceDB schema（Python 描述，Rust 端对应 Arrow/Schema 定义）
schema:
  - id:        string     # UUID，主键
  - content:   string     # 记忆正文（<= 256 tokens）
  - embedding: fixed_size_list(float32, 384)  # fastembed AllMiniLML6V2
  - source:    string     # "model" | "user" | "compaction"
  - session:   string     # 来源会话 ID
  - tags:      string     # 逗号分隔标签，如 "preference,rust"
  - created_at: timestamp
  - ttl:       timestamp  # 过期时间，NULL 表示永不过期

索引:
  - vector: IVF-PQ (索引加速)
  - scalar: tags, source (标量过滤)
```

### 记忆来源

| 来源 | 触发条件 | 示例 |
|------|---------|------|
| 模型主动写入 | `remember` tool | "用户喜欢 4 空格缩进" |
| 对话自动提取 | 跨 session 的重复模式 | "这个项目用 Rust 2024 edition" |
| 用户主动写入 | `# <note>` 快速添加 | "# 不要修改 .ssh 目录" |
| 压缩时提取 | 旧摘要中的持久性信息 | "架构决策：使用 axum 框架" |

### 相关度评分

```rust
struct MemoryItem {
    content: String,
    score: f64,         // 向量相似度 (余弦)
    recency: f64,       // 时间衰减因子
    importance: f64,    // 重要性权重（模型标注）
    source_trust: f64,  // 来源可信度
}

fn final_score(item: &MemoryItem) -> f64 {
    item.score * 0.4
        + item.recency * 0.2
        + item.importance * 0.3
        + item.source_trust * 0.1
}
```

### 上下文注入大小限制

**每条记忆**注入时限制为 `256 tokens`，**每次总计**不超过 `2048 tokens`（约当前 `<user_memory>` 100KB 的 1/50）。

### 实现位置

`crates/tui/src/memory.rs` — **完全重写**：

```rust
// 基于 LanceDB 的记忆系统
use lancedb::Table;
use fastembed::EmbeddingModel;

pub struct LanceMemorySystem {
    table: Table,          // LanceDB "memories" 表
    embedder: EmbeddingModel,
}

impl LanceMemorySystem {
    /// 存储一条记忆，自动计算 embedding
    pub async fn store(&self, item: NewMemoryItem) -> Result<()> {
        let embedding = self.embedder.embed(vec![&item.content])?;
        let record = MemoryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            content: item.content,
            embedding: embedding[0].clone(),
            source: item.source,
            session: item.session,
            tags: item.tags,
            created_at: Utc::now(),
            ttl: item.ttl,
        };
        self.table.add(vec![record]).execute().await?;
        Ok(())
    }

    /// 语义检索 top-k 条相关记忆
    pub async fn retrieve(&self, query: &str, k: u32) -> Result<Vec<MemoryRecord>> {
        let query_vec = self.embedder.embed(vec![query])?;
        let results = self.table
            .search(&query_vec[0])
            .limit(k)
            .execute()
            .await?;

        // 按相关度+时间混合排序
        Ok(self.rerank(results))
    }

    /// 清理过期的记忆
    pub async fn delete_expired(&self) -> Result<usize> {
        let now = Utc::now();
        let deleted = self.table
            .delete(format!("ttl IS NOT NULL AND ttl < '{now}'"))
            .execute().await?;
        Ok(deleted)
    }

    /// 组装 `<user_memory>` system prompt 块
    pub fn compose_block(&self, items: &[MemoryRecord]) -> Option<String> {
        if items.is_empty() { return None; }
        let body = items.iter()
            .map(|m| format!("- [{tag}] {content}",
                tag = m.tags.split(',').next().unwrap_or("general"),
                content = &m.content))
            .collect::<Vec<_>>()
            .join("\n");
        Some(format!("<user_memory>\n{body}\n</user_memory>"))
    }
}
```

## Tier 4: 代码知识 (Code Embedding)

### 目标
在模型操作文件时，自动检索代码库中的相关上下文。

### 设计

```
代码文件 → 分块 (chunk) → 1024 token 块，重叠 128 token
   ↓
fastembed → 384 维向量
   ↓
LanceDB `code_index` 表（按项目路径分区）
   ↓
模型操作文件时 → 检索相关 chunk → 注入 tool result
```

LanceDB `code_index` 表 schema：

```python
schema:
  - id:          string
  - file_path:   string     # 项目相对路径
  - chunk_index: int32      # 第几个块
  - content:     string     # 代码块原文
  - embedding:   fixed_size_list(float32, 384)
  - project:     string     # 项目根目录 hash，用于分区
  - updated_at:  timestamp

索引:
  - vector: IVF-PQ
  - scalar: file_path, project
```

### 实现时机

| 时机 | 操作 |
|------|------|
| 项目首次打开 | 全量索引（后台任务，非阻塞） |
| `read_file` 工具调用 | 先检索相关 chunk 附在 tool result 后 |
| `edit_file`/`write_file` | 增量更新索引 |
| 会话空闲时 | 增量索引变更的文件 |

### 实现位置

`crates/tui/src/tools/` — 在文件操作工具中嵌入检索调用

## 向量存储选型：LanceDB

### 选型理由

在 Rust CLI 工具的约束下，LanceDB 是唯一满足所有条件的方案：

| 需求 | LanceDB | 其他方案 |
|------|---------|---------|
| 纯 Rust 实现，零外部依赖 | 是（Rust 原生） | sqlite-vec 需 FFI，pgvector 需 PG 服务 |
| 嵌入式，无独立进程 | 是 | qdrant/milvus 需独立服务 |
| 支持标量过滤 | 是（按标签、时间、来源过滤） | instant-hnsw/voyager 不支持 |
| 增量持久化 | 是（文件即数据库） | instant-hnsw 需全量序列化 |
| 成熟度 | 生产级，LanceDB 团队维护 | sqlite-vec 尚不成熟 |

### 安装

```toml
# Cargo.toml
lancedb = "0.17"
fastembed = "4.0"
```

两个依赖，零系统库要求。

### 数据库结构与表设计

所有数据存储在 `~/.deepseek/vector_db/` 目录，LanceDB 以目录即数据库方式管理：

```
~/.deepseek/vector_db/
├── history_summaries.lance/   # Tier 2: 历史摘要
├── memories.lance/            # Tier 3: 持久记忆
└── code_index.lance/          # Tier 4: 代码知识
```

#### history_summaries 表（Tier 2）

```python
schema:
  - id:          string        # "sum_{uuid}"
  - turn_range:  string        # "1-10", "11-20" ...
  - summary:     string        # LLM 摘要文本
  - embedding:   fixed_size_list(float32, 384)
  - key_files:   string        # 涉及的关键文件路径
  - session_id:  string
  - created_at:  timestamp

索引:
  - vector: IVF-PQ
  - scalar: session_id
```

#### memories 表（Tier 3）

```python
schema:
  - id:          string        # UUID
  - content:     string        # 记忆正文（<= 256 tokens）
  - embedding:   fixed_size_list(float32, 384)
  - source:      string        # "model" | "user" | "compaction"
  - session_id:  string        # 来源会话
  - tags:        string        # 逗号分隔，"preference,rust,project-convention"
  - importance:  float32       # 模型标注的重要性 [0-1]
  - created_at:  timestamp
  - ttl:         timestamp     # NULL 永不过期

索引:
  - vector: IVF-PQ
  - scalar: tags, source, session_id
```

#### code_index 表（Tier 4）

```python
schema:
  - id:          string
  - file_path:   string        # 项目相对路径
  - chunk_index: int32         # 第几个块
  - content:     string        # 代码块原文
  - embedding:   fixed_size_list(float32, 384)
  - project:     string        # 项目根路径 hash
  - updated_at:  timestamp

索引:
  - vector: IVF-PQ
  - scalar: file_path, project
```

### Embedding 方案

```rust
use fastembed::EmbeddingModel;

let model = EmbeddingModel::AllMiniLML6V2;  // 384 维，速度优先
// 或
let model = EmbeddingModel::BGEBaseENV15;    // 768 维，精度优先
```

`fastembed-rs` 使用 ONNX Runtime 本地运行模型，无需 GPU，无 API 调用。单次 embedding < 10ms。模型文件首次自动下载至 `~/.cache/fastembed/`，后续离线可用。

所有四个 Tier 共用同一个 embedding 模型实例。384 维向量在 LanceDB IVF-PQ 索引下，每条记录存储约 2KB，10 万条仅 ~200MB 磁盘空间。

### 核心读写模式

```rust
use lancedb::connect;

/// 初始化 LanceDB 数据库
pub async fn init_vector_db(path: &Path) -> Result<LanceDb> {
    let db = connect(path.to_str().unwrap()).execute().await?;

    // 表不存在时自动创建
    for (name, schema) in TABLES {
        if !db.table_names().execute().await?.contains(&name) {
            db.create_table(name, schema).execute().await?;
        }
    }
    Ok(db)
}

/// 检索记忆（Tier 3 核心路径）
pub async fn retrieve_memories(db: &LanceDb, query: &str, k: u32) -> Result<Vec<MemoryRecord>> {
    let query_vec = embed(query)?;
    let table = db.open_table("memories").execute().await?;

    let results = table
        .search(&query_vec)
        .limit(k)
        .execute()
        .await?;

    // 反序列化并过滤过期
    Ok(results.into_iter()
        .filter(|r| r.ttl.is_none() || r.ttl > Utc::now())
        .collect())
}
```

### 为什么不选其他方案

| 方案 | 排除原因 |
|------|---------|
| `pgvector` | 需要 PostgreSQL 服务，CLI 工具场景太重 |
| `qdrant`/`milvus`/`weaviate` | 需要独立服务进程 |
| `sqlite-vec` | Rust binding 不成熟，大规模性能退化 |
| `instant-hnsw`/`voyager` | 不支持标量过滤和增量 Schema 演进，后续扩展受限 |
| 纯关键词/TF-IDF | 无语义理解，无法满足跨会话记忆检索需求 |

## 请求组装新流程

```rust
async fn assemble_request(
    messages: &[Message],
    db: &LanceDb,
    config: &Config,
) -> MessageRequest {
    // 1. 切分即时窗口和历史
    let (window, history) = split_at_verbatim_window(messages, config.verbatim_window_size);

    // 2. 从 current_turn 提取检索 query
    let query = extract_current_query(&window);

    // 3. LanceDB 检索历史摘要 (Tier 2)
    let mut retrieved_blocks: Vec<String> = Vec::new();
    if config.enable_history_retrieval {
        let table = db.open_table("history_summaries").execute().await?;
        let query_vec = embed(&query)?;
        let summaries = table.search(&query_vec)
            .limit(config.max_history_summaries)
            .execute().await?;
        for s in &summaries {
            retrieved_blocks.push(format!("[history] {}", s.summary));
        }
    }

    // 4. LanceDB 检索持久记忆 (Tier 3)
    if config.memory.enabled {
        let memory_table = db.open_table("memories").execute().await?;
        let query_vec = embed(&query)?;
        let memories = memory_table.search(&query_vec)
            .limit(config.memory.max_items as u32)
            .execute().await?;
        for m in &memories {
            retrieved_blocks.push(format!("- [{tag}] {content}",
                tag = m.tags.split(',').next().unwrap_or("general"),
                content = &m.content));
        }
    }

    // 5. 组装 system prompt
    let context_block = if !retrieved_blocks.is_empty() {
        format!(
            "<retrieved_context>\n{}\n</retrieved_context>",
            retrieved_blocks.join("\n")
        )
    } else {
        String::new()
    };

    MessageRequest {
        messages: window,              // 只发窗口内的消息
        system: Some(SystemPrompt::build(
            &config.base_prompt,
            &context_block,
        )),
        ...
    }
}
```

## Token 节省估算

| 场景 | 当前消耗/轮 | 改进后消耗/轮 | 节省比例 |
|------|------------|--------------|---------|
| 10 轮对话 | 50K | 20K | 60% |
| 50 轮对话 | 250K | 30K | 88% |
| 100 轮对话 | 500K | 35K | 93% |
| 200 轮对话 | 1M+ | 40K | 96% |

## 实施路线

### Phase 1：LanceDB 集成 + 即时窗口（1-2 周）
- [ ] 添加 `lancedb` + `fastembed-rs` 依赖
- [ ] 实现 `VectorDbService` 初始化（启动时连接 `~/.deepseek/vector_db/`）
- [ ] 创建 `history_summaries`、`memories`、`code_index` 三张表
- [ ] 实现 `MessagesSplitter`（窗口大小配置、tool call/result 配对保护、pin 机制）
- [ ] 修改 `messages_with_turn_metadata()` 按窗口截断
- [ ] 添加 `verbatim_window_size` 配置项

### Phase 2：历史摘要向量化（1 周）
- [ ] 改造现有 compaction 流程：压缩结果写入 LanceDB `history_summaries` 表
- [ ] 请求前从 `history_summaries` 检索相关摘要注入 system prompt
- [ ] 降低自动压缩阈值（50K → 30K tokens）
- [ ] 添加阶段式摘要（每 10 轮触发一次 mini-compaction）

### Phase 3：持久记忆系统（1 周）
- [ ] 重写 `memory.rs`：`LanceMemorySystem` 替换扁平文件
- [ ] `remember` tool 写入 LanceDB `memories` 表
- [ ] 每次请求前检索 top-5 相关记忆注入 `<user_memory>` block
- [ ] 实现 TTL 过期自动清理

### Phase 4：代码知识索引（2 周）
- [ ] 项目文件分块 + fastembed → LanceDB `code_index` 表（后台任务）
- [ ] 增量更新：文件修改时自动更新对应 chunk
- [ ] `read_file`/`edit_file` 工具调用时检索相关 chunk 附在结果中

## 配置项

```toml
[context]
# 即时窗口大小（最近 N 条消息全量保留）
verbatim_window_size = 8
# 启用历史摘要检索（LanceDB history_summaries 表）
enable_history_retrieval = true
# 每轮注入的历史摘要数量
max_history_summaries = 3

[memory]
# 启用 LanceDB 检索式记忆（替代扁平 memory.md）
enabled = true
# 每轮注入的记忆条目数
max_items = 5
# 每条最大长度（tokens）
max_item_tokens = 256
# 记忆过期天数（0 表示永不过期）
default_ttl_days = 90

[code_index]
# 启用代码知识索引（LanceDB code_index 表）
enabled = false
# 索引文件 glob 模式
include_patterns = ["**/*.rs", "**/*.toml", "**/*.md"]
# 排除模式
exclude_patterns = ["**/target/**", "**/node_modules/**"]

[vector_db]
# LanceDB 数据库路径（默认 ~/.deepseek/vector_db/）
path = "~/.deepseek/vector_db"
# embedding 模型
embedding_model = "all-MiniLM-L6-v2"
```

## 测试策略

### 分层测试架构

```
┌─────────────────────────────────────────┐
│  Tier 1: 单元测试 (Unit Tests)            │
│  即时窗口逻辑、工具配对、消息切分          │
│  纯内存，无外部依赖，毫秒级               │
├─────────────────────────────────────────┤
│  Tier 2: 集成测试 (Integration Tests)     │
│  LanceDB 读写、记忆 CRUD、检索排序        │
│  需要临时目录 + LanceDB 实例              │
│  秒级                                     │
├─────────────────────────────────────────┤
│  Tier 3: 端到端测试 (E2E Tests)           │
│  模拟引擎 + 向量 DB → 验证请求组装        │
│  需要加载 embedding 模型                   │
│  分钟级（CI 可选）                        │
└─────────────────────────────────────────┘
```

### Tier 1: 单元测试（即时窗口逻辑）

这些测试纯逻辑，不依赖任何外部资源，放在模块内 `#[cfg(test)]` 中。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // ── 窗口切分 ──

    #[test]
    fn verbatim_window_keeps_last_n_messages() {
        let messages = create_messages(20);
        let window = split_verbatim_window(&messages, 8);
        assert_eq!(window.len(), 8);
        assert_eq!(window[0].role, messages[12].role); // index 12-19
    }

    #[test]
    fn verbatim_window_respects_message_count_less_than_window() {
        let messages = create_messages(3);
        let window = split_verbatim_window(&messages, 8);
        assert_eq!(window.len(), 3); // 全部保留
    }

    // ── Tool call/result 配对保护 ──

    #[test]
    fn window_preserves_tool_call_pairs() {
        // messages[15] = tool_use(id="t1")
        // messages[16] = tool_result(tool_use_id="t1")
        // window=4, 保留 index 16-19
        // 但 index 16 是 tool_result, 对应的 tool_use 在 index=15
        // → 窗口应扩展至 index 15-19
        let mut messages = create_messages(20);
        messages[15] = tool_use_msg("t1", "read_file");
        messages[16] = tool_result_msg("t1", "ok");

        let window = split_verbatim_window(&messages, 4);
        assert!(window.iter().any(|m| has_tool_use_id(m, "t1")));
        assert!(window.iter().any(|m| has_tool_result_id(m, "t1")));
        // 验证扩展后的窗口 = 5 条（15-19），不是 4 条
        assert_eq!(window.len(), 5);
    }

    #[test]
    fn pin_override_keeps_specific_messages() {
        let messages = create_messages(20);
        let pins = vec![0]; // 强制保留第一条
        let window = split_verbatim_window_with_pins(&messages, 8, &pins);
        assert_eq!(window.len(), 9); // 8 + 1 pin
        assert_eq!(window[0].content, messages[0].content);
    }

    // ── 消息截断 ──

    #[test]
    fn messages_below_window_are_not_sent() {
        // 验证窗口之外的消息不会出现在请求中
        let messages = create_messages(20);
        let window = split_verbatim_window(&messages, 5);
        for msg in &window {
            assert!(msg.index >= 15); // 只保留 15-19
        }
    }

    // ── 热路径标记 ──

    #[test]
    fn hot_path_files_expand_window() {
        let messages = create_messages(20);
        messages[18] = text_msg("assistant", "edit src/core/engine.rs");
        let hot_paths = vec!["src/core/engine.rs"];
        let window = split_verbatim_window_with_hot_paths(&messages, 4, &hot_paths);
        // 包含 src/core/engine.rs 的消息虽在窗口外，但因热路径被保留
        assert!(window.iter().any(|m| m.content.contains("engine.rs")));
    }
}
```

### Tier 2: 集成测试（LanceDB 读写）

使用 `tempfile` 创建临时目录，测试真实的 LanceDB 实例。这些测试不依赖 embedding 模型——用随机向量替代。

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    use tempfile::TempDir;

    /// 辅助函数：用随机向量替代真实 embedding
    fn random_vec(dim: usize) -> Vec<f32> {
        (0..dim).map(|_| rand::random::<f32>()).collect()
    }

    // ── 基础 CRUD ──

    #[tokio::test]
    async fn lancedb_store_and_retrieve_memory() {
        let tmp = TempDir::new().unwrap();
        let db = lancedb::connect(tmp.path().to_str().unwrap())
            .execute().await.unwrap();

        let service = LanceMemoryService::new(db, 384);

        // 写入
        service.store(NewMemoryItem {
            content: "用户喜欢 4 空格缩进".into(),
            tags: "preference".into(),
            source: "model".into(),
            embedding: random_vec(384),
        }).await.unwrap();

        // 检索
        let results = service.retrieve(&random_vec(384), 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("4 空格缩进"));
    }

    // ── 标量过滤 ──

    #[tokio::test]
    async fn filter_by_tags() {
        let tmp = TempDir::new().unwrap();
        let db = lancedb::connect(tmp.path().to_str().unwrap())
            .execute().await.unwrap();

        let service = LanceMemoryService::new(db, 384);
        service.store_preference("preference,rust").await;
        service.store_preference("convention,python").await;

        // 只检索 tag 包含 rust 的
        let results = service.retrieve_by_tag("rust", 10).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    // ── TTL 过期 ──

    #[tokio::test]
    async fn expired_memories_are_not_returned() {
        let tmp = TempDir::new().unwrap();
        let db = lancedb::connect(tmp.path().to_str().unwrap())
            .execute().await.unwrap();

        let service = LanceMemoryService::new(db, 384);
        service.store_with_ttl("临时记忆", chrono::Duration::seconds(-1)).await;

        let results = service.retrieve(&random_vec(384), 5).await.unwrap();
        assert_eq!(results.len(), 0); // 已过期，不返回
    }

    // ── 重复写入幂等 ──

    #[tokio::test]
    async fn duplicate_memory_dedup() {
        let tmp = TempDir::new().unwrap();
        let db = lancedb::connect(tmp.path().to_str().unwrap())
            .execute().await.unwrap();

        let service = LanceMemoryService::new(db, 384);
        // 相同内容写两次
        service.store(/* 内容 A */).await.unwrap();
        service.store(/* 内容 A */).await.unwrap();

        let results = service.retrieve(&random_vec(384), 10).await.unwrap();
        assert_eq!(results.len(), 1); // 去重后只有一条
    }

    // ── 并发读写 ──

    #[tokio::test]
    async fn concurrent_read_write() {
        let tmp = TempDir::new().unwrap();
        let db = lancedb::connect(tmp.path().to_str().unwrap())
            .execute().await.unwrap();

        let service = Arc::new(LanceMemoryService::new(db, 384));
        let mut handles = vec![];

        for i in 0..10 {
            let svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.store(/* ... */).await.unwrap();
            }));
        }

        futures::future::join_all(handles).await;
        let count = service.count().await.unwrap();
        assert_eq!(count, 10); // 无数据竞争
    }

    // ── 清理过期 ──

    #[tokio::test]
    async fn delete_expired_removes_only_expired() {
        let tmp = TempDir::new().unwrap();
        let db = lancedb::connect(tmp.path().to_str().unwrap())
            .execute().await.unwrap();

        let service = LanceMemoryService::new(db, 384);
        service.store_valid().await;
        service.store_expired().await;

        let deleted = service.delete_expired().await.unwrap();
        assert_eq!(deleted, 1);

        let remaining = service.count().await.unwrap();
        assert_eq!(remaining, 1);
    }
}
```

### Tier 3: 端到端测试（嵌入真实 embedding）

这些测试加载 `fastembed` 模型，验证语义检索效果。运行较慢（模型加载约 2-5s），适合作为 CI 的单独 job 或 nightly 测试。

```rust
#[cfg(test)]
mod e2e_tests {
    use super::*;
    use fastembed::EmbeddingModel;

    /// 仅在 CI 且启用 `test_e2e` feature 时运行
    #[cfg_attr(feature = "test_e2e", tokio::test)]
    async fn semantic_retrieval_ranks_relevant_higher() {
        let tmp = TempDir::new().unwrap();
        let db = lancedb::connect(tmp.path().to_str().unwrap())
            .execute().await.unwrap();
        let model = EmbeddingModel::AllMiniLML6V2;
        let embed = |text: &str| model.embed(vec![text]).unwrap().remove(0);

        let mut service = LanceMemoryService::new(db, model);
        service.store("用户用 cargo test --workspace 运行测试", embed).await;
        service.store("用户喜欢 VS Code 编辑器", embed).await;

        // 查询测试相关的内容
        let results = service.retrieve("how to run tests", 2).await.unwrap();
        assert_eq!(results[0].content, "用户用 cargo test --workspace 运行测试");
        // ↑ 语义上最相关的排在第一位
    }

    #[cfg_attr(feature = "test_e2e", tokio::test)]
    async fn cross_lingual_retrieval() {
        // 中文 query 能检索到英文记忆（MiniLM 跨语言）
        let results = service.retrieve("测试方法", 5).await.unwrap();
        assert!(results.iter().any(|r| r.content.contains("test")));
    }

    #[cfg_attr(feature = "test_e2e", tokio::test)]
    async fn full_pipeline_assemble_request() {
        // 模拟完整链路：消息切分 → 嵌入 → LanceDB 检索 → 请求组装
        let engine = TestEngineBuilder::new()
            .with_vector_db(tmp.path())
            .with_memories(vec![/* 预设记忆 */])
            .with_messages(vec![/* 20 条历史消息 */])
            .build();

        let request = engine.assemble_request().await.unwrap();

        // 验证：只有窗口内消息 + 检索到的记忆
        assert!(request.messages.len() <= 10); // 窗口 8 + 可能的扩展
        assert!(request.system.contains("<user_memory>"));
    }
}
```

### CI 配置

```yaml
# .github/workflows/test.yml

test:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: actions-rust-lang/setup-rust-toolchain@v1

    # 单元测试（快速）
    - run: cargo test --lib

    # 集成测试（LanceDB 临时实例）
    - run: cargo test --test integration

    # E2E 测试（加载 embedding 模型）
    # 单独 job，避免拖慢主流程
    - run: cargo test --features test_e2e --test e2e

# 按模块分组运行
unit:
  - run: cargo test -p deepseek-tui -- compaction::tests
  - run: cargo test -p deepseek-tui -- memory::tests
  - run: cargo test -p deepseek-tui -- core::engine::tests::verbatim_window
```

### 测试覆盖率目标

| 模块 | 目标覆盖率 | 关键测什么 |
|------|-----------|-----------|
| 即时窗口 `MessagesSplitter` | 100% | 切分边界、配对保护、pin 扩展 |
| LanceDB CRUD `LanceMemoryService` | 90% | 读写、过滤、TTL、并发 |
| 记忆检索 `retrieve` | 90% | 相关度排序、去重、过期过滤 |
| 请求组装 `assemble_request` | 80% | 合并上下文、system prompt 格式 |
| compaction + 向量化 | 70% | 压缩流程中写入 LanceDB |
| 代码索引 `code_index` | 70% | 分块、增量更新 |

## 风险与缓释

| 风险 | 影响 | 缓释措施 |
|------|------|---------|
| 检索不准确导致模型漏掉关键上下文 | 回答质量下降 | 向量检索 + 最近 N 条摘要兜底双路保障 |
| LanceDB 表损坏 | 数据丢失 | LanceDB 列存有校验，支持 `optimize.compact()` 修复；定期从 session JSON 恢复索引 |
| 首次 embedding 模型下载慢 | 启动延迟 | 后台下载，不阻塞启动；下载进度在 TUI 状态栏显示 |
| 本地 embedding 精度不够 | 检索相关度低 | 支持 BYO embedding 配置项，可接入外部 API |
| 跨会话记忆污染 | 误导模型 | TTL 过期 + 相关性阈值（cosine < 0.5 不注入） |
| LanceDB Rust API 版本升级 break | 构建失败 | 在 `Cargo.lock` 锁定版本；CI 中跑集成测试 |
