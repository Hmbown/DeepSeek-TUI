<div align="center">
  <a href="https://signature4u.vercel.app/schemaui?font=satisfy&fontSize=153&speed=2.8&charSpacing=0&borderRadius=0&cardPadding=24&fill=multi&fill1=001bb7&fill2=ec4899&stroke=001bb7&stroke2=ec4899&strokeMode=multi&strokeEnabled=1&bg=transparent&bgMode=solid&bg2=1e3a8a&texture=cross&texColor=566486&texSize=30&texThickness=1&texOpacity=0.4&colors=001bb7-001bb7-001bb7-001bb7-001bb7-001bb7-ff8040-fcb53b&linkFillStroke=1" target="_blank">
    <img src="https://signature4u.vercel.app/api/sign?text=schemaui&font=satisfy&fontSize=153&speed=2.8&charSpacing=0&borderRadius=0&cardPadding=24&fill=multi&fill1=001bb7&fill2=ec4899&stroke=001bb7&stroke2=ec4899&strokeMode=multi&strokeEnabled=1&bg=transparent&bgMode=solid&bg2=1e3a8a&texture=cross&texColor=566486&texSize=30&texThickness=1&texOpacity=0.4&colors=001bb7-001bb7-001bb7-001bb7-001bb7-001bb7-ff8040-fcb53b&linkFillStroke=1" align="center"  alt="schemaui signature"/>
  </a>
</div>

