# Vector Memory System — 逆向审视问题清单

> 生成于 2026-05-09。对 `feat/vector-memory-system` 分支进行的完整逆向审计。
> 每个问题标注严重程度（🔴 严重 / 🟡 中等 / 🟢 低优先级）、影响范围和修复思路。

---

## 🔴 严重问题（影响生产可用性）

### 1. Embedder 阻塞 async runtime

**位置**: `crates/tui/src/vector_db.rs` — `Embedder::embed()`

**问题**: 
- 使用 `std::sync::Mutex` + 同步 `TextEmbedding::embed()` 调用
- 首次加载模型（fastembed 下载 `all-MiniLM-L6-v2`）会阻塞调用线程 2-10 秒
- ONNX Runtime 初始化也是同步的 CPU 密集操作
- Mutex 持有期间阻塞所有并发嵌入请求

**影响**: 首次使用向量检索时（或首次 `remember` 调用时），整个 async runtime 被同步阻塞。在 TUI 中表现为界面卡死 5-10 秒。

**修复思路**:
- 在 `init_vector_db()` 中调用 `warmup_embedder()` 预加载模型
- 考虑用 `tokio::task::spawn_blocking()` 包裹 `embed()` 调用
- 或者在 `LanceDbBackend::connect()` 时同步初始化（启动时完成，用户可接受）

---

### 2. Compaction 摘要原样存入向量库，样板文字污染 embedding 空间

**位置**: `crates/tui/src/core/engine.rs` — `store_compaction_summary_to_vector_db()`

**问题**:
- 存入的文本是完整的 SystemPrompt，包含大量样板：
  ```
  ## 📋 Conversation Summary (Auto-Generated)
  {summary}
  ---
  ## 🔍 Workflow Context
  {workflow_context}
  ---
  ## 💡 What to Do Next
  You have just resumed from a context compaction...
  ```
- 每个摘要都有完全相同的标题和指令文本
- 嵌入时会把这些样板也编码进 384 维向量，降低语义检索精度

**影响**: 搜索历史摘要时，会匹配到大量包含相同样板文字但不相关的结果。

**修复思路**:
- 只提取 `{summary}` 部分（即 LLM 实际生成的对话摘要）
- 或者用正则提取 `## 📋 Conversation Summary` 和 `---` 之间的内容
- 把 workflow_context 和 key_files 作为独立字段（如 tags）存储，不参与 embedding

---

### 3. 空表创建没有向量索引 → 全表暴力扫描

**位置**: `crates/tui/src/vector_db.rs` — `LanceDbBackend::create_empty_table()`

**问题**:
- 代码注释说 "Index is created automatically by LanceDB on first data addition"
- 但 LanceDB 只在 `table.add()` 且配置了 `Index::Auto` 时才创建索引
- 建空表用 `RecordBatch::new_empty(schema)` → `create_table`，不会触发自动索引
- 没有 IVF-PQ 索引 = 每次检索都是 **O(n) 全表暴力搜索**
- 在 1000+ 条记录时性能差异可达 50-100x

**影响**: 向量检索退化为全表扫描。初期数据少时不可见，积累后性能急剧下降。

**修复思路**:
- 方案 A: 在 `ensure_tables` 中检查已存在表是否有索引，没有则创建
- 方案 B: 在首次 `store_memory` / `store_summary` 后调用 `create_index(Index::Auto)`
- 方案 C: 去掉空表创建，改为在首次写入时创建附带数据的表

---

### 4. `vector_memory_enabled=false` 时 compaction 阈值下调的副作用

**位置**: `crates/tui/src/compaction.rs` — `MINIMUM_AUTO_COMPACTION_TOKENS` + `token_threshold`

**问题**:
- 我们将 `auto_floor_tokens` 从 500K 降到 50K
- 将 `token_threshold` 从 800K 降到 200K
- 但这两个阈值对 **所有用户** 生效，无论 `vector_memory_enabled` 是否为 true
- 对于未启用向量存储的用户，更频繁的 compaction 只是破坏 V4 prefix cache（~90% 折扣），没有向量存储的收益
- 每次 compaction 的成本（LLM 调用 + cache 失效）可能超过 token 节省

**影响**: 未启用向量存储的用户会遭受不必要的 compaction 开销。这是一个所有用户都会遇到的 regression。

