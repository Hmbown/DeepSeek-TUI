//! Main GUI application state and rendering.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use deepseek_tui::config::{Config, save_api_key};
use deepseek_tui::core::engine::{EngineConfig, EngineHandle, spawn_engine};
use deepseek_tui::core::events::Event as EngineEvent;
use deepseek_tui::core::ops::Op;


use egui::Color32;
use crate::theme::{DeepSeekColors, apply as apply_theme};

/// One entry in the chat transcript.
#[derive(Debug, Clone)]
pub enum ChatMessage {
    User { text: String },
    Assistant {
        text: String,
        thinking: Option<String>,
    },
    ToolCall {
        name: String,
        input: String,
    },
    ToolResult {
        name: String,
        output: String,
    },
    SystemError { text: String },
}

/// A tool call awaiting user approval.
#[derive(Debug, Clone)]
struct PendingApproval {
    id: String,
    tool_name: String,
    description: String,
}

#[derive(Debug, Clone)]
pub struct OpenFile {
    path: PathBuf,
    content: String,
    dirty: bool,
    /// Cached syntax highlight LayoutJob to avoid recomputing every frame.
    highlight_cache: Option<(String, egui::text::LayoutJob)>,
}

/// One entry in the file tree (async-loaded).
#[derive(Debug, Clone)]
struct FileTreeEntry {
    path: PathBuf,
    is_dir: bool,
}

/// Result of an async file-save operation.
#[derive(Debug, Clone)]
struct SaveResult {
    path: PathBuf,
    success: bool,
    error: Option<String>,
}

pub struct GuiApp {
    config: Config,
    engine: Option<EngineHandle>,
    event_rx: Receiver<EngineEvent>,
    messages: Vec<ChatMessage>,
    input: String,
    is_streaming: bool,
    current_thinking: String,
    current_assistant_text: String,
    status: String,
    pending_approvals: Vec<PendingApproval>,
    show_settings: bool,
    settings_api_key: String,
    settings_model: String,
    should_scroll_to_bottom: bool,
    // Editor / file-tree state
    workspace_path: PathBuf,
    open_files: Vec<OpenFile>,
    active_file_index: usize,
    // Syntax highlighting
    syntax_set: Arc<syntect::parsing::SyntaxSet>,
    theme: Arc<syntect::highlighting::Theme>,
    // Deferred tab-close actions (applied after context menu closes to avoid egui state issues)
    pending_close_all: bool,
    pending_close_others: Option<usize>,
    // Deferred auto-save flag
    pending_auto_save: bool,
    // Async file-tree I/O
    file_tree_cache: HashMap<PathBuf, Vec<FileTreeEntry>>,
    file_tree_pending: HashSet<PathBuf>,
    file_tree_tx: Sender<(PathBuf, Vec<FileTreeEntry>)>,
    file_tree_rx: Receiver<(PathBuf, Vec<FileTreeEntry>)>,
    // Async file-save results
    save_result_tx: Sender<SaveResult>,
    save_result_rx: Receiver<SaveResult>,
}