[![Crates.io](https://img.shields.io/crates/v/schemaui.svg)](https://crates.io/crates/schemaui)
[![Documentation](https://docs.rs/schemaui/badge.svg)](https://docs.rs/schemaui)
[![License](https://img.shields.io/crates/l/schemaui)](https://github.com/yuniqueunic/schemaui#license)
![Crates.io Total Downloads](https://img.shields.io/crates/d/schemaui)

<!-- ![Deps.rs Crate Dependencies (latest)](https://img.shields.io/deps-rs/schemaui/latest) -->

<div align="center">
  <a href="https://asciinema.org/a/7IBbhRJAUBlIQaPWSrspEgZtE" target="_blank">
    <img src="https://asciinema.org/a/7IBbhRJAUBlIQaPWSrspEgZtE.svg" width="500" />
  </a>

[English](./README.md) | [中文文档](./README.ZH.md)

</div>

`schemaui` 将 JSON Schema
文档转换为由`ratatui`、`crossterm`和`jsonschema`驱动的完全交互式的终端用户界面。

该库解析丰富的模式（嵌套部分、`$ref`、数组、键值映射、模式属性等），将其转换为可导航的表单树，将其呈现为键盘优先的编辑器，并在每次编辑后验证结果，以便用户在保存之前始终可以看到完整的错误列表。

<!-- AUTO-GENERATED:CLI-QUICKLINK:BEGIN -->

> CLI 可用：`schemaui-cli` 会安装 `schemaui` 可执行文件。想直接使用 CLI？跳转到
> [CLI 安装与用法](#cli-schemaui-cli)。

<!-- AUTO-GENERATED:CLI-QUICKLINK:END -->

## 功能亮点

- **模式保真度** –
  `draft-07`，包括`$ref`、`definitions`、、`patternProperties`、枚举、数值范围以及嵌套的对象/数组。
- **部分和覆盖层** –
  顶层属性成为根标签，嵌套对象被展平为部分，复杂节点（复合体、键值集合、数组条目）打开具有自身验证器的专用覆盖层。
- **即时验证** –
  每次按键都可以触发`jsonschema::Validator`，所有错误（字段作用域 +
  全局）都被收集并一起显示。
- **可插拔的 I/O** – `io::input`可以处理
  JSON/YAML/TOML（通过功能标志），而`io::output`可以输出到标准输出和/或任何启用格式的多个文件。
- **内置 CLI** –
  `schemaui-cli`提供了与库相同的流程，包括多目标输出、stdin/内联规范和聚合诊断。

## Config Schema 自动检测

当你只传 `--config` 而不显式传 `--schema` 时，`schemaui-cli`
现在会按以下优先级解析 schema：

1. 显式 `--schema`
2. 配置文件内声明的 schema
3. `schema_from_data_value` 自动推断

支持的声明形式：

- **JSON**：根级 `$schema`
- **TOML**：`#:schema ./schema.json`
- **YAML**：`# yaml-language-server: $schema=...`
- **YAML fallback**：`# @schema ...`

本地与远程 schema 都支持。相对本地路径会按配置文件所在目录解析；如果配置来自
stdin/内联文本，则回退到当前工作目录。对于 JSON 配置，根级 `$schema`
只作为元数据使用，会在内存中的 defaults
里剥离，避免编辑器提示字段污染最终校验与输出。

远程 `http(s)` schema 加载只存在于 `schemaui-cli`，并且 CLI 默认开启
`remote-schema` feature；如果你希望二进制只保留本地能力，可以关闭它。 `schemaui`
这个库默认不会开启任何远程 schema 加载能力。

两者的默认 feature 按使用场景刻意分开：

- `schemaui-cli` 默认走“开箱即用”路径：TUI + Web + 远程 schema 加载都开启。
- `schemaui` 作为库默认开 `tui + json`，这样保留最基础的 JSON
  能力，同时不默认引入 Web 与联网相关能力。
- `json`、`yaml`、`toml` 都是真正落在代码路径上的 feature
  gate；三者不能全关，否则会直接给出清晰的编译期错误。

参考：

- JSON Schema
  `$schema`：<https://json-schema.org/understanding-json-schema/reference/schema>
- Taplo `#:schema`
  指令：<https://taplo.tamasfe.dev/configuration/directives.html>
- YAML language server
  modeline：<https://github.com/redhat-developer/yaml-language-server>

## 快速开始

```toml
[dependencies]
schemaui = "0.12.0"
serde_json = "1"
```

```rust
use anyhow::Result;
use schemaui::prelude::*;
use serde_json::json;

fn main() -> Result<()> {
    let schema = json!({
        "$schema": "http://json-schema.org/draft-07/schema# ",
        "title": "服务运行时",
        "type": "object",
        "properties": {
            "metadata": {
                "type": "object",
                "properties": {
                    "serviceName": {"type": "string"},
                    "environment": {
                        "type": "string",
                        "enum": ["dev", "staging", "prod"]
                    }
                },
                "required": ["serviceName"]
            },
            "runtime": {
                "type": "object",
                "properties": {
                    "http": {
                        "type": "object",
                        "properties": {
                            "host": {"type": "string", "default": "0.0.0.0"},
                            "port": {"type": "integer", "minimum": 1024, "maximum": 65535}
                        }
                    }
                }
            }
        },
        "required": ["metadata", "runtime"]
    });

    let config = serde_json::json!({
        "metadata": { "name": "demo-service" },
        "runtime": { "http": { "host": "127.0.0.1", "port": 8080 } }
    });

    let value = SchemaUI::new(config)
        .with_schema(schema)
        .with_title("SchemaUI 演示")
        .with_description("基于现有配置和校验 schema 进行编辑")
        .run(FrontendOptions::Tui(UiOptions::default()))?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
```

## 公共 API 入口

在作为库集成 schemaui 时，主要的入口包括：

- **高层运行入口**：`SchemaUI`、`DocumentInput`、`FrontendOptions`、
  `ServeOptions`、`UiOptions`
- **TUI 运行时**：`crate::tui::session::TuiFrontend`（适合通过
  `SchemaUI::run_with_frontend` 注入自定义前端）
- **TUI 状态**：`crate::tui::state::*`（例如
  `FormState`、`FormCommand`、`FormEngine`、`SectionState` 等）
- **Schema 后端**：`crate::ui_ast::build_ui_ast` 配合
  `crate::tui::model::form_schema_from_ui_ast`（先生成规范 UI AST，再派生
  `FormSchema`）

## 架构快照

```
┌─────────────┐   解析/合并     ┌──────────────┐   布局+类型          ┌───────────────┐
│ io::input   ├────────────────▶│ schema       ├────────────────────▶│ tui::state    │
└─────────────┘                 │ (loader/     │                     │ (FormState,   │
                                │ resolver/    │                     │ sections,     │
┌─────────────┐   输出值        │ build_form_  │   FormSchema        │ reducers)     │
│ io::output  ◀─────────────────┴────pipeline──┘                     └────────┬──────┘
└─────────────┘                                             焦点/编辑          │
                                                                               │
                                                                        ┌──────▼──────────┐
                                                                        │ tui::app::runtime │
                                                                        │ (输入路由器,      │
                                                                        │ 覆盖层, 状态)    │
                                                                        └──────┬──────────┘
                                                                               │ 绘制
                                                                        ┌──────▼──────────┐
                                                                        │ tui::view::*    │
                                                                        │ (ratatui 视图)  │
                                                                        └─────────────────┘
```

此布局反映了`src/`下的实际模块，便于将任何代码更改映射到其架构责任。

## 输入与输出设计

- `io::input::parse_document_str`将
  JSON/YAML/TOML（通过`serde_json`、`serde_yaml`、`toml`）转换为`serde_json::Value`。功能标志（`json`、`yaml`、`toml`）不仅控制依赖项，也会真实控制`DocumentFormat`的解析与探测路径。
- `schema_from_data_value/str`从活动配置中推断模式，注入草稿 -07
  元数据和默认值，以便 UI 加载现有值。
- `schema_with_defaults`将规范模式与用户数据合并，通过`properties`、`patternProperties`、`additionalProperties`、`dependencies`、`dependentSchemas`、数组和`$ref`目标传播默认值，而不修改原始树。
- `io::output::OutputOptions`封装了序列化格式、美观/紧凑切换以及`OutputDestination::{Stdout, File}`的向量。支持多个目标；冲突在输出前被捕获。
- `OutputOptions::render` 负责把最终的 `serde_json::Value` 渲染成 JSON/YAML/TOML
  文本，`OutputOptions::write` 则在 `SchemaUI::run*` 返回后显式输出到 stdout /
  文件。

## JSON Schema → TUI 映射

`build_ui_ast` 先把解析后的模式规范化为 UI AST，再由 `form_schema_from_ui_ast`
将每个子树映射为 `FormSection`/`FieldSchema`：

| 模式功能                                                     | 结果控件                                                |
| ------------------------------------------------------------ | ------------------------------------------------------- |
| `type: string`, `integer`, `number`                          | 带有数值保护的内联文本编辑器                            |
| `type: boolean`                                              | 切换/复选框                                             |
| `enum`                                                       | 弹出选择器（单选或多选用于数组枚举）                    |
| 数组                                                         | 内联列表摘要 + 每个项目的覆盖层编辑器                   |
| `patternProperties`, `propertyNames`, `additionalProperties` | 带有模式支持验证的键值编辑器                            |
| `$ref`, `definitions`                                        | 在布局前解析；被视为内联模式                            |
| `oneOf` / `anyOf`                                            | 变体选择器 + 覆盖层表单，将非活动变体排除在最终负载之外 |

根对象生成标签；嵌套对象成为带有面包屑标题的部分。每个字段记录其 JSON
指针（例如`/runtime/http/port`），以便焦点管理和验证可以精确映射错误。

## 验证生命周期

- `jsonschema::validator_for`在`SchemaUI::run`开始时编译完整模式一次。
- 每次编辑都会触发`FormCommand::FieldEdited`。`FormEngine`通过`FormState::try_build_value`重建当前文档，运行验证器，并将错误反馈到`FieldState`或全局状态行。
- 覆盖层（复合变体、键值映射、列表条目）会根据当前正在编辑的子模式启动自己的验证器，因此问题会在离开覆盖层之前浮出水面。

```
┌─────────────┐ 解析模式   ┌───────────────────────────────┐ 膨胀状态        ┌────────────┐
│ SchemaUI::run├──────────▶│ form_schema_from_ui_ast       ├───────────────▶│ FormState  │
└─────┬───────┘            │ (tui::model::FormSchema)      │                 └──────┬─────┘
      │ validator_for()    └───────────────────────────────┘           编辑         │
      │                                                        ┌──────▼─────────┐
      └────────────────────────────────────────────────────── ▶│ app::runtime   │
                                                               │ (状态, 输入)   │
                                                               └──────┬─────────┘
                                                                      │ FormCommand
                                                               ┌──────▼──────────┐
                                                               │ FormEngine      │
                                                               │ + jsonschema    │
                                                               └─────────────────┘
```

`App`是`FormState`的唯一所有者；即使是覆盖层编辑也会通过`FormEngine`流动，以保持验证规则集中。

## TUI 构建块与快捷键

- **快捷键单一来源** –
  `keymap/default.keymap.json`列出了每个快捷键（上下文、组合键、动作）。`app::keymap::keymap_source!()`宏将此文件拉入二进制文件中，`InputRouter`使用它对`KeyEvent`进行分类，运行时页脚从相同的数据中呈现帮助文本——保持文档和行为
  DRY。
- **根标签与部分** –
  焦点通过`Ctrl+J / Ctrl+L`（根）和`Ctrl+Tab / Ctrl+Shift+Tab`（部分）循环。普通`Tab`/`Shift+Tab`在各个字段之间移动。
- **字段** –
  渲染标签、描述和内联错误消息。枚举/复合字段显示当前选择；数组总结长度和选定条目。
- **弹出窗口与覆盖层** – 按下`Enter`键打开枚举/oneOf
  选择器的弹出窗口；`Ctrl+E`打开复合编辑器的全屏覆盖层。覆盖层暴露集合快捷键（`Ctrl+N`、`Ctrl+D`、`Ctrl+←/→`、`Ctrl+↑/↓`）以及`Ctrl+S`提交。
- **状态与帮助** –
  页脚突出显示脏状态、未解决的验证错误和上下文感知帮助文本。当自动验证启用时，每次编辑都会立即更新这些计数器。

### 自动生成的快捷键参考

<!-- AUTO-GENERATED:SHORTCUTS:BEGIN -->

#### 默认上下文

| 快捷键              | 动作                                  | 类型   |
| ------------------- | ------------------------------------- | ------ |
| `Tab` / `Down`      | 下一个字段                            | `命令` |
| `BackTab` / `Up`    | 上一个字段                            | `命令` |
| `Ctrl+Tab`          | 下一个分区                            | `命令` |
| `Ctrl+Shift+Tab`    | 上一个分区                            | `命令` |
| `Ctrl+L`            | 下一个根标签                          | `命令` |
| `Ctrl+J`            | 上一个根标签                          | `命令` |
| `Enter`             | 打开弹窗 / 应用选择                   | `命令` |
| `Ctrl+E`            | 打开复合编辑器                        | `命令` |
| `Ctrl+S`            | 保存并验证（覆盖层保持打开）          | `命令` |
| `Ctrl+Q` / `Ctrl+C` | 退出（脏状态需确认）                  | `命令` |
| `Esc`               | 取消 / 清除状态（覆盖层：弹出当前层） | `命令` |
| `Ctrl+?` / `Ctrl+H` | 显示帮助与错误摘要                    | `命令` |

#### 集合上下文

| 快捷键              | 动作               | 类型   |
| ------------------- | ------------------ | ------ |
| `Ctrl+E`            | 打开复合编辑器     | `命令` |
| `Ctrl+N`            | 添加条目           | `命令` |
| `Ctrl+D`            | 删除条目           | `命令` |
| `Ctrl+Left`         | 选择上一个条目     | `命令` |
| `Ctrl+Right`        | 选择下一个条目     | `命令` |
| `Ctrl+Up`           | 条目上移           | `命令` |
| `Ctrl+Down`         | 条目下移           | `命令` |
| `Ctrl+?` / `Ctrl+H` | 显示帮助与错误摘要 | `命令` |

#### 覆盖层上下文

| 快捷键              | 动作                                  | 类型   |
| ------------------- | ------------------------------------- | ------ |
| `Tab` / `Down`      | 下一个字段                            | `命令` |
| `BackTab` / `Up`    | 上一个字段                            | `命令` |
| `Ctrl+N`            | 添加条目                              | `命令` |
| `Ctrl+D`            | 删除条目                              | `命令` |
| `Ctrl+Left`         | 选择上一个条目                        | `命令` |
| `Ctrl+Right`        | 选择下一个条目                        | `命令` |
| `Ctrl+Up`           | 条目上移                              | `命令` |
| `Ctrl+Down`         | 条目下移                              | `命令` |
| `Ctrl+S`            | 保存并验证（覆盖层保持打开）          | `命令` |
| `Esc`               | 取消 / 清除状态（覆盖层：弹出当前层） | `命令` |
| `Ctrl+?` / `Ctrl+H` | 显示帮助与错误摘要                    | `命令` |

#### 帮助上下文

| 快捷键                      | 动作           | 类型   |
| --------------------------- | -------------- | ------ |
| `Esc` / `Ctrl+H` / `Ctrl+?` | 关闭帮助       | `命令` |
| `Tab`                       | 下一页错误     | `命令` |
| `BackTab`                   | 上一页错误     | `命令` |
| `Up` / `k`                  | 快捷键向上滚动 | `命令` |
| `Down` / `j`                | 快捷键向下滚动 | `命令` |
| `PageUp`                    | 快捷键上翻页   | `命令` |
| `PageDown`                  | 快捷键下翻页   | `命令` |
| `Home`                      | 快捷键跳到顶部 | `命令` |
| `End`                       | 快捷键跳到底部 | `命令` |
| `h`                         | 错误文本左滚   | `命令` |
| `l`                         | 错误文本右滚   | `命令` |

#### 文本字段上下文

| 快捷键      | 动作           | 类型       |
| ----------- | -------------- | ---------- |
| `Left`      | 光标左移       | `局部编辑` |
| `Right`     | 光标右移       | `局部编辑` |
| `Home`      | 跳到行首       | `局部编辑` |
| `End`       | 跳到行尾       | `局部编辑` |
| `Backspace` | 删除前一个字符 | `局部编辑` |
| `Delete`    | 删除后一个字符 | `局部编辑` |
| `Ctrl+W`    | 删除前一个单词 | `局部编辑` |
| `Ctrl+Z`    | 撤销文本编辑   | `局部编辑` |
| `Ctrl+Y`    | 重做文本编辑   | `局部编辑` |

#### 数值字段上下文

| 快捷键        | 动作           | 类型       |
| ------------- | -------------- | ---------- |
| `Left`        | 数值减一步     | `局部编辑` |
| `Right`       | 数值加一步     | `局部编辑` |
| `Shift+Left`  | 数值快速减一步 | `局部编辑` |
| `Shift+Right` | 数值快速加一步 | `局部编辑` |
| `Backspace`   | 删除前一个字符 | `局部编辑` |
| `Delete`      | 删除后一个字符 | `局部编辑` |
| `Ctrl+Z`      | 撤销数值编辑   | `局部编辑` |
| `Ctrl+Y`      | 重做数值编辑   | `局部编辑` |

<!-- AUTO-GENERATED:SHORTCUTS:END -->

### 快捷键系统

将每个快捷键放入`keymap/default.keymap.json`中，以便运行时逻辑、帮助覆盖层和自动生成的
README 快捷键参考都使用单一信息源。

- **格式** – 每个 JSON
  对象声明一个`id`、英文`description`、双语`descriptionZh`、`contexts`（任何`"default"`、`"collection"`、`"overlay"`、`"help"`、`"text"`、`"numeric"`），一个`action`区分联合类型以及文本`combos`列表。例如：

  ```json
  {
    "id": "list.move.up",
    "description": "Move entry up",
    "descriptionZh": "条目上移",
    "contexts": ["collection", "overlay"],
    "action": { "kind": "ListMove", "delta": -1 },
    "combos": ["Ctrl+Up"]
  }
  ```

- **宏 + 解析器** – `app::keymap::keymap_source!()` `include_str!`s
  JSON，`once_cell::sync::Lazy`在启动时一次解析，并将每个组合键编译为`KeyPattern`（键码、所需修饰符、美观显示字符串）。
- **集成** –
  `InputRouter::classify`委托给`keymap::classify_key`，该函数返回嵌入在 JSON
  中的`KeyAction`。`keymap::help_text`根据`KeymapContext`过滤绑定，连接用于`StatusLine`和覆盖层说明的片段。
- **生成文档** – `build.rs`会解析 `keymap/default.keymap.json`，并通过显式 HTML
  marker 刷新 `README.md` 与 `README.ZH.md` 中的快捷键块；因此常规 Cargo
  构建就会让双语快捷键参考与运行时行为保持同步。
- **扩展** – 要添加快捷键，编辑
  JSON，选择暴露帮助文本的上下文，并在引入新的语义命令时在`KeyBindingMap`中连接结果`KeyAction`。

## 运行时层

| 层           | 模块 (s)                                                      | 责任                                                           |
| ------------ | ------------------------------------------------------------- | -------------------------------------------------------------- |
| 摄取         | `io::input`, `schema::loader`, `schema::resolver`             | 解析 JSON/TOML/YAML，解析`$ref`，并规范化元数据。              |
| 布局类型     | `ui_ast::build_ui_ast`, `tui::model::form_schema_from_ui_ast` | 从规范 UI AST 生成 `FormSchema`（根/部分/字段）。              |
| 表单状态     | `tui::state::{form_state, section, field}`                    | 跟踪焦点、指针、脏标志、强制转换和错误。                       |
| 命令与简化器 | `tui::state::{actions, reducers}`, `tui::app::validation`     | 定义 `FormCommand`，突变状态，并路由验证结果。                 |
| 运行时控制器 | `tui::app::{runtime, overlay, popup, status, keymap}`         | 事件循环，输入路由器分发，覆盖层生命周期，帮助文本，状态更新。 |
| 呈现         | `tui::view` 和 `tui::view::components::*`                     | 通过 `ratatui` 呈现标签、字段列表、弹出窗口、覆盖层和页脚。    |

每个模块保持在约 600 行代码以下（硬上限 800），以尊重 KISS
原则并使重构易于管理。

## CLI (`schemaui-cli`)

<!-- AUTO-GENERATED:CLI-INSTALL:BEGIN -->

### 安装

安装后的实际可执行文件名始终是 `schemaui`，所以常规入口仍然是
`schemaui -c ./config.json`。

选择下面任意一种支持的分发方式：

#### Cargo（`cargo install`）

使用 Cargo 从 crates.io 编译安装。

```bash
cargo install schemaui-cli
```

#### Cargo binstall

通过 cargo-binstall 拉取预构建的 GitHub release 二进制。

```bash
cargo binstall schemaui-cli
```

#### Homebrew

在 macOS 或 Linux 上通过仓库 tap 安装。

```bash
brew install YuniqueUnic/schemaui/schemaui
```

#### Scoop

在 Windows 上通过仓库内维护的 Scoop manifest 安装。

```bash
scoop install https://raw.githubusercontent.com/YuniqueUnic/schemaui/main/packaging/scoop/schemaui-cli.json
```

#### 直接下载

从 `https://github.com/YuniqueUnic/schemaui/releases/latest`
下载对应平台压缩包，解压 `schemaui` / `schemaui.exe` 后放到 `PATH` 中。

#### winget manifests

使用 `packaging/winget` 中的 versioned manifests 配合
`winget install --manifest <dir>` 安装，或将其提交到社区仓库。

<!-- AUTO-GENERATED:CLI-INSTALL:END -->

```bash
schemaui \
  --schema ./schema.json \
  --config ./defaults.yaml \
  -o - \
  -o ./config.toml ./config.json
```

```
┌────────┐  clap args   ┌──────────────┐ read stdin/files ┌─────────────┐
│  CLI   ├─────────────▶│ InputSource  ├─────────────────▶│ io::input   │
└────┬───┘              └──────┬───────┘                  └────┬────────┘
     │ diagnostics             │ schema/default Value          │
┌────▼─────────┐        ┌──────▼──────┐                        |
│Diagnostic    │◀───────┤ FormatHint  │                        │
│Collector     │        └──────┬──────┘                        │
└────┬─────────┘               │ pass if clean                 │
     │                         │                               │
┌────▼────────┐  build options └────────────┐                  │
│Output logic ├────────────────────────────▶│ OutputOptions    │
└────┬────────┘                             └────────────┬─────┘
     │ SchemaUI::new / with_*                        ┌───▼────────┐
     └──────────────────────────────────────────────▶│ SchemaUI   │
                                                     │ (library)  │
                                                     └────────────┘
```

- 输入 – `--schema` /
  `--config`接受文件路径、内联有效载荷或`-`用于标准输入（但不能同时使用两者）。如果只提供配置，CLI
  通过`schema_from_data_value`推断模式。
- 诊断 –
  `DiagnosticCollector`累积格式问题、功能标志不匹配、标准输入冲突和现有输出文件，以执行前的诊断。
- 输出 –
  `-o/--output`可重复使用，并且可以混合文件路径与`-`用于标准输出。当未设置目标时，工具直接写到
  stdout；如果你明确想走回退文件，可以传`--temp-file <PATH>`。扩展名决定格式；拒绝冲突的扩展名。
- 标志 – `--no-pretty`切换紧凑输出，`--force/--yes`允许覆盖文件，`--title` /
  `--description`分别传递到`SchemaUI::with_title` /
  `SchemaUI::with_description`。
- Shell completion – `schemaui completion <bash|zsh|fish|powershell>` 会通过
  `clap_complete` 从同一套 `clap` 命令树生成补全脚本。

## 关键依赖项

| 库                                          | 用途                                      |
| ------------------------------------------- | ----------------------------------------- |
| `serde`, `serde_json`, `serde_yaml`, `toml` | 解析和序列化模式/配置数据。               |
| `schemars`                                  | draft-07 模式表示，由 `schema` 模块使用。 |
| `jsonschema`                                | 表单和覆盖层的运行时验证。                |
| `ratatui`                                   | 渲染小部件、布局、覆盖层和页脚。          |
| `crossterm`                                 | 输入路由器消耗的终端事件。                |
| `indexmap`                                  | 模式遍历的顺序保持映射。                  |
| `once_cell`                                 | 懒解析快捷键 JSON。                       |
| `clap`, `clap_complete`, `anyhow` (CLI)     | 参数解析、Shell 补全生成和友好的诊断。    |

## 文档映射

- `README.md` – 概述 + 架构快照（英文版本为事实来源）。
- `README.ZH.md` – 本中文概览文档（尽量与英文保持同步）。
- `docs/en/structure_design.md` – 英文版模式/布局/运行时设计，带有流程图。
- `docs/zh/structure_design.md` – 本中文架构指南，与英文版对应。
- `docs/en/cli_usage.md` – 英文 CLI 使用手册（输入、输出、管道、示例）。
- `docs/zh/cli_usage.zh.md` – 中文 CLI 使用手册，与英文版对应。

## 开发

- 定期运行`cargo fmt && cargo test`；大多数模块通过`include!`嵌入`tests/`中的文件，以覆盖私有
  API。
- 将模块保持在约 600 行代码以下（硬上限
  800）。一旦行为增长，就拆分帮助程序，以保持 KISS 完整。
- 优先使用成熟的库（`serde_*`、`schemars`、`jsonschema`、`ratatui`、`crossterm`、`once_cell`），除非更改微不足道，否则不要编写定制代码。
- 每当管道、快捷键或 CLI 语义演变时，更新`docs/*`，以确保面向用户文档的真实性。

## 参考项目

1. https://github.com/rjsf-team/react-jsonschema-form
2. https://ui-schema.bemit.codes/examples

## 路线图

- [x] 在运行时解析 JSON Schema 并生成 TUI
- [x] 在运行时解析 JSON Schema 并生成 Web UI
- [x] 在编译时解析 JSON Schema，然后生成 TUI 代码，为运行时暴露必要的 API
- [x] 在编译时解析 JSON Schema，然后生成 Web UI 代码，为运行时暴露必要的 API
- [ ] 在运行时解析 JSON Schema 并生成交互式 CLI
- [ ] 在编译时解析 JSON Schema，然后生成交互式 CLI 代码，为运行时暴露必要的 API

## 许可证

根据您的选择，本项目可在以下许可证下授权：

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) 或
  http://www.apache.org/licenses/LICENSE-2.0 )
- MIT license ([LICENSE-MIT](LICENSE-MIT) 或 http://opensource.org/licenses/MIT
  )

### 贡献

欢迎贡献！请随时提交拉取请求。

祝您编程愉快！

## Star 历史

<a href="https://www.star-history.com/#YuniqueUnic/schemaui&type=date&legend=top-left">
<picture>
  <source
    media="(prefers-color-scheme: dark)"
    srcset="
      https://api.star-history.com/svg?repos=YuniqueUnic/schemaui&type=date&legend=top-left&theme=dark
    "
  />
  <source
    media="(prefers-color-scheme: light)"
    srcset="
      https://api.star-history.com/svg?repos=YuniqueUnic/schemaui&type=date&legend=top-left
    "
  />
  <img
    alt="Star History Chart"
    src="https://api.star-history.com/svg?repos=YuniqueUnic/schemaui&type=date&legend=top-left"
  />
</picture>
</a>