**修复思路**:
- 方案 A: 当 `vector_memory_enabled=false` 时，保持旧阈值
- 方案 B: 在 `CompactionConfig` 中添加 `vector_memory_aware` 标志，由 engine 根据 `self.vector_db.is_some()` 动态选择阈值
- 方案 C: 将阈值分成两档：`token_threshold_with_vector` 和 `token_threshold_without_vector`

---

## 🟡 中等问题（影响数据质量或一致性）

### 5. 记忆重复写入无去重

**位置**: `crates/tui/src/tools/remember.rs` — `RememberTool::execute()`

**问题**:
- `remember` tool 每次调用都创建新的 MemoryRecord
- 即使内容与已有记忆完全相同（如 "用户喜欢 4 空格缩进"）
- 没有基于内容相似度的去重机制
- 模型可能多次调用 `remember` 记录同一件事

**影响**: 向量库中积累大量重复记忆。检索时可能返回多个相同内容的条目，浪费 system prompt token。

**修复思路**:
- 写入前先 `search_memories(note, 1)` 检查 cosine 相似度
- 如果 `score > 0.9`，跳过写入
- 或者更新已有记录的时间戳（刷新 TTL）

---

### 6. InMemoryBackend 和 LanceDB 数据不一致风险

**位置**: `crates/tui/src/vector_db.rs` — `VectorDbService::store_memory()`

**问题**:
- 当前逻辑：LanceDB 写入成功 → InMemoryBackend 写入 → 返回 LanceDB 的结果
- 如果 InMemoryBackend 写入失败（磁盘满、JSON 序列化失败），两者状态不一致
- InMemoryBackend 被设计为"快速读缓存"，但它无限增长为 LanceDB 的完整副本
- 没有缓存淘汰策略（LRU/TTL）

**影响**: 长期运行时 InMemoryBackend 可能 OOM（内存无限增长），且与 LanceDB 不同步时返回过期数据。

**修复思路**:
- 启用 LanceDB 时，InMemoryBackend 不再镜像全量数据
- 或者将 InMemoryBackend 改为有界的 LRU 缓存（如最多 1000 条）
- 读取路径：先查 InMemoryBackend，miss 时查 LanceDB 并回填缓存

---

### 7. 搜索无相似度阈值，低分噪音注入 prompt

**位置**: `crates/tui/src/core/engine.rs` — `build_verbatim_window_for_request()`

**问题**:
- `search_memories` 返回 top-k 结果，无条件全部注入 system prompt
- LanceDB 返回的 `_distance` 转换为 `score = 1 - distance`
- 距离 > 0.5 的结果语义基本不相关（cosine < 0.5），但仍被注入
- 每次请求额外消耗 ~200-500 tokens 的无用上下文

**影响**: System prompt 被低质量检索结果污染，反而可能误导模型。

**修复思路**:
- 在 `build_verbatim_window_for_request` 中过滤 `score < 0.4` 的结果
- 或在 `VectorDbService::search_memories` 中添加 `min_score` 参数
- LanceDB 的 `nearest_to` 可以设置 `distance_type: Cosine`，直接用 cosine 距离

---

### 8. `code_index` 表永远为空

**位置**: `crates/tui/src/vector_db.rs` — `VectorDbService::store_code_chunk()` + `search_code()`

**问题**:
- 表在 `ensure_tables` 中创建成功
- `store_code_chunk` 和 `search_code` API 已实现
- 但没有任何代码路径调用 `store_code_chunk`
- `read_file` 中的 `search_code` 永远返回空结果
- 代码索引功能（Tier 4）实际上是 **没有生效的**

**影响**: `<related_code>` 块永远不会出现在 read_file 的输出中。用户看不到代码上下文优化。

**修复思路**:
- 后续需要在 `write_file` / `edit_file` 成功后触发 `store_code_chunk`
- 或者在引擎启动时做后台全量索引
- 短期可以用 `/index` 命令手动触发

---

## 🟢 低优先级（优化和边界条件）

### 9. 语义检索 query 构造不够充分

**位置**: `crates/tui/src/core/engine.rs` — `build_verbatim_window_for_request()` 中的 query 构建

