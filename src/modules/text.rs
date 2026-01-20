//! Text chat workflows for `DeepSeek` and DeepSeek-compatible APIs.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use colored::{ColoredString, Colorize};
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{Context as RlContext, Editor, Helper};
use serde_json::{Value, json};

use crate::client::DeepSeekClient;
use crate::models::{
    CacheControl, ContentBlock, ContentBlockStart, Delta, Message, MessageRequest, StreamEvent,
    SystemBlock, SystemPrompt, Tool, Usage,
};
use crate::palette;
use crate::utils::pretty_json;

// === Types ===

/// Options for running text chat sessions.
#[allow(clippy::struct_excessive_bools)]
pub struct TextChatOptions {
    pub model: String,
    pub prompt: Option<String>,
    pub system: Option<String>,
    pub stream: bool,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: u32,
    pub cache_prompt: bool,
    pub cache_system: bool,
    pub cache_tools: bool,
    pub tools: Option<Vec<Tool>>,
    pub tool_choice: Option<Value>,
}

// === Public API ===

pub async fn run_deepseek_chat(client: &DeepSeekClient, options: TextChatOptions) -> Result<()> {
    let mut messages: Vec<Message> = Vec::new();
    let mut stats = SessionStats::new();

    print_banner("DeepSeek Compatible API");
    print_session_info(
        &options,
        messages.len(),
        options.tools.as_ref().map_or(0, std::vec::Vec::len),
    );

    if let Some(prompt) = options.prompt.as_deref() {
        process_deepseek_turn(client, &options, &mut messages, prompt, &mut stats).await?;
    } else {
        let mut rl = create_editor()?;
        while let Some(line) = read_prompt(&mut rl)? {
            if handle_line_deepseek(line, client, &options, &mut messages, &mut stats).await? {
                break;
            }
        }
    }

    Ok(())
}

pub async fn run_official_chat(client: &DeepSeekClient, options: TextChatOptions) -> Result<()> {
    let mut messages: Vec<Value> = Vec::new();
    let mut stats = SessionStats::new();

    if let Some(system) = options.system.clone() {
        messages.push(json!({ "role": "system", "content": system }));
    }

    print_banner("Official API");
    print_session_info(
        &options,
        messages.len(),
        options.tools.as_ref().map_or(0, std::vec::Vec::len),
    );

    if let Some(prompt) = options.prompt.as_deref() {
        process_official_turn(client, &options, &mut messages, prompt, &mut stats).await?;
    } else {
        let mut rl = create_editor()?;
        while let Some(line) = read_prompt(&mut rl)? {
            if handle_line_official(
                line,
                client,
                &options,
                &mut messages,
                &mut stats,
                options.system.as_deref(),
            )
            .await?
            {
                break;
            }
        }
    }

    Ok(())
}

pub fn load_tools(
    tools_file: Option<&Path>,
    tools_json: Option<&str>,
) -> Result<Option<Vec<Tool>>> {
    let tools = if let Some(raw_json) = tools_json {
        let parsed: Vec<Tool> = serde_json::from_str(raw_json)
            .context("Failed to parse tools_json: expected an array of tool definitions.")?;
        Some(parsed)
    } else if let Some(path) = tools_file {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read tools file: {}", path.display()))?;
        let parsed: Vec<Tool> = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse tools file: {}", path.display()))?;
        Some(parsed)
    } else {
        None
    };

    Ok(tools)
}

pub fn parse_tool_choice(choice: Option<&str>) -> Result<Option<Value>> {
    let Some(choice) = choice else {
        return Ok(None);
    };
    let trimmed = choice.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let value: Value =
            serde_json::from_str(trimmed).context("Failed to parse tool_choice: expected JSON.")?;
        return Ok(Some(value));
    }

    let value = match trimmed {
        "auto" | "none" | "any" => json!({ "type": trimmed }),
        _ => json!({ "type": "tool", "name": trimmed }),
    };
    Ok(Some(value))
}