impl GuiApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&_cc.egui_ctx);

        let (event_tx, event_rx) = std::sync::mpsc::channel::<EngineEvent>();
        let (file_tree_tx, file_tree_rx) = std::sync::mpsc::channel::<(PathBuf, Vec<FileTreeEntry>)>();
        let (save_result_tx, save_result_rx) = std::sync::mpsc::channel::<SaveResult>();

        let (config, engine) = match load_config() {
            Ok(cfg) => {
                let engine = match init_engine(&cfg, event_tx) {
                    Ok(handle) => {
                        tracing::info!("Engine spawned successfully");
                        Some(handle)
                    }
                    Err(e) => {
                        tracing::error!("Failed to spawn engine: {e}");
                        None
                    }
                };
                (cfg, engine)
            }
            Err(e) => {
                tracing::error!("Failed to load config: {e}");
                let default_cfg = Config::default();
                (default_cfg, None)
            }
        };

        let engine_ready = engine.is_some();
        let syntax_set = Arc::new(syntect::parsing::SyntaxSet::load_defaults_newlines());
        let theme = Arc::new(syntect::highlighting::ThemeSet::load_defaults().themes["base16-ocean.dark"].clone());
        Self {
            config: config.clone(),
            engine,
            event_rx,
            messages: Vec::new(),
            input: String::new(),
            is_streaming: false,
            current_thinking: String::new(),
            current_assistant_text: String::new(),
            status: if engine_ready {
                "Ready".to_string()
            } else {
                "Config error — check API key".to_string()
            },
            pending_approvals: Vec::new(),
            show_settings: false,
            settings_api_key: String::new(),
            settings_model: config.default_model(),
            should_scroll_to_bottom: false,
            workspace_path: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            open_files: Vec::new(),
            active_file_index: 0,
            syntax_set,
            theme,
            pending_close_all: false,
            pending_close_others: None,
            pending_auto_save: false,
            file_tree_cache: HashMap::new(),
            file_tree_pending: HashSet::new(),
            file_tree_tx,
            file_tree_rx,
            save_result_tx,
            save_result_rx,
        }
    }

    /// Drain all pending engine events and update state.
    /// Drain async file-tree I/O results and update the cache.
    fn poll_file_tree_results(&mut self) {
        while let Ok((path, entries)) = self.file_tree_rx.try_recv() {
            self.file_tree_pending.remove(&path);
            self.file_tree_cache.insert(path, entries);
        }
    }

    fn poll_engine_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                EngineEvent::MessageStarted { .. } => {
                    self.is_streaming = true;
                    self.current_assistant_text.clear();
                    self.current_thinking.clear();
                    self.should_scroll_to_bottom = true;
                }
                EngineEvent::MessageDelta { content, .. } => {
                    self.current_assistant_text.push_str(&content);
                    self.should_scroll_to_bottom = true;
                }
                EngineEvent::MessageComplete { .. } => {
                    self.is_streaming = false;
                    if !self.current_assistant_text.is_empty() {
                        self.messages.push(ChatMessage::Assistant {
                            text: self.current_assistant_text.clone(),
                            thinking: if self.current_thinking.is_empty() {
                                None
                            } else {
                                Some(self.current_thinking.clone())
                            },
                        });
                        self.current_assistant_text.clear();
                        self.current_thinking.clear();
                    }
                    self.status = "Ready".to_string();
                    self.should_scroll_to_bottom = true;
                }
                EngineEvent::ThinkingStarted { .. } => {
                    self.current_thinking.clear();
                    self.should_scroll_to_bottom = true;
                }
                EngineEvent::ThinkingDelta { content, .. } => {
                    self.current_thinking.push_str(&content);
                    self.should_scroll_to_bottom = true;
                }
                EngineEvent::ThinkingComplete { .. } => {
                    self.should_scroll_to_bottom = true;
                }
                EngineEvent::ToolCallStarted { name, input, .. } => {
                    self.messages.push(ChatMessage::ToolCall {
                        name,
                        input: input.to_string(),
                    });
                    self.should_scroll_to_bottom = true;
                }
                EngineEvent::ToolCallComplete { name, result, .. } => {
                    let output = match result {
                        Ok(r) => r.content.clone(),
                        Err(e) => format!("Error: {e}"),
                    };
                    self.messages.push(ChatMessage::ToolResult { name, output });
                    self.should_scroll_to_bottom = true;
                }
                EngineEvent::ApprovalRequired {
                    id,
                    tool_name,
                    description,
                    ..
                } => {
                    self.pending_approvals.push(PendingApproval {
                        id,
                        tool_name,
                        description,
                    });
                }
                EngineEvent::Status { message } => {
                    self.status = message;
                }
                EngineEvent::Error { envelope, .. } => {
                    let friendly = friendly_error_message(&envelope.message);
                    self.status = friendly.clone();
                    self.messages.push(ChatMessage::SystemError {
                        text: friendly,
                    });
                    self.is_streaming = false;
                    self.should_scroll_to_bottom = true;
                }
                _ => {}
            }
        }
    }

    fn send_message(&mut self) {
        if self.is_streaming {
            return; // wait for the current response to finish
        }
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.input.clear();
        self.messages.push(ChatMessage::User { text: text.clone() });
        self.should_scroll_to_bottom = true;
        self.status = "Thinking...".to_string();

        if let Some(engine) = &self.engine {
            let op = Op::SendMessage {
                content: text,
                mode: deepseek_tui::tui::app::AppMode::Agent,
                model: self.config.default_model(),
                goal_objective: None,
                reasoning_effort: None,
                allow_shell: true,
                trust_mode: false,
                auto_approve: false,
            };
            let engine = engine.clone();
            tokio::spawn(async move {
                let _ = engine.send(op).await;
            });
        } else {
            self.messages.push(ChatMessage::SystemError {
                text: "Engine not ready. Please set your API key in Settings (⚙) and save."
                    .to_string(),
            });
            self.status = "Engine not ready".to_string();
        }
    }

    /// Restart the engine with the latest config.
    fn restart_engine(&mut self) {
        if let Some(engine) = self.engine.take() {
            engine.cancel();
        }

        let (event_tx, event_rx) = std::sync::mpsc::channel::<EngineEvent>();
        self.event_rx = event_rx;

        match load_config() {
            Ok(cfg) => {
                self.config = cfg.clone();
                match init_engine(&self.config, event_tx) {
                    Ok(handle) => {
                        tracing::info!("Engine restarted successfully");
                        self.engine = Some(handle);
                        self.status = "Ready".to_string();
                    }
                    Err(e) => {
                        tracing::error!("Failed to restart engine: {e}");
                        self.status = format!("Engine start failed: {e}");
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to reload config: {e}");
                self.status = format!("Config reload failed: {e}");
            }
        }
    }

    /// Render the left sidebar with session info and controls.
    /// Render the settings window.
    fn render_settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(DeepSeekColors::SURFACE)
                    .stroke(egui::Stroke::new(1.0, DeepSeekColors::BORDER)),
            )
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.label(
                    egui::RichText::new("API Configuration")
                        .color(DeepSeekColors::TEXT_PRIMARY)
                        .size(16.0)
                        .strong(),
                );
                ui.add_space(8.0);

                ui.label(
                    egui::RichText::new("API Key")
                        .color(DeepSeekColors::TEXT_SECONDARY)
                        .size(13.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings_api_key)
                        .password(true)
                        .hint_text("sk-..."),
                );
                ui.add_space(6.0);

                ui.label(
                    egui::RichText::new("Model")
                        .color(DeepSeekColors::TEXT_SECONDARY)
                        .size(13.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings_model)
                        .hint_text("deepseek-chat"),
                );
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    if ui
                        .button(egui::RichText::new("Save").color(DeepSeekColors::SUCCESS))
                        .clicked()
                    {
                        if let Err(e) = save_settings_to_disk(
                            &self.settings_api_key,
                            &self.settings_model,
                        ) {
                            self.status = format!("Save failed: {e}");
                        } else {
                            self.status = "Settings saved, restarting engine...".to_string();
                            self.show_settings = false;
                            self.restart_engine();
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_settings = false;
                    }
                });
            });
        if !open {
            self.show_settings = false;
        }
    }

    /// Render inline approval cards inside the chat stream (Qoder-style).
    /// Returns true if any approval was acted upon.
    fn render_inline_approvals(&mut self, ui: &mut egui::Ui) {
        if self.pending_approvals.is_empty() {
            return;
        }

        let mut to_remove = Vec::new();

        for (i, approval) in self.pending_approvals.iter().enumerate() {
            // Approval card in the chat stream
            egui::Frame::none()
                .fill(DeepSeekColors::SURFACE)
                .stroke(egui::Stroke::new(1.0, DeepSeekColors::WARNING))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("⚠")
                                .color(DeepSeekColors::WARNING)
                                .size(14.0),
                        );
                        ui.label(
                            egui::RichText::new(&approval.tool_name)
                                .color(DeepSeekColors::WARNING)
                                .size(13.0)
                                .strong(),
                        );
                    });

                    // Command preview in a code-style block
                    egui::Frame::none()
                        .fill(DeepSeekColors::CODE_BG)
                        .rounding(egui::Rounding::same(4.0))
                        .inner_margin(egui::Margin::same(8.0))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&approval.description)
                                    .color(DeepSeekColors::TEXT_PRIMARY)
                                    .size(12.0)
                                    .monospace(),
                            );
                        });
                    ui.add_space(8.0);

                    // Run / Cancel buttons
                    ui.horizontal(|ui| {
                        let run_btn = egui::Button::new(
                            egui::RichText::new(" ▶ Run ").color(Color32::WHITE).size(13.0).strong(),
                        )
                        .fill(DeepSeekColors::SUCCESS)
                        .rounding(egui::Rounding::same(4.0));

                        let cancel_btn = egui::Button::new(
                            egui::RichText::new(" ✗ Cancel ").color(Color32::WHITE).size(13.0).strong(),
                        )
                        .fill(DeepSeekColors::ERROR)
                        .rounding(egui::Rounding::same(4.0));

                        if ui.add(run_btn).clicked() {
                            to_remove.push((i, true));
                        }
                        if ui.add(cancel_btn).clicked() {
                            to_remove.push((i, false));
                        }
                    });
                });
            ui.add_space(6.0);
        }

        // Process approvals/denials in reverse order so indices remain valid
        for (i, approved) in to_remove.into_iter().rev() {
            let approval = self.pending_approvals.remove(i);
            if let Some(engine) = &self.engine {
                let engine = engine.clone();
                let id = approval.id;
                if approved {
                    tokio::spawn(async move {
                        let _ = engine.approve_tool_call(id).await;
                    });
                } else {
                    tokio::spawn(async move {
                        let _ = engine.deny_tool_call(id).await;
                    });
                }
            }
        }
    }

    /// Start an async file-save so the UI thread never blocks on I/O.
    fn save_current_file(&mut self) {
        if let Some(file) = self.open_files.get(self.active_file_index) {
            let path = file.path.clone();
            let content = file.content.clone();
            let tx = self.save_result_tx.clone();
            tokio::spawn(async move {
                match tokio::fs::write(&path, &content).await {
                    Ok(_) => {
                        let _ = tx.send(SaveResult {
                            path,
                            success: true,
                            error: None,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(SaveResult {
                            path,
                            success: false,
                            error: Some(e.to_string()),
                        });
                    }
                }
            });
        }
    }

    /// Drain async save results and update dirty flags / status.
    fn poll_save_results(&mut self) {
        while let Ok(result) = self.save_result_rx.try_recv() {
            if let Some(file) = self.open_files.iter_mut().find(|f| f.path == result.path) {
                if result.success {
                    file.dirty = false;
                    self.status = "File saved".to_string();
                } else {
                    self.status = format!(
                        "Save failed: {}",
                        result.error.unwrap_or_default()
                    );
                }
            }
        }
    }
}

impl eframe::App for GuiApp {
    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Re-apply theme every frame to prevent eframe persistence from overriding it.
        apply_theme(ctx);

        self.poll_engine_events();
        self.poll_file_tree_results();
        self.poll_save_results();

        // Settings window
        if self.show_settings {
            self.render_settings_window(ctx);
        }

        // Top panel — menu bar
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                // Left: quick menu with settings
                ui.menu_button("☰", |ui| {
                    ui.label(
                        egui::RichText::new("Session")
                            .color(DeepSeekColors::TEXT_PRIMARY)
                            .size(13.0)
                            .strong(),
                    );
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("Model: {}", self.config.default_model()))
                            .color(DeepSeekColors::TEXT_SECONDARY)
                            .size(13.0),
                    );
                    ui.label(
                        egui::RichText::new("Mode: Agent")
                            .color(DeepSeekColors::TEXT_SECONDARY)
                            .size(13.0),
                    );
                    ui.label(
                        egui::RichText::new(format!("Messages: {}", self.messages.len()))
                            .color(DeepSeekColors::TEXT_SECONDARY)
                            .size(13.0),
                    );
                    if self.is_streaming {
                        ui.label(
                            egui::RichText::new("● Streaming...")
                                .color(DeepSeekColors::ACCENT)
                                .size(13.0),
                        );
                    }
                    ui.separator();
                    ui.label(
                        egui::RichText::new("Shortcuts")
                            .color(DeepSeekColors::TEXT_PRIMARY)
                            .size(13.0)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("Enter — Send")
                            .color(DeepSeekColors::TEXT_SECONDARY)
                            .size(13.0),
                    );
                    ui.label(
                        egui::RichText::new("Shift+Enter — New line")
                            .color(DeepSeekColors::TEXT_SECONDARY)
                            .size(13.0),
                    );
                    ui.label(
                        egui::RichText::new("Ctrl+S — Save file")
                            .color(DeepSeekColors::TEXT_SECONDARY)
                            .size(13.0),
                    );
                    ui.separator();
                    if ui.button("设置").clicked() {
                        self.show_settings = true;
                        ui.close_menu();
                    }
                    if ui.button("退出").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("文件(F)", |ui| {
                    if ui.button("打开文件").clicked() {
                        ui.close_menu();
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            if let Some(idx) = self.open_files.iter().position(|f| f.path == path) {
                                self.active_file_index = idx;
                            } else {
                                match std::fs::read_to_string(&path) {
                                    Ok(content) => {
                                        const MAX_TABS: usize = 12;
                                        if self.open_files.len() >= MAX_TABS {
                                            // Close oldest clean tab to make room
                                            if let Some(idx) = self.open_files.iter().position(|f| !f.dirty) {
                                                self.open_files.remove(idx);
                                                if self.active_file_index >= idx && self.active_file_index > 0 {
                                                    self.active_file_index -= 1;
                                                }
                                            }
                                        }
                                        self.open_files.push(OpenFile {
                                            path,
                                            content,
                                            dirty: false,
                                            highlight_cache: None,
                                        });
                                        self.active_file_index = self.open_files.len() - 1;
                                    }
                                    Err(e) => {
                                        self.status = format!("Failed to read file: {e}");
                                    }
                                }
                            }
                        }
                    }
                    if ui.button("打开文件夹").clicked() {
                        ui.close_menu();
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.workspace_path = path.clone();
                            std::env::set_current_dir(&path).ok();
                            self.messages.push(ChatMessage::SystemError {
                                text: format!("Workspace changed to: {}", path.display()),
                            });
                            self.restart_engine();
                        }
                    }
                    ui.separator();
                    if ui.button("保存").clicked() {
                        self.save_current_file();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("新建会话").clicked() {
                        self.messages.clear();
                        self.should_scroll_to_bottom = true;
                        ui.close_menu();
                    }
                    if ui.button("清空历史").clicked() {
                        self.messages.clear();
                        ui.close_menu();
                    }
                });

                ui.menu_button("查看(V)", |ui| {
                    if ui.button("刷新文件树").clicked() {
                        ui.close_menu();
                    }
                });

                ui.menu_button("帮助(H)", |ui| {
                    if ui.button("帮助文档").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("提交功能建议").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("问题上报").clicked() {
                        ui.close_menu();
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙").clicked() {
                        self.show_settings = true;
                    }
                });
            });
            ui.separator();
        });

        // Left panel — file tree
        egui::SidePanel::left("file_tree")
            .resizable(true)
            .default_width(280.0)
            .min_width(180.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Explorer")
                        .color(DeepSeekColors::TEXT_PRIMARY)
                        .size(13.0)
                        .strong(),
                );
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let workspace = self.workspace_path.clone();
                    self.render_file_tree(ui, &workspace);
                });
            });

        // Right panel — chat + input
        egui::SidePanel::right("chat")
            .resizable(true)
            .default_width(400.0)
            .min_width(300.0)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // Chat header — title + model + status
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Chat")
                                .color(DeepSeekColors::TEXT_PRIMARY)
                                .size(13.0)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new(format!("  {}", self.settings_model))
                                .color(DeepSeekColors::TEXT_SECONDARY)
                                .size(11.0),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let status_color = if self.is_streaming {
                                DeepSeekColors::ACCENT
                            } else if self.status.starts_with("Error")
                                || self.status.starts_with("Config error")
                            {
                                DeepSeekColors::ERROR
                            } else {
                                DeepSeekColors::SUCCESS
                            };
                            ui.label(
                                egui::RichText::new(if self.is_streaming { "● Generating" } else { &self.status })
                                    .color(status_color)
                                    .size(11.0),
                            );
                        });
                    });
                    ui.separator();

                    // Chat messages
                    let chat_height = ui.available_height() - 100.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), chat_height.max(80.0)),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    for msg in &self.messages {
                                        render_message(ui, msg);
                                        ui.add_space(8.0);
                                    }
                                    if self.is_streaming
                                        || !self.current_assistant_text.is_empty()
                                        || !self.current_thinking.is_empty()
                                    {
                                        render_in_progress(
                                            ui,
                                            &self.current_assistant_text,
                                            &self.current_thinking,
                                        );
                                    }
                                    // Inline approval cards (Qoder-style)
                                    self.render_inline_approvals(ui);
                                    if self.should_scroll_to_bottom {
                                        ui.scroll_to_cursor(None);
                                        self.should_scroll_to_bottom = false;
                                    }
                                });
                        },
                    );

                    ui.separator();

                    // Input area with send button inline
                    egui::Frame::none()
                        .fill(DeepSeekColors::SURFACE)
                        .stroke(egui::Stroke::new(1.0, DeepSeekColors::BORDER))
                        .rounding(egui::Rounding::same(6.0))
                        .inner_margin(egui::Margin::same(8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let text_edit = egui::TextEdit::multiline(&mut self.input)
                                    .desired_rows(2)
                                    .hint_text("输入消息…")
                                    .return_key(egui::KeyboardShortcut::new(
                                        egui::Modifiers::NONE,
                                        egui::Key::Enter,
                                    ));

                                let response = ui.add_sized(
                                    egui::vec2(ui.available_width() - 40.0, 50.0),
                                    text_edit,
                                );

                                if response.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    self.send_message();
                                    response.request_focus();
                                }

                                ui.vertical_centered(|ui| {
                                    ui.add_space(8.0);
                                    let btn_color = if self.is_streaming {
                                        DeepSeekColors::TEXT_SECONDARY
                                    } else {
                                        DeepSeekColors::ACCENT
                                    };
                                    let btn_text_color = if self.is_streaming {
                                        DeepSeekColors::BORDER
                                    } else {
                                        Color32::WHITE
                                    };
                                    if ui
                                        .add_sized(
                                            egui::vec2(32.0, 32.0),
                                            egui::Button::new(
                                                egui::RichText::new("➤").color(btn_text_color).size(16.0)
                                            )
                                            .fill(btn_color)
                                            .rounding(egui::Rounding::same(6.0)),
                                        )
                                        .clicked()
                                    {
                                        self.send_message();
                                    }
                                });
                            });
                        });
                });
            });

        // Central panel — editor with tabs
        egui::CentralPanel::default().show(ctx, |ui| {
            // Tab bar
            if !self.open_files.is_empty() {
                let mut close_idx = None;
                ui.horizontal(|ui| {
                    let len = self.open_files.len();
                    for idx in 0..len {
                        let file = &self.open_files[idx];
                        let is_active = idx == self.active_file_index;
                        let name = file
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let label = if file.dirty {
                            format!("● {}", name)
                        } else {
                            name
                        };
                        let text_color = if is_active {
                            DeepSeekColors::TEXT_PRIMARY
                        } else {
                            DeepSeekColors::TEXT_SECONDARY
                        };
                        // Auto-width tab (Qoder-style) with spacing between tabs
                        let tab_response = ui.scope(|ui| {
                            ui.set_min_size(egui::vec2(0.0, 28.0));
                            let label_response = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&label).color(text_color).size(13.0),
                                )
                                .sense(egui::Sense::click()),
                            );
                            if label_response.clicked() {
                                self.active_file_index = idx;
                            }
                            label_response.context_menu(|ui| {
                                if ui.button("关闭其他").clicked() {
                                    self.pending_close_others = Some(idx);
                                    ui.close_menu();
                                }
                                if ui.button("关闭全部").clicked() {
                                    self.pending_close_all = true;
                                    ui.close_menu();
                                }
                            });
                            if ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new("×")
                                            .color(DeepSeekColors::TEXT_SECONDARY)
                                            .size(13.0),
                                    )
                                    .sense(egui::Sense::click()),
                                )
                                .clicked()
                            {
                                close_idx = Some(idx);
                            }
                        });
                        // Active tab bottom indicator (Qoder-style)
                        if is_active {
                            let rect = tab_response.response.rect;
                            let line_y = rect.max.y - 1.0;
                            ui.painter().hline(
                                rect.min.x..=rect.max.x,
                                line_y,
                                egui::Stroke::new(2.0, DeepSeekColors::ACCENT),
                            );
                        }
                        ui.add_space(8.0);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("💾").clicked() {
                            self.save_current_file();
                        }
                    });
                });
                if let Some(idx) = close_idx {
                    self.open_files.remove(idx);
                    if self.active_file_index >= self.open_files.len()
                        && !self.open_files.is_empty()
                    {
                        self.active_file_index = self.open_files.len() - 1;
                    }
                }
                ui.separator();
            }

            // Breadcrumb + Editor content
            if let Some(file) = self.open_files.get_mut(self.active_file_index) {
                let path_display = file.path.display().to_string();
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(path_display)
                            .color(DeepSeekColors::TEXT_SECONDARY)
                            .size(11.0),
                    );
                });
                ui.separator();
                let ext = file
                    .path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_owned();
                let syntax_set = self.syntax_set.clone();
                let theme = self.theme.clone();

                // Re-highlight only when content changed; skip highlight for large files
                const MAX_HIGHLIGHT_CHARS: usize = 20_000;
                let cache_valid = file
                    .highlight_cache
                    .as_ref()
                    .map(|(cached_text, _)| cached_text == &file.content)
                    .unwrap_or(false);
                if !cache_valid {
                    let job = if file.content.len() > MAX_HIGHLIGHT_CHARS {
                        plain_text_layout_job(&file.content)
                    } else {
                        highlight_code(&file.content, &ext, &syntax_set, &theme, 0.0)
                    };
                    file.highlight_cache = Some((file.content.clone(), job));
                }
                let cached_job = file.highlight_cache.as_ref().map(|(_, job)| job.clone());

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut layouter = |ui: &egui::Ui, _text: &str, _wrap_width: f32| {
                        let job = cached_job.clone().unwrap_or_else(|| {
                            if _text.len() > MAX_HIGHLIGHT_CHARS {
                                plain_text_layout_job(_text)
                            } else {
                                highlight_code(_text, &ext, &syntax_set, &theme, _wrap_width)
                            }
                        });
                        ui.fonts(|f| f.layout_job(job))
                    };
                    let text_edit = egui::TextEdit::multiline(&mut file.content)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(50)
                        .layouter(&mut layouter);
                    let response = ui.add(text_edit);
                    if response.changed() {
                        file.dirty = true;
                        file.highlight_cache = None; // invalidate cache on edit
                        self.pending_auto_save = true;
                    }
                });
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.4);
                    ui.label(
                        egui::RichText::new("Select a file from the Explorer to view and edit")
                            .color(DeepSeekColors::TEXT_SECONDARY)
                            .size(13.0),
                    );
                });
            }

            if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S)) {
                self.save_current_file();
            }
        });

        // Apply deferred tab-close actions after all UI is done rendering
        if self.pending_close_all {
            self.open_files.clear();
            self.active_file_index = 0;
            self.pending_close_all = false;
        }
        if let Some(idx) = self.pending_close_others.take() {
            if let Some(path) = self.open_files.get(idx).map(|f| f.path.clone()) {
                self.open_files.retain(|f| f.path == path);
                self.active_file_index = 0;
            }
        }
        if self.pending_auto_save {
            self.save_current_file();
            self.pending_auto_save = false;
        }
    }
}