**问题**:
- 检索 query 只取最后 3 条 **user 消息**的 text block
- 排除了 tool results、thinking block、assistant 消息
- 当模型刚执行完一系列工具调用，最近的 user 消息可能只有 "继续"、"好的"、"请"
- 这种短 query 很难产生有意义的向量检索结果

**影响**: 某些轮次中，语义检索可能退化为随机匹配。尤其对中文短指令。

**修复思路**:
- 在 query 构造中包含最近 1-2 个 tool result 的摘要（截取前 100 chars）
- 或者使用当前轮次的所有 text block 拼接
- 对短 query 添加用户之前的消息作为上下文

---

### 10. TTL 删除的 SQL 跨版本兼容性

**位置**: `crates/tui/src/vector_db.rs` — `LanceDbBackend::delete_expired_memories()`

**问题**:
```rust
table.delete(&format!("ttl IS NOT NULL AND CAST(ttl AS INT64) < {nanos}"))
```
- `CAST(ttl AS INT64)` 是 LanceDB 特定的 SQL 语法
- LanceDB v0.27 SQL 支持有限，某些存储后端可能不支持此语法
- 版本升级时可能 break
- 没有单元测试覆盖 LanceDB 的 delete 路径

**影响**: `delete_expired_memories` 在特定版本/后端可能静默失败或报错。

**修复思路**:
- 方案 A: 先查询所有记录，在内存中过滤 TTL，再用 id 列表删除
- 方案 B: 使用 LanceDB Rust API 的 `only_if` 进行过滤（如果支持 timestamp 比较）
- 方案 C: 定期用 `compact_files` + `cleanup_old_versions` 做物理清理

---

### 11. VerbatimWindow 消息数计算不准

**位置**: `crates/tui/src/core/engine.rs` — `build_verbatim_window_for_request()`

**问题**:
```rust
let vw = VerbatimWindow::build(total, window_turns * 2, ...)
```
- `window_turns` = 16 时，窗口大小 = 32 条消息
- 假设每轮 2 条（user + assistant），但一个工具调用密集的轮次可能有 5-10 条消息
- 实际发送到 API 的消息数可能是预期的 2-3 倍
- 反过来，全是短对话时，32 条消息可能对应 16+ 轮

**影响**: verbatim window 的实际大小不可预测。可能超出 token 预算。

**修复思路**:
- 用消息数代替轮数设置窗口
- 或者在 build 时传入实际的 token 预算作为上限，动态调整窗口大小

---

### 12. Sub-agent 不继承父 agent 的已检索上下文

**位置**: `crates/tui/src/core/engine.rs` — `handle_send_message()` 中的 `fork_context_for_runtime`

**问题**:
- 子代理通过 `SubAgentForkContext` 获得 system prompt + 父代理的消息
- 父代理在 `prepare_request_context` 中检索的记忆/摘要只注入到当前请求的 system prompt
- 子代理有自己的 `ToolContext.vector_db`，可以独立检索
- 但子代理的检索结果可能与父代理不同（query 不同，或根本没有有用的 query）

**影响**: 子代理缺少父代理已经获取到的上下文信息，可能需要重新检索或根本不知道相关信息。

**修复思路**:
- 在 `SubAgentForkContext` 中包含父代理已检索的 `<retrieved_context>` 块
- 或者在子代理的 system prompt 中追加父代理的检索结果

---

## 附录：其他观察

### 未使用的代码
- `warmup_embedder()` — 定义了但从未被调用
- `Embedder::initialize()` — 同上
- `Embedder::dim()` — 同上
- `VerbatimWindow::iter()` / `is_empty()` / `contains()` — 同上
- `RetrievedContext::is_empty()` — 同上
- `LanceDbBackend::embedder()` / `create_index()` — 同上

### 配置缺失
- 无 `max_memory_items` / `max_summary_items` 配置项（上限硬编码为 3/2）
- 无 `min_similarity_score` 阈值配置
- 无 `code_index_enabled` 开关
- 无 `compaction_threshold_with_vector` vs `without_vector` 的区别配置

### 测试覆盖
- LanceDB 集成测试为 0（全部用 `dim=0` 跳过）
- 没有 compaction + vector DB 的端到端测试
- 没有 Embedder 预热失败的回退测试
- 没有 TTL 删除路径的测试