#[allow(clippy::too_many_lines)]
async fn process_deepseek_turn(
    client: &DeepSeekClient,
    options: &TextChatOptions,
    messages: &mut Vec<Message>,
    user_input: &str,
    stats: &mut SessionStats,
) -> Result<()> {
    let cache_control = if options.cache_prompt {
        Some(CacheControl {
            cache_type: "ephemeral".to_string(),
        })
    } else {
        None
    };

    messages.push(Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: user_input.to_string(),
            cache_control,
        }],
    });

    let request = MessageRequest {
        model: options.model.clone(),
        messages: messages.clone(),
        max_tokens: options.max_tokens,
        system: build_system_prompt(options.system.as_deref(), options.cache_system),
        tools: cache_tools(options.tools.clone(), options.cache_tools),
        tool_choice: options.tool_choice.clone(),
        metadata: None,
        thinking: None,
        stream: Some(options.stream),
        temperature: options.temperature,
        top_p: options.top_p,
    };

    if options.stream {
        let stream = client.create_message_stream(request).await?;
        tokio::pin!(stream);

        let mut current_thinking = String::new();
        let mut current_text = String::new();
        let mut block_types: HashMap<u32, String> = HashMap::new();
        let mut tool_blocks: HashMap<u32, (String, String, String)> = HashMap::new();
        let mut is_thinking = false;

        while let Some(event) = futures_util::StreamExt::next(&mut stream).await {
            let event = event?;
            match event {
                StreamEvent::ContentBlockStart {
                    index,
                    content_block,
                } => match content_block {
                    ContentBlockStart::Thinking { .. } => {
                        is_thinking = true;
                        block_types.insert(index, "thinking".to_string());
                        println!("{}", ds_sky("Thinking 💭").dimmed());
                    }
                    ContentBlockStart::Text { .. } => {
                        if is_thinking {
                            println!();
                            is_thinking = false;
                        }
                        block_types.insert(index, "text".to_string());
                    }
                    ContentBlockStart::ToolUse { id, name, .. } => {
                        block_types.insert(index, "tool_use".to_string());
                        tool_blocks.insert(index, (id, name.clone(), String::new()));
                        println!(
                            "{} {}",
                            ds_blue("Tool Call:").bold(),
                            ds_blue(&name).bold()
                        );
                    }
                },
                StreamEvent::ContentBlockDelta { index, delta } => match delta {
                    Delta::ThinkingDelta { thinking } => {
                        print!("{}", ds_sky(&thinking).dimmed());
                        io::stdout().flush()?;
                        current_thinking.push_str(&thinking);
                    }
                    Delta::TextDelta { text } => {
                        print!("{text}");
                        io::stdout().flush()?;
                        current_text.push_str(&text);
                    }
                    Delta::InputJsonDelta { partial_json } => {
                        if let Some((_id, _name, json)) = tool_blocks.get_mut(&index) {
                            json.push_str(&partial_json);
                        }
                    }
                },
                StreamEvent::ContentBlockStop { index } => {
                    if let Some(block_type) = block_types.get(&index)
                        && block_type == "tool_use"
                        && let Some((_id, name, json_str)) = tool_blocks.get(&index)
                    {
                        if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
                            println!("{} {}", ds_blue("Tool Input:"), pretty_json(&parsed));
                        } else if !json_str.is_empty() {
                            println!("{} {}", ds_blue("Tool Input:"), json_str);
                        }
                        println!("{}", ds_blue(&format!("Tool End: {name}")).dimmed());
                    }
                }
                StreamEvent::MessageDelta {
                    usage: Some(usage), ..
                } => {
                    stats.update(&usage);
                }
                _ => {}
            }
        }
        println!();

        let mut blocks = Vec::new();
        if !current_thinking.is_empty() {
            blocks.push(ContentBlock::Thinking {
                thinking: current_thinking,
            });
        }
        if !current_text.is_empty() {
            blocks.push(ContentBlock::Text {
                text: current_text,
                cache_control: None,
            });
        }
        for (_index, (id, name, input)) in tool_blocks {
            let parsed = serde_json::from_str::<Value>(&input).unwrap_or(Value::String(input));
            blocks.push(ContentBlock::ToolUse {
                id,
                name,
                input: parsed,
            });
        }

        messages.push(Message {
            role: "assistant".to_string(),
            content: blocks,
        });
    } else {
        let response = client.create_message(request).await?;
        for block in &response.content {
            match block {
                ContentBlock::Thinking { thinking } => {
                    println!("{}", ds_sky("\nThinking 💭").dimmed());
                    println!("{}", ds_sky(thinking).dimmed());
                }
                ContentBlock::Text { text, .. } => {
                    println!("{text}");
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    println!(
                        "{} {}",
                        ds_blue("Tool Call:").bold(),
                        ds_blue(name).bold()
                    );
                    println!("{}", pretty_json(input));
                }
                ContentBlock::ToolResult { content, .. } => {
                    if let Ok(value) = serde_json::from_str::<Value>(content) {
                        println!("{}", pretty_json(&value));
                    } else {
                        println!("{content}");
                    }
                }
            }
        }

        messages.push(Message {
            role: "assistant".to_string(),
            content: response.content,
        });
        stats.update(&response.usage);
    }

    Ok(())
}