fn render_message(ui: &mut egui::Ui, msg: &ChatMessage) {
    match msg {
        ChatMessage::User { text } => {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let _bubble = egui::Frame::none()
                    .fill(DeepSeekColors::USER_BUBBLE)
                    .stroke(egui::Stroke::new(1.0, DeepSeekColors::BORDER))
                    .rounding(egui::Rounding::same(4.0))
                    .inner_margin(egui::Margin::same(8.0))
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width() * 0.75);
                        ui.label(egui::RichText::new(text).color(DeepSeekColors::TEXT_PRIMARY).size(13.0));
                    });
            });
        }
        ChatMessage::Assistant { text, thinking } => {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                let _bubble = egui::Frame::none()
                    .fill(DeepSeekColors::ASSISTANT_BUBBLE)
                    .stroke(egui::Stroke::new(1.0, DeepSeekColors::BORDER))
                    .rounding(egui::Rounding::same(4.0))
                    .inner_margin(egui::Margin::same(8.0))
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width() * 0.75);
                        ui.vertical(|ui| {
                            if let Some(t) = thinking {
                                ui.collapsing(
                                    egui::RichText::new("Thinking").color(DeepSeekColors::THINKING).size(13.0),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(t)
                                                .color(DeepSeekColors::THINKING)
                                                .size(13.0),
                                        );
                                    },
                                );
                            }
                            ui.label(egui::RichText::new(text).color(DeepSeekColors::TEXT_PRIMARY).size(13.0));
                        });
                    });
            });
        }
        ChatMessage::ToolCall { name, input } => {
            egui::Frame::none()
                .fill(DeepSeekColors::SURFACE)
                .stroke(egui::Stroke::new(1.0, DeepSeekColors::BORDER))
                .rounding(egui::Rounding::same(4.0))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.collapsing(
                        egui::RichText::new(format!("Tool: {name}")).color(DeepSeekColors::WARNING).size(13.0),
                        |ui| {
                            ui.monospace(input);
                        },
                    );
                });
        }
        ChatMessage::ToolResult { name, output } => {
            egui::Frame::none()
                .fill(DeepSeekColors::SURFACE)
                .stroke(egui::Stroke::new(1.0, DeepSeekColors::BORDER))
                .rounding(egui::Rounding::same(4.0))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.collapsing(
                        egui::RichText::new(format!("Result: {name}")).color(DeepSeekColors::SUCCESS).size(13.0),
                        |ui| {
                            ui.monospace(output);
                        },
                    );
                });
        }
        ChatMessage::SystemError { text } => {
            egui::Frame::none()
                .fill(DeepSeekColors::ERROR.linear_multiply(0.12))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::same(10.0))
                .stroke(egui::Stroke::new(1.0, DeepSeekColors::ERROR.linear_multiply(0.4)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("⚠")
                                .color(DeepSeekColors::ERROR)
                                .size(14.0),
                        );
                        ui.label(
                            egui::RichText::new(text)
                                .color(DeepSeekColors::ERROR)
                                .size(13.0),
                        );
                    });
                });
        }
    }
}

