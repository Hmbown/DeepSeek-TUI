import * as vscode from "vscode";
import { Mode, Message, ToolCall, StreamEvent, WebviewToHost, HostToWebview } from "./types";

export class ChatPanel implements vscode.WebviewViewProvider {
  public static readonly viewType = "deepseek.chatView";
  private _view?: vscode.WebviewView;
  private _extensionUri: vscode.Uri;
  private _currentMode: Mode = "agent";

  constructor(private readonly _context: vscode.ExtensionContext) {
    this._extensionUri = _context.extensionUri;
  }

  /**
   * Called by VS Code when the webview view is resolved.
   */
  resolveWebviewView(
    webviewView: vscode.WebviewView,
    _context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken
  ): void {
    this._view = webviewView;

    webviewView.webview.options = {
      enableScripts: true,
      localResourceRoots: [this._extensionUri],
    };

    webviewView.webview.html = this._getHtmlForWebview(webviewView.webview);

    // Handle messages from the webview.
    webviewView.webview.onDidReceiveMessage(
      (message: WebviewToHost) => {
        switch (message.type) {
          case "send":
            this._handleSend(message.text, message.mode);
            break;
          case "modeChange":
            this._currentMode = message.mode;
            break;
          case "ready":
            // Webview is ready; push current mode.
            this.postMessage({ type: "setMode", mode: this._currentMode });
            break;
        }
      },
      undefined,
      this._context.subscriptions
    );
  }

  /**
   * Post a message to the webview.
   */
  postMessage(message: HostToWebview): void {
    this._view?.webview.postMessage(message);
  }