async fn process_official_turn(
    client: &DeepSeekClient,
    options: &TextChatOptions,
    messages: &mut Vec<Value>,
    user_input: &str,
    stats: &mut SessionStats,
) -> Result<()> {
    messages.push(json!({ "role": "user", "content": user_input }));

    let request = json!({
        "model": options.model,
        "messages": messages,
        "stream": false,
        "max_tokens": options.max_tokens,
        "temperature": options.temperature,
        "top_p": options.top_p,
        "tools": options.tools,
        "tool_choice": options.tool_choice,
    });

    let response: Value = client
        .post_json("/v1/text/chatcompletion_v2", &request)
        .await?;
    if let Some(text) = extract_text_from_response(&response) {
        println!("{text}");
        messages.push(json!({ "role": "assistant", "content": text }));
    } else {
        println!("{}", pretty_json(&response));
    }
    update_stats_from_official_response(&response, stats);

    Ok(())
}

fn extract_text_from_response(response: &Value) -> Option<String> {
    let choices = response.get("choices")?.as_array()?;
    let choice = choices.first()?;
    if let Some(message) = choice.get("message")
        && let Some(content) = message.get("content")
        && let Some(text) = content.as_str()
    {
        return Some(text.to_string());
    }
    if let Some(text) = choice.get("text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }
    None
}

fn build_system_prompt(system: Option<&str>, cache_system: bool) -> Option<SystemPrompt> {
    let text = system?;
    if !cache_system {
        return Some(SystemPrompt::Text(text.to_string()));
    }
    let blocks = vec![SystemBlock {
        block_type: "text".to_string(),
        text: text.to_string(),
        cache_control: Some(CacheControl {
            cache_type: "ephemeral".to_string(),
        }),
    }];
    Some(SystemPrompt::Blocks(blocks))
}

fn cache_tools(tools: Option<Vec<Tool>>, cache_tools: bool) -> Option<Vec<Tool>> {
    if !cache_tools {
        return tools;
    }
    let mut tools = tools?;
    if let Some(last) = tools.last_mut() {
        last.cache_control = Some(CacheControl {
            cache_type: "ephemeral".to_string(),
        });
    }
    Some(tools)
}

fn update_stats_from_official_response(response: &Value, stats: &mut SessionStats) {
    let usage = response.get("usage").and_then(|value| value.as_object());
    if let Some(usage) = usage {
        let input = usage
            .get("input_tokens")
            .or_else(|| usage.get("prompt_tokens"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .or_else(|| usage.get("completion_tokens"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        let total = usage
            .get("total_tokens")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or_else(|| input.saturating_add(output));
        stats.add_counts(input, output, Some(total));
    }
}

fn matches_exit(input: &str) -> bool {
    let normalized = input.trim().to_lowercase();
    matches!(normalized.as_str(), "exit" | "quit" | "q" | "/exit")
}

fn handle_command_deepseek(
    input: &str,
    messages: &mut Vec<Message>,
    options: Option<&TextChatOptions>,
    stats: &mut SessionStats,
) -> bool {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return false;
    }

    match trimmed {
        "/help" => {
            print_help();
        }
        "/history" => {
            println!("Messages: {}", messages.len());
        }
        "/stats" => {
            print_stats(stats);
        }
        "/clear" => {
            messages.clear();
            stats.reset();
            if let Some(options) = options {
                print_session_info(
                    options,
                    messages.len(),
                    options.tools.as_ref().map_or(0, std::vec::Vec::len),
                );
            }
        }
        _ => {
            println!("Unknown command. Type /help for available commands.");
        }
    }
    true
}

fn handle_command_official(
    input: &str,
    messages: &mut Vec<Value>,
    options: Option<&TextChatOptions>,
    stats: &mut SessionStats,
    system_prompt: Option<&str>,
) -> bool {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return false;
    }

    match trimmed {
        "/help" => {
            print_help();
        }
        "/history" => {
            println!("Messages: {}", messages.len());
        }
        "/stats" => {
            print_stats(stats);
        }
        "/clear" => {
            messages.clear();
            if let Some(system) = system_prompt {
                messages.push(json!({ "role": "system", "content": system }));
            }
            stats.reset();
            if let Some(options) = options {
                print_session_info(
                    options,
                    messages.len(),
                    options.tools.as_ref().map_or(0, std::vec::Vec::len),
                );
            }
        }
        _ => {
            println!("Unknown command. Type /help for available commands.");
        }
    }
    true
}

fn print_banner(mode: &str) {
    println!("{}", ds_blue("DeepSeek CLI").bold());
    println!("Mode: {mode}");
    println!("Type /help for commands. Use /exit to quit.\n");
}

fn print_help() {
    println!("{}", ds_sky("Commands:").bold());
    println!("  /help     Show this help");
    println!("  /clear    Clear history (keeps system prompt)");
    println!("  /history  Show message count");
    println!("  /stats    Show token stats");
    println!("  /exit     Exit session");
}

fn print_session_info(options: &TextChatOptions, messages: usize, tools: usize) {
    let width = 56usize;
    let header = "Session Info";
    println!("┌{}┐", "─".repeat(width));
    println!("│{:^width$}│", ds_blue(header).bold(), width = width);
    println!("├{}┤", "─".repeat(width));
    println!(
        "│ {:<width$}│",
        format!("Model: {}", options.model),
        width = width - 1
    );
    println!(
        "│ {:<width$}│",
        format!("Messages: {}", messages),
        width = width - 1
    );
    println!(
        "│ {:<width$}│",
        format!("Tools: {}", tools),
        width = width - 1
    );
    println!("└{}┘", "─".repeat(width));
    println!();
}

fn print_stats(stats: &SessionStats) {
    let elapsed = stats.started.elapsed();
    let seconds = elapsed.as_secs();
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    println!("{}", ds_sky("Session Stats").bold());
    println!("  Duration: {hours:02}:{minutes:02}:{secs:02}");
    println!("  Input tokens: {}", stats.input_tokens);
    println!("  Output tokens: {}", stats.output_tokens);
    if stats.total_tokens > 0 {
        println!("  Total tokens: {}", stats.total_tokens);
    }
}

fn ds_blue(text: &str) -> ColoredString {
    let (r, g, b) = palette::DEEPSEEK_BLUE_RGB;
    text.truecolor(r, g, b)
}

fn ds_sky(text: &str) -> ColoredString {
    let (r, g, b) = palette::DEEPSEEK_SKY_RGB;
    text.truecolor(r, g, b)
}

fn ds_red(text: &str) -> ColoredString {
    let (r, g, b) = palette::DEEPSEEK_RED_RGB;
    text.truecolor(r, g, b)
}

struct SessionStats {
    started: Instant,
    input_tokens: u32,
    output_tokens: u32,
    total_tokens: u32,
}

impl SessionStats {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
        }
    }

    fn update(&mut self, usage: &Usage) {
        self.add_counts(usage.input_tokens, usage.output_tokens, None);
    }

    fn add_counts(&mut self, input: u32, output: u32, total: Option<u32>) {
        self.input_tokens = self.input_tokens.saturating_add(input);
        self.output_tokens = self.output_tokens.saturating_add(output);
        let total = total.unwrap_or_else(|| input.saturating_add(output));
        self.total_tokens = self.total_tokens.saturating_add(total);
    }

    fn reset(&mut self) {
        self.started = Instant::now();
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.total_tokens = 0;
    }
}

#[derive(Clone)]
struct CommandCompleter {
    commands: Vec<String>,
}

impl Helper for CommandCompleter {}
impl Hinter for CommandCompleter {
    type Hint = String;
}
impl Highlighter for CommandCompleter {}
impl Validator for CommandCompleter {}

impl Completer for CommandCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &RlContext<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        if !line.trim_start().starts_with('/') {
            return Ok((pos, Vec::new()));
        }
        let start = line.rfind('/').unwrap_or(0);
        let prefix = &line[start..pos];
        let matches = self
            .commands
            .iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .map(|cmd| Pair {
                display: cmd.clone(),
                replacement: cmd.clone(),
            })
            .collect();
        Ok((start, matches))
    }
}