fn render_in_progress(ui: &mut egui::Ui, text: &str, thinking: &str) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
        egui::Frame::none()
            .fill(DeepSeekColors::ASSISTANT_BUBBLE)
            .stroke(egui::Stroke::new(1.0, DeepSeekColors::BORDER))
            .rounding(egui::Rounding::same(4.0))
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width() * 0.75);
                ui.vertical(|ui| {
                    if !thinking.is_empty() {
                        ui.label(
                            egui::RichText::new(thinking)
                                .color(DeepSeekColors::THINKING)
                                .size(13.0),
                        );
                    }
                    if !text.is_empty() {
                        ui.label(egui::RichText::new(text).color(DeepSeekColors::TEXT_PRIMARY));
                    } else {
                        ui.label(
                            egui::RichText::new("● ● ●")
                                .color(DeepSeekColors::TEXT_SECONDARY),
                        );
                    }
                });
            });
    });
}

/// Load configuration from the standard location.
fn load_config() -> anyhow::Result<Config> {
    // TODO: support CLI overrides and project-level config merging
    let config = Config::load(None, None)?;
    Ok(config)
}

/// Initialize the engine and wire up the event forwarding task.
fn init_engine(
    config: &Config,
    event_tx: Sender<EngineEvent>,
) -> anyhow::Result<EngineHandle> {
    let mut engine_config = EngineConfig::default();
    engine_config.model = config.default_model();
    engine_config.workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    engine_config.allow_shell = true;
    engine_config.trust_mode = false;
    engine_config.notes_path = config.notes_path();
    engine_config.mcp_config_path = config.mcp_config_path();
    engine_config.skills_dir = config.skills_dir();
    engine_config.instructions = config.instructions_paths();
    engine_config.max_subagents = config.max_subagents();
    engine_config.features = config.features();
    engine_config.memory_enabled = config.memory_enabled();
    engine_config.memory_path = config.memory_path();

    let handle = spawn_engine(engine_config, config);

    // Spawn a background task that forwards engine events to the GUI thread.
    // Uses recv().await instead of try_recv()+sleep to avoid polling latency.
    let engine_clone = handle.clone();
    tokio::spawn(async move {
        loop {
            let evt = {
                let mut rx = engine_clone.rx_event.write().await;
                rx.recv().await
            };
            match evt {
                Some(evt) => {
                    if event_tx.send(evt).is_err() {
                        break; // GUI closed
                    }
                }
                None => break, // Engine sender dropped
            }
        }
    });

    Ok(handle)
}