  /**
   * Handle a send request from the webview.
   * Forwards to the app-server SSE endpoint and streams tokens back.
   */
  private async _handleSend(text: string, mode: Mode): Promise<void> {
    const port = vscode.workspace
      .getConfiguration("deepseek")
      .get<number>("serverPort", 8787);

    // Add user message to transcript immediately.
    const userMessage: Message = {
      id: `msg-${Date.now()}-user`,
      role: "user",
      content: text,
      timestamp: new Date().toISOString(),
    };
    this.postMessage({ type: "addMessage", message: userMessage });

    // Create a placeholder assistant message.
    const assistantId = `msg-${Date.now()}-assistant`;
    const assistantMessage: Message = {
      id: assistantId,
      role: "assistant",
      content: "",
      timestamp: new Date().toISOString(),
      toolCalls: [],
    };
    this.postMessage({ type: "addMessage", message: assistantMessage });

    let accumulatedContent = "";
    const toolCalls: ToolCall[] = [];

    try {
      const response = await fetch(`http://127.0.0.1:${port}/stream`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ input: text, mode }),
      });

      if (!response.ok) {
        throw new Error(`Server returned ${response.status}: ${response.statusText}`);
      }

      const reader = response.body?.getReader();
      if (!reader) {
        throw new Error("Response body is not readable");
      }

      const decoder = new TextDecoder("utf-8");
      let buffer = "";

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });

        // Parse SSE frames: split on double newline.
        const frames = buffer.split("\n\n");
        // The last element may be incomplete — keep it in the buffer.
        buffer = frames.pop() || "";

        for (const frame of frames) {
          if (!frame.trim()) continue;
          const event = this._parseSSEFrame(frame);
          if (!event) continue;

          switch (event.event) {
            case "response_start":
              // Model info — could display in UI.
              break;

            case "response_delta":
              if (event.text) {
                accumulatedContent += event.text;
                this.postMessage({
                  type: "updateMessage",
                  id: assistantId,
                  content: accumulatedContent,
                });
              }
              break;

            case "tool_call_start": {
              const tc: ToolCall = {
                name: event.tool_name || "unknown",
                arguments: event.tool_arguments || "{}",
                complete: false,
              };
              toolCalls.push(tc);
              // Send tool call update.
              this.postMessage({
                type: "updateToolCall",
                messageId: assistantId,
                toolCall: { ...tc },
              });
              break;
            }

            case "tool_call_result": {
              const last = toolCalls[toolCalls.length - 1];
              if (last) {
                last.result = event.tool_result || "";
                last.complete = true;
                this.postMessage({
                  type: "updateToolCall",
                  messageId: assistantId,
                  toolCall: { ...last },
                });
              }
              break;
            }

            case "response_end":
              // Stream complete.
              break;
          }
        }
      }
    } catch (err) {
      const errorContent = `**Error**: ${err instanceof Error ? err.message : String(err)}`;
      if (accumulatedContent) {
        accumulatedContent += `\n\n${errorContent}`;
      } else {
        accumulatedContent = errorContent;
      }
      this.postMessage({
        type: "updateMessage",
        id: assistantId,
        content: accumulatedContent,
      });
    }
  }

  /**
   * Parse a single SSE frame (event + data lines) into a StreamEvent.
   */
  private _parseSSEFrame(frame: string): StreamEvent | null {
    let eventType = "";
    let data = "";

    for (const line of frame.split("\n")) {
      if (line.startsWith("event: ")) {
        eventType = line.slice(7).trim();
      } else if (line.startsWith("data: ")) {
        data = line.slice(6).trim();
      }
    }

    if (!data) return null;

    try {
      const parsed = JSON.parse(data);
      return {
        event: eventType || parsed.event || "response_delta",
        text: parsed.text,
        tool_name: parsed.tool_name,
        tool_arguments: parsed.tool_arguments,
        tool_result: parsed.tool_result,
        model: parsed.model,
      };
    } catch {
      // If data is not JSON, treat as raw text delta.
      return {
        event: "response_delta",
        text: data,
      };
    }
  }

  /**
   * Build the full HTML for the webview.
   */
  private _getHtmlForWebview(webview: vscode.Webview): string {
    const port = vscode.workspace
      .getConfiguration("deepseek")
      .get<number>("serverPort", 8787);

    const styleResetUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this._extensionUri, "media", "reset.css")
    );

    return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta
    http-equiv="Content-Security-Policy"
    content="default-src 'none'; style-src ${webview.cspSource} 'unsafe-inline'; script-src 'unsafe-inline'; connect-src http://127.0.0.1:${port} http://localhost:${port}; font-src ${webview.cspSource};"
  />
  <title>DeepSeek Chat</title>
  <style>
    :root {
      --bg: var(--vscode-editor-background, #1e1e1e);
      --fg: var(--vscode-editor-foreground, #d4d4d4);
      --border: var(--vscode-panel-border, #3c3c3c);
      --accent: var(--vscode-focusBorder, #007acc);
      --user-bg: var(--vscode-textBlockQuote-background, #2a2a2a);
      --assistant-bg: transparent;
      --tool-bg: var(--vscode-textCodeBlock-background, #1e1e1e);
      --tool-border: #555;
      --button-bg: var(--vscode-button-secondaryBackground, #3a3d41);
      --button-fg: var(--vscode-button-secondaryForeground, #ccc);
      --button-active-bg: var(--vscode-button-background, #007acc);
      --button-active-fg: var(--vscode-button-foreground, #fff);
      --radius: 6px;
      --font-mono: var(--vscode-editor-font-family, 'Cascadia Code', 'JetBrains Mono', monospace);
      --font-ui: var(--vscode-font-family, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif);
    }

    * { box-sizing: border-box; margin: 0; padding: 0; }

    body {
      background: var(--bg);
      color: var(--fg);
      font-family: var(--font-ui);
      font-size: 13px;
      line-height: 1.5;
      display: flex;
      flex-direction: column;
      height: 100vh;
      overflow: hidden;
    }

    /* ── Mode bar ── */
    #mode-bar {
      display: flex;
      gap: 4px;
      padding: 8px 12px;
      border-bottom: 1px solid var(--border);
      flex-shrink: 0;
    }

    #mode-bar button {
      background: var(--button-bg);
      color: var(--button-fg);
      border: none;
      border-radius: 4px;
      padding: 4px 14px;
      font-size: 12px;
      font-family: var(--font-ui);
      cursor: pointer;
      transition: background 0.15s;
    }

    #mode-bar button:hover {
      background: var(--vscode-toolbar-hoverBackground, #444);
    }

    #mode-bar button.active {
      background: var(--button-active-bg);
      color: var(--button-active-fg);
    }

    /* ── Transcript ── */
    #transcript {
      flex: 1;
      overflow-y: auto;
      padding: 12px;
      display: flex;
      flex-direction: column;
      gap: 16px;
    }

    .message {
      display: flex;
      flex-direction: column;
      max-width: 100%;
    }

    .message-user {
      align-self: flex-end;
      background: var(--user-bg);
      border-radius: var(--radius) var(--radius) 2px var(--radius);
      padding: 8px 12px;
      max-width: 85%;
    }

    .message-assistant {
      align-self: flex-start;
      max-width: 100%;
    }

    .message-role {
      font-size: 11px;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 0.5px;
      color: var(--vscode-descriptionForeground, #888);
      margin-bottom: 4px;
    }

    .message-content {
      white-space: pre-wrap;
      word-break: break-word;
    }

    .message-content p { margin-bottom: 8px; }
    .message-content code {
      font-family: var(--font-mono);
      background: var(--tool-bg);
      padding: 1px 4px;
      border-radius: 3px;
      font-size: 12px;
    }
    .message-content pre {
      background: var(--tool-bg);
      padding: 8px 12px;
      border-radius: var(--radius);
      overflow-x: auto;
      font-size: 12px;
      margin: 8px 0;
    }
    .message-content pre code {
      background: none;
      padding: 0;
    }

    /* ── Tool calls ── */
    .tool-calls {
      margin-top: 8px;
      display: flex;
      flex-direction: column;
      gap: 6px;
    }

    .tool-call {
      border: 1px solid var(--tool-border);
      border-radius: var(--radius);
      background: var(--tool-bg);
      overflow: hidden;
    }

    .tool-call-header {
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 4px 10px;
      cursor: pointer;
      font-size: 12px;
      font-family: var(--font-mono);
      user-select: none;
    }

    .tool-call-header:hover {
      background: var(--vscode-list-hoverBackground, #2a2d2e);
    }

    .tool-call-chevron {
      font-size: 10px;
      transition: transform 0.15s;
      display: inline-block;
      width: 12px;
    }

    .tool-call.expanded .tool-call-chevron {
      transform: rotate(90deg);
    }

    .tool-call-name {
      font-weight: 600;
      color: var(--vscode-debugIcon-startForeground, #89d185);
    }

    .tool-call-status {
      font-size: 11px;
      margin-left: auto;
      color: var(--vscode-descriptionForeground, #888);
    }

    .tool-call-status.done {
      color: var(--vscode-testing-iconPassed, #73c991);
    }

    .tool-call-body {
      display: none;
      padding: 8px 10px;
      border-top: 1px solid var(--tool-border);
      font-size: 12px;
      font-family: var(--font-mono);
      max-height: 200px;
      overflow-y: auto;
    }

    .tool-call.expanded .tool-call-body {
      display: block;
    }

    .tool-call-section {
      margin-bottom: 6px;
    }

    .tool-call-section-label {
      font-size: 10px;
      text-transform: uppercase;
      color: var(--vscode-descriptionForeground, #888);
      margin-bottom: 2px;
    }

    /* ── Composer ── */
    #composer-bar {
      flex-shrink: 0;
      padding: 8px 12px;
      border-top: 1px solid var(--border);
      display: flex;
      gap: 8px;
      align-items: flex-end;
    }

    #composer {
      flex: 1;
      background: var(--vscode-input-background, #3c3c3c);
      color: var(--vscode-input-foreground, #ccc);
      border: 1px solid var(--border);
      border-radius: var(--radius);
      padding: 8px 10px;
      font-family: var(--font-ui);
      font-size: 13px;
      resize: none;
      min-height: 36px;
      max-height: 120px;
      outline: none;
      line-height: 1.4;
    }

    #composer:focus {
      border-color: var(--accent);
    }

    #send-btn {
      background: var(--button-active-bg);
      color: var(--button-active-fg);
      border: none;
      border-radius: var(--radius);
      padding: 8px 16px;
      font-family: var(--font-ui);
      font-size: 13px;
      cursor: pointer;
      white-space: nowrap;
      flex-shrink: 0;
    }

    #send-btn:hover {
      opacity: 0.9;
    }

    #send-btn:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }

    /* ── Streaming cursor ── */
    .streaming-cursor::after {
      content: "▊";
      animation: blink 1s step-end infinite;
    }

    @keyframes blink {
      50% { opacity: 0; }
    }

    /* ── Scrollbar ── */
    ::-webkit-scrollbar {
      width: 6px;
    }
    ::-webkit-scrollbar-track {
      background: transparent;
    }
    ::-webkit-scrollbar-thumb {
      background: var(--vscode-scrollbarSlider-background, #424242);
      border-radius: 3px;
    }
  </style>
</head>
<body>
  <!-- Mode bar -->
  <div id="mode-bar">
    <button data-mode="plan">Plan</button>
    <button data-mode="agent" class="active">Agent</button>
    <button data-mode="yolo">YOLO</button>
  </div>

  <!-- Transcript -->
  <div id="transcript"></div>

  <!-- Composer -->
  <div id="composer-bar">
    <textarea id="composer" rows="1" placeholder="Type a message... (Ctrl+Enter to send)"></textarea>
    <button id="send-btn">Send</button>
  </div>

  <script>
    (function () {
      const vscode = acquireVsCodeApi();
      const PORT = ${port};
      let currentMode = "agent";
      let streamingMessageId = null;
      let isStreaming = false;

      // ── DOM refs ──
      const transcript = document.getElementById("transcript");
      const composer = document.getElementById("composer");
      const sendBtn = document.getElementById("send-btn");
      const modeButtons = document.querySelectorAll("#mode-bar button");

      // ── Mode switching ──
      modeButtons.forEach((btn) => {
        btn.addEventListener("click", () => {
          const mode = btn.dataset.mode;
          if (mode === currentMode) return;
          currentMode = mode;
          modeButtons.forEach((b) => b.classList.remove("active"));
          btn.classList.add("active");
          vscode.postMessage({ type: "modeChange", mode });
        });
      });

      // ── Helpers ──
      function scrollToBottom() {
        transcript.scrollTop = transcript.scrollHeight;
      }

      function escapeHtml(text) {
        const div = document.createElement("div");
        div.textContent = text;
        return div.innerHTML;
      }

      /**
       * Very light Markdown → HTML conversion.
       * Handles code blocks (triple backtick), inline code, bold, italics.
       */
      function renderMarkdown(text) {
        let out = "";
        const lines = text.split("\\n");
        let inCodeBlock = false;
        let codeLang = "";
        let codeBuffer = "";

        for (let i = 0; i < lines.length; i++) {
          const line = lines[i];

          if (!inCodeBlock && line.startsWith("\`\`\`")) {
            inCodeBlock = true;
            codeLang = line.slice(3).trim();
            codeBuffer = "";
            continue;
          }

          if (inCodeBlock) {
            if (line.startsWith("\`\`\`")) {
              inCodeBlock = false;
              out += '<pre><code>' + escapeHtml(codeBuffer) + '</code></pre>';
              continue;
            }
            codeBuffer += (codeBuffer ? "\\n" : "") + line;
            continue;
          }

          // Inline formatting
          let processed = escapeHtml(line);

          // Bold: **text**
          processed = processed.replace(/\\*\\*(.+?)\\*\\*/g, '<strong>$1</strong>');
          // Italic: *text*
          processed = processed.replace(/(?<!\\*)\\*(?!\\*)(.+?)(?<!\\*)\\*(?!\\*)/g, '<em>$1</em>');
          // Inline code: \`text\`
          processed = processed.replace(/\`([^\`]+)\`/g, '<code>$1</code>');

          if (processed === "" && i > 0 && lines[i - 1] === "") {
            continue; // skip double blank lines
          }

          out += (out ? "\\n" : "") + (processed || "<br>");
        }

        // If the stream ends mid-code-block, render it anyway.
        if (inCodeBlock && codeBuffer) {
          out += '<pre><code>' + escapeHtml(codeBuffer) + '</code></pre>';
        }

        // Wrap non-empty lines in <p> blocks.
        const paras = out.split("\\n\\n").filter(Boolean);
        return paras.map((p) => {
          if (p.startsWith("<pre>")) return p;
          return "<p>" + p.replace(/\\n/g, "<br>") + "</p>";
        }).join("\\n");
      }

      function findMessageEl(id) {
        return document.querySelector('[data-message-id="' + id + '"]');
      }

      // ── Message rendering ──
      function addMessage(message) {
        const wrapper = document.createElement("div");
        wrapper.className = "message message-" + message.role;
        wrapper.dataset.messageId = message.id;

        const roleLabel = document.createElement("div");
        roleLabel.className = "message-role";
        roleLabel.textContent = message.role === "user" ? "You" : "DeepSeek";

        const content = document.createElement("div");
        content.className = "message-content";
        content.innerHTML = renderMarkdown(message.content);

        wrapper.appendChild(roleLabel);
        wrapper.appendChild(content);

        // Tool calls container
        if (message.role === "assistant") {
          const tcContainer = document.createElement("div");
          tcContainer.className = "tool-calls";
          tcContainer.dataset.toolCalls = "true";
          wrapper.appendChild(tcContainer);
        }

        transcript.appendChild(wrapper);
        scrollToBottom();
      }

      function updateMessage(id, content) {
        const el = findMessageEl(id);
        if (!el) return;
        const contentEl = el.querySelector(".message-content");
        if (!contentEl) return;
        contentEl.innerHTML = renderMarkdown(content);
        scrollToBottom();
      }

      function getOrCreateToolEl(wrapper, toolCall) {
        const tcContainer = wrapper.querySelector(".tool-calls");
        if (!tcContainer) return null;

        // Find existing tool call by name
        const existing = tcContainer.querySelector(
          '.tool-call[data-tool-name="' + toolCall.name + '"]'
        );
        if (existing) return existing;

        const tcDiv = document.createElement("div");
        tcDiv.className = "tool-call expanded";
        tcDiv.dataset.toolName = toolCall.name;

        const header = document.createElement("div");
        header.className = "tool-call-header";

        const chevron = document.createElement("span");
        chevron.className = "tool-call-chevron";
        chevron.textContent = "▶";

        const nameSpan = document.createElement("span");
        nameSpan.className = "tool-call-name";
        nameSpan.textContent = toolCall.name;

        const statusSpan = document.createElement("span");
        statusSpan.className = "tool-call-status";
        statusSpan.textContent = "running…";

        header.appendChild(chevron);
        header.appendChild(nameSpan);
        header.appendChild(statusSpan);

        header.addEventListener("click", () => {
          tcDiv.classList.toggle("expanded");
        });

        const body = document.createElement("div");
        body.className = "tool-call-body";

        // Args section
        if (toolCall.arguments) {
          const argsSection = document.createElement("div");
          argsSection.className = "tool-call-section";
          const argsLabel = document.createElement("div");
          argsLabel.className = "tool-call-section-label";
          argsLabel.textContent = "Arguments";
          const argsPre = document.createElement("pre");
          let argsText = toolCall.arguments;
          try {
            argsText = JSON.stringify(JSON.parse(argsText), null, 2);
          } catch (_) { /* raw string */ }
          argsPre.textContent = argsText;
          argsSection.appendChild(argsLabel);
          argsSection.appendChild(argsPre);
          body.appendChild(argsSection);
        }

        // Result section (placeholder)
        const resultSection = document.createElement("div");
        resultSection.className = "tool-call-section result-section";
        const resultLabel = document.createElement("div");
        resultLabel.className = "tool-call-section-label";
        resultLabel.textContent = "Result";
        resultSection.appendChild(resultLabel);
        body.appendChild(resultSection);

        tcDiv.appendChild(header);
        tcDiv.appendChild(body);
        tcContainer.appendChild(tcDiv);

        return tcDiv;
      }

      function updateToolCall(messageId, toolCall) {
        const wrapper = findMessageEl(messageId);
        if (!wrapper) return;

        const tcDiv = getOrCreateToolEl(wrapper, toolCall);
        if (!tcDiv) return;

        const statusEl = tcDiv.querySelector(".tool-call-status");
        if (statusEl) {
          statusEl.textContent = toolCall.complete ? "done" : "running…";
          statusEl.className = "tool-call-status" + (toolCall.complete ? " done" : "");
        }

        if (toolCall.result !== undefined) {
          const resultSection = tcDiv.querySelector(".result-section");
          if (resultSection) {
            // Remove old result pre if any
            const oldPre = resultSection.querySelector("pre");
            if (oldPre) oldPre.remove();

            const pre = document.createElement("pre");
            let resultText = toolCall.result;
            try {
              resultText = JSON.stringify(JSON.parse(resultText), null, 2);
            } catch (_) { /* raw string */ }
            pre.textContent = resultText;
            resultSection.appendChild(pre);
          }
        }
      }

      // ── Send ──
      async function doSend() {
        const text = composer.value.trim();
        if (!text || isStreaming) return;

        composer.value = "";
        composer.style.height = "auto";
        isStreaming = true;
        sendBtn.disabled = true;

        // Add user message locally
        const userMsg = {
          id: "msg-" + Date.now() + "-user",
          role: "user",
          content: text,
          timestamp: new Date().toISOString(),
        };
        addMessage(userMsg);

        // Create placeholder assistant message
        const assistantId = "msg-" + Date.now() + "-assistant";
        streamingMessageId = assistantId;
        const assistantMsg = {
          id: assistantId,
          role: "assistant",
          content: "",
          timestamp: new Date().toISOString(),
        };
        addMessage(assistantMsg);

        // Mark content as streaming
        const assistantEl = findMessageEl(assistantId);
        if (assistantEl) {
          const contentEl = assistantEl.querySelector(".message-content");
          if (contentEl) contentEl.classList.add("streaming-cursor");
        }

        try {
          const response = await fetch("http://127.0.0.1:" + PORT + "/stream", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ input: text, mode: currentMode }),
          });

          if (!response.ok) {
            throw new Error("Server returned " + response.status + ": " + response.statusText);
          }

          const reader = response.body.getReader();
          if (!reader) throw new Error("Response body is not readable");

          const decoder = new TextDecoder("utf-8");
          let buffer = "";
          let accumulated = "";

          while (true) {
            const { done, value } = await reader.read();
            if (done) break;

            buffer += decoder.decode(value, { stream: true });
            const frames = buffer.split("\\n\\n");
            buffer = frames.pop() || "";

            for (const frame of frames) {
              if (!frame.trim()) continue;
              const event = parseSSEFrame(frame);
              if (!event) continue;

              switch (event.event) {
                case "response_delta":
                  if (event.text) {
                    accumulated += event.text;
                    updateMessage(assistantId, accumulated);
                  }
                  break;

                case "tool_call_start":
                  updateToolCall(assistantId, {
                    name: event.tool_name || "unknown",
                    arguments: event.tool_arguments || "{}",
                    complete: false,
                  });
                  break;

                case "tool_call_result":
                  updateToolCall(assistantId, {
                    name: event.tool_name || "unknown",
                    arguments: event.tool_arguments || "{}",
                    result: event.tool_result || "",
                    complete: true,
                  });
                  break;
              }
            }
          }

          // Remove streaming cursor
          if (assistantEl) {
            const contentEl = assistantEl.querySelector(".message-content");
            if (contentEl) contentEl.classList.remove("streaming-cursor");
          }
        } catch (err) {
          const errorText = "**Error**: " + (err instanceof Error ? err.message : String(err));
          const currentContent = (findMessageEl(assistantId)?.querySelector(".message-content")?.textContent) || "";
          updateMessage(assistantId, currentContent + "\\n\\n" + errorText);

          if (assistantEl) {
            const contentEl = assistantEl.querySelector(".message-content");
            if (contentEl) contentEl.classList.remove("streaming-cursor");
          }
        } finally {
          isStreaming = false;
          streamingMessageId = null;
          sendBtn.disabled = false;
          composer.focus();
        }
      }

      function parseSSEFrame(frame) {
        let eventType = "";
        let data = "";

        for (const line of frame.split("\\n")) {
          if (line.startsWith("event: ")) {
            eventType = line.slice(7).trim();
          } else if (line.startsWith("data: ")) {
            data = line.slice(6).trim();
          }
        }

        if (!data) return null;

        try {
          const parsed = JSON.parse(data);
          return {
            event: eventType || parsed.event || "response_delta",
            text: parsed.text,
            tool_name: parsed.tool_name,
            tool_arguments: parsed.tool_arguments,
            tool_result: parsed.tool_result,
            model: parsed.model,
          };
        } catch (_) {
          return { event: "response_delta", text: data };
        }
      }

      // ── Event listeners ──
      composer.addEventListener("keydown", (e) => {
        if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
          e.preventDefault();
          doSend();
        }
      });

      sendBtn.addEventListener("click", () => doSend());

      // Auto-resize textarea
      composer.addEventListener("input", () => {
        composer.style.height = "auto";
        composer.style.height = Math.min(composer.scrollHeight, 120) + "px";
      });

      // ── Handle messages from extension host ──
      window.addEventListener("message", (event) => {
        const message = event.data;
        switch (message.type) {
          case "setMode":
            currentMode = message.mode;
            modeButtons.forEach((btn) => {
              btn.classList.toggle("active", btn.dataset.mode === currentMode);
            });
            break;

          case "addMessage":
            addMessage(message.message);
            break;

          case "updateMessage":
            updateMessage(message.id, message.content);
            break;

          case "updateToolCall":
            updateToolCall(message.messageId, message.toolCall);
            break;

          case "clear":
            transcript.innerHTML = "";
            break;
        }
      });

      // ── Ready ──
      vscode.postMessage({ type: "ready" });
    })();
  </script>
</body>
</html>`;
  }
}