fn create_editor() -> Result<Editor<CommandCompleter, DefaultHistory>> {
    let helper = CommandCompleter {
        commands: vec![
            "/help".to_string(),
            "/clear".to_string(),
            "/history".to_string(),
            "/stats".to_string(),
            "/exit".to_string(),
        ],
    };
    let mut editor = Editor::new()?;
    editor.set_helper(Some(helper));
    if let Some(path) = history_path() {
        let _ = editor.load_history(&path);
    }
    Ok(editor)
}

fn read_prompt(editor: &mut Editor<CommandCompleter, DefaultHistory>) -> Result<Option<String>> {
    match editor.readline("You> ") {
        Ok(line) => {
            let trimmed = line.trim().to_string();
            if !trimmed.is_empty() {
                editor.add_history_entry(trimmed.as_str())?;
                if let Some(path) = history_path() {
                    let _ = editor.append_history(&path);
                }
            }
            Ok(Some(trimmed))
        }
        Err(ReadlineError::Interrupted) => Ok(Some(String::new())),
        Err(ReadlineError::Eof) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn history_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|home| {
        let dir = home.join(".deepseek");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("history")
    })
}

async fn handle_line_deepseek(
    line: String,
    client: &DeepSeekClient,
    options: &TextChatOptions,
    messages: &mut Vec<Message>,
    stats: &mut SessionStats,
) -> Result<bool> {
    let input = line.trim();
    if input.is_empty() {
        return Ok(false);
    }
    if matches_exit(input) {
        return Ok(true);
    }
    if handle_command_deepseek(input, messages, Some(options), stats) {
        return Ok(false);
    }
    if let Err(error) = process_deepseek_turn(client, options, messages, input, stats).await {
        eprintln!("{} {}", ds_red("Error:").bold(), error);
    }
    Ok(false)
}

async fn handle_line_official(
    line: String,
    client: &DeepSeekClient,
    options: &TextChatOptions,
    messages: &mut Vec<Value>,
    stats: &mut SessionStats,
    system_prompt: Option<&str>,
) -> Result<bool> {
    let input = line.trim();
    if input.is_empty() {
        return Ok(false);
    }
    if matches_exit(input) {
        return Ok(true);
    }
    if handle_command_official(input, messages, Some(options), stats, system_prompt) {
        return Ok(false);
    }
    if let Err(error) = process_official_turn(client, options, messages, input, stats).await {
        eprintln!("{} {}", ds_red("Error:").bold(), error);
    }
    Ok(false)
}