/// Save API key and default model to the user config file (~/.deepseek/config.toml).
fn save_settings_to_disk(api_key: &str, model: &str) -> anyhow::Result<()> {
    // Save API key via the existing helper (creates file if missing).
    let _ = save_api_key(api_key)?;

    // Now update the model in the same file.
    let config_path = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Home directory not found"))?
        .join(".deepseek")
        .join("config.toml");

    let content = if config_path.exists() {
        let existing = std::fs::read_to_string(&config_path)?;
        update_or_insert_line(&existing, "default_text_model", model)
    } else {
        format!(
            r#"api_key = "{api_key}"
default_text_model = "{model}"
"#
        )
    };

    std::fs::write(&config_path, content)?;
    Ok(())
}

/// Replace an existing `key = "..."` line or append it if missing.
fn update_or_insert_line(content: &str, key: &str, value: &str) -> String {
    let prefix = format!("{key} = ");
    let new_line = format!("{key} = \"{value}\"");
    let mut found = false;
    let mut result = String::new();

    for line in content.lines() {
        if line.trim_start().starts_with(&prefix) {
            result.push_str(&new_line);
            found = true;
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    if !found {
        result.push_str(&new_line);
        result.push('\n');
    }

    result
}

/// Load directory entries off the main thread (blocking I/O).
fn load_dir_entries(path: &std::path::Path) -> Vec<FileTreeEntry> {
    let skip_dirs: std::collections::HashSet<&str> =
        ["target", "node_modules", ".git", "dist", "build"]
            .iter()
            .copied()
            .collect();

    let mut entries = match std::fs::read_dir(path) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if skip_dirs.contains(name.as_str()) {
                    return None;
                }
                let path = e.path();
                let is_dir = path.is_dir();
                Some(FileTreeEntry { path, is_dir })
            })
            .collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };

    entries.sort_by(|a, b| {
        match b.is_dir.cmp(&a.is_dir) {
            std::cmp::Ordering::Equal => {
                a.path.file_name().cmp(&b.path.file_name())
            }
            other => other,
        }
    });
    entries.truncate(200);
    entries
}

