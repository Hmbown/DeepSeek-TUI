# 无障碍

[English](ACCESSIBILITY.md) | [繁體中文](ACCESSIBILITY.zh-TW.md)

DeepSeek-TUI 在终端中运行，因此平台自身的无障碍堆栈（屏幕阅读器、放大镜、终端级别主题）完成了大部分工作。TUI 提供一小组开关，为屏幕阅读器和低动态效果用户减少视觉动效和密度。

## 快速参考

| 开关 | 默认值 | 效果 |
| --- | --- | --- |
| `NO_ANIMATIONS=1` 环境变量 | 未设置 | 在启动时强制 `low_motion = true` 且 `fancy_animations = false`。覆盖 `settings.toml` 中保存的任何设置。 |
| `low_motion` 设置 | `false` | 抑制 spinner 动态效果、transcript 淡入动画、footer 漂移、header 状态指示器循环以及活动单元格脉冲。帧率限制器也会降低空闲重绘速度，使光标不再那么频繁闪烁。 |
| `fancy_animations` 设置 | `false` | Footer 水柱条带和 pulsating sub-agent 计数器。默认关闭。 |
| `status_indicator` 设置 | `whale` | Header 状态芯片。设为 `dots` 使用紧凑的点循环，或设为 `off` 隐藏它。 |
| `calm_mode` 设置 | `false` | 默认折叠 tool 输出详情并精简状态消息。适用于会在每次重绘时播报的屏幕阅读器。 |
| `show_thinking` 设置 | `true` | 设为 `false` 完全隐藏模型的 `reasoning_content` 块。 |
| `show_tool_details` 设置 | `true` | 设为 `false` 将 tool 调用渲染为单行摘要，不展开 payload。 |

## 标准环境变量接口

在你的 shell 配置文件中设置这些变量，使其应用于每个会话：

```bash
# 强制低动态效果 + 无花哨动画。
export NO_ANIMATIONS=1

# 可选：遵循更广泛的终端颜色约定。
export NO_COLOR=1            # 底层 ratatui 后端会遵循此设置
```

`NO_ANIMATIONS` 接受 `1`、`true`、`yes` 或 `on` 中的任意值（不区分大小写）。任何其他值（包括 `0`、`false`、空值或未设置）将保持你已保存的设置不变。

该覆盖在启动时应用一次。在会话进行中更改环境变量不会产生任何效果——设置仅在下一次启动时重新读取。

## 通过 `/settings` 进行配置

同样的开关也可以通过命令面板访问：

* `/settings set low_motion on`
* `/settings set fancy_animations off`
* `/settings set calm_mode on`
* `/settings set status_indicator off`

通过这些方式写入的设置会持久保存到 `~/.config/deepseek/settings.toml`。但如果在启动时设置了 `NO_ANIMATIONS` 环境变量，它仍然会优先。因此若想遵循已保存的选择，需要取消该环境变量。

Tilix 和 Terminator 会话会自动以低动态效果模式启动，因为这些基于 VTE 的终端已报告在活跃回合期间存在可见的重绘闪烁。如果你的终端版本渲染干净，你仍可以在启动后覆盖已保存的设置。

## 屏幕阅读器用户注意事项

* `low_motion` 将空闲重绘循环降低到每帧约 120ms，使光标不会频繁重新定位。结合 `calm_mode`，重绘速率足够低，使得 VoiceOver/Orca 的播报与模型输出成线性对齐，而不是在每个时钟周期重新朗读整个屏幕。
* transcript 是纯文本——没有图像或画布渲染——因此任何与平台无障碍服务集成的终端（例如 macOS Terminal.app、iTerm2、Ghostty、Windows Terminal）都会直接将渲染内容传递过去。
* 如果你发现在 `low_motion = true` 时仍有 UI 元素在产生动态效果，请在 [`PRIOR: Screen-reader / accessibility flag`](https://github.com/Hmbown/DeepSeek-TUI/issues/450) 下提交 issue，并附上截图或终端录制。

## 相关的 issues / 历史

* [#450](https://github.com/Hmbown/DeepSeek-TUI/issues/450) —— 记录现有标志、添加 `NO_ANIMATIONS` 启动覆盖以及编写本页面。
* [#449](https://github.com/Hmbown/DeepSeek-TUI/issues/449) —— footer 状态栏现在使用活动主题的对比色对，而非自定义调色板。