impl GuiApp {
    fn render_file_tree(&mut self, ui: &mut egui::Ui, path: &std::path::Path) {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path.is_dir() {
            // Skip build artifact directories to avoid rendering thousands of files
            let skip_dirs = ["target", "node_modules", ".git", "dist", "build"];
            if skip_dirs.contains(&name.as_ref()) {
                return;
            }
            let id = ui.make_persistent_id(path);
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                false, // default collapsed for performance
            );
            let is_open = state.is_open();
            let arrow = if is_open { "▾" } else { "▸" };
            let header_text = format!("{} {}", arrow, name);
            let header_response = ui
                .horizontal(|ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(header_text)
                                .color(DeepSeekColors::TEXT_PRIMARY)
                                .size(13.0),
                        )
                        .sense(egui::Sense::click()),
                    )
                })
                .inner;
            if header_response.clicked() {
                state.toggle(ui);
            }

            // Trigger async load if opening for the first time
            if is_open
                && !self.file_tree_cache.contains_key(path)
                && !self.file_tree_pending.contains(path)
            {
                self.file_tree_pending.insert(path.to_path_buf());
                let path = path.to_path_buf();
                let tx = self.file_tree_tx.clone();
                std::thread::spawn(move || {
                    let entries = load_dir_entries(&path);
                    let _ = tx.send((path, entries));
                });
            }

            let cached = self.file_tree_cache.get(path).cloned();
            let pending = self.file_tree_pending.contains(path);
            state.show_body_indented(&header_response, ui, |ui| {
                if let Some(entries) = cached {
                    for entry in &entries {
                        self.render_file_tree(ui, &entry.path);
                    }
                } else if pending {
                    ui.label(
                        egui::RichText::new("…")
                            .color(DeepSeekColors::TEXT_SECONDARY)
                            .size(12.0),
                    );
                }
            });
        } else {
            let is_open = self.open_files.iter().position(|f| f.path == path);
            let is_active =
                is_open.map(|idx| idx == self.active_file_index).unwrap_or(false);
            let text_color = if is_active {
                DeepSeekColors::TEXT_PRIMARY
            } else {
                file_type_color(path)
            };
            let icon = file_type_icon(path);
            let display = format!("{} {}", icon, name);
            let desired_size = egui::vec2(ui.available_width(), 18.0);
            let (rect, response) = ui.allocate_at_least(desired_size, egui::Sense::click());
            if is_active {
                ui.painter().rect_filled(rect, 0.0, DeepSeekColors::SURFACE_HOVER);
            }
            let text_pos = rect.min + egui::vec2(4.0, rect.height() / 2.0);
            ui.painter().text(
                text_pos,
                egui::Align2::LEFT_CENTER,
                display,
                egui::FontId::new(13.0, egui::FontFamily::Proportional),
                text_color,
            );
            if response.clicked() {
                if let Some(idx) = is_open {
                    self.active_file_index = idx;
                } else if let Ok(content) = std::fs::read_to_string(path) {
                    const MAX_TABS: usize = 12;
                    if self.open_files.len() >= MAX_TABS {
                        if let Some(idx) = self.open_files.iter().position(|f| !f.dirty) {
                            self.open_files.remove(idx);
                            if self.active_file_index >= idx && self.active_file_index > 0 {
                                self.active_file_index -= 1;
                            }
                        }
                    }
                    self.open_files.push(OpenFile {
                        path: path.to_path_buf(),
                        content,
                        dirty: false,
                        highlight_cache: None,
                    });
                    self.active_file_index = self.open_files.len() - 1;
                }
            }
        }
    }
}

fn file_type_icon(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "",
        Some("toml") => "",
        Some("md") => "",
        Some("json") => "",
        Some("yaml") | Some("yml") => "",
        Some("py") => "",
        Some("js") => "",
        Some("ts") => "",
        Some("html") => "",
        Some("css") => "",
        Some("dockerfile") => "",
        Some("sh") | Some("ps1") | Some("bat") => "",
        Some("lock") => "",
        _ => "",
    }
}

fn file_type_color(path: &std::path::Path) -> egui::Color32 {
    use egui::Color32;
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => Color32::from_rgb(222, 165, 132),   // Rust orange
        Some("toml") | Some("lock") => Color32::from_rgb(156, 163, 175), // Gray
        Some("md") => Color32::from_rgb(96, 165, 250),    // Blue
        Some("json") | Some("yaml") | Some("yml") => Color32::from_rgb(250, 204, 21), // Yellow
        Some("py") => Color32::from_rgb(96, 165, 250),    // Blue
        Some("js") => Color32::from_rgb(250, 240, 137),   // JS yellow
        Some("ts") => Color32::from_rgb(49, 120, 198),    // TS blue
        Some("html") => Color32::from_rgb(227, 76, 38),   // HTML orange
        Some("css") => Color32::from_rgb(21, 114, 182),   // CSS blue
        Some("dockerfile") => Color32::from_rgb(13, 98, 180), // Docker blue
        Some("sh") | Some("ps1") | Some("bat") => Color32::from_rgb(137, 224, 81), // Shell green
        _ => Color32::from_rgb(156, 163, 175),            // Default gray
    }
}

/// Highlight code with syntect and produce an egui LayoutJob.
fn highlight_code(
    text: &str,
    ext: &str,
    syntax_set: &syntect::parsing::SyntaxSet,
    theme: &syntect::highlighting::Theme,
    _wrap_width: f32,
) -> egui::text::LayoutJob {
    use syntect::easy::HighlightLines;
    use syntect::util::LinesWithEndings;

    let syntax = syntax_set
        .find_syntax_by_extension(ext)
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let mut h = HighlightLines::new(syntax, theme);

    let mut job = egui::text::LayoutJob::default();
    for line in LinesWithEndings::from(text) {
        match h.highlight_line(line, syntax_set) {
            Ok(regions) => {
                for (style, text_slice) in regions {
                    let fg = style.foreground;
                    job.append(
                        text_slice,
                        0.0,
                        egui::text::TextFormat {
                            font_id: egui::FontId::monospace(13.0),
                            color: egui::Color32::from_rgb(fg.r, fg.g, fg.b),
                            ..Default::default()
                        },
                    );
                }
            }
            Err(_) => {
                job.append(
                    line,
                    0.0,
                    egui::text::TextFormat {
                        font_id: egui::FontId::monospace(13.0),
                        color: egui::Color32::LIGHT_GRAY,
                        ..Default::default()
                    },
                );
            }
        }
    }
    job
}

/// Convert raw API error messages into user-friendly Chinese text.
fn friendly_error_message(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.contains("insufficient balance") || lower.contains("402") {
        return "API 余额不足，请前往 DeepSeek 控制台充值后重试。".to_string();
    }
    if lower.contains("invalid api key") || lower.contains("unauthorized") || lower.contains("401") {
        return "API Key 无效或已过期，请在设置中重新配置。".to_string();
    }
    if lower.contains("rate limit") || lower.contains("429") {
        return "请求过于频繁，请稍后重试。".to_string();
    }
    if lower.contains("403") {
        return "无权访问该资源，请检查 API Key 权限。".to_string();
    }
    if lower.contains("timeout") || lower.contains("timed out") {
        return "请求超时，请检查网络连接后重试。".to_string();
    }
    if lower.contains("connection") || lower.contains("network") {
        return "网络连接失败，请检查网络设置。".to_string();
    }
    // Fallback: show original but strip JSON noise
    raw.to_string()
}

/// Fast plain-text LayoutJob for large files where syntax highlighting is skipped.
fn plain_text_layout_job(text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::text::TextFormat {
            font_id: egui::FontId::monospace(13.0),
            color: egui::Color32::LIGHT_GRAY,
            ..Default::default()
        },
    );
    job
}