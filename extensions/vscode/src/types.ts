/**
 * Operating mode for a prompt request.
 * Mirrors the TUI's Plan / Agent / YOLO modes.
 */
export type Mode = "plan" | "agent" | "yolo";

/**
 * A single chat message (user or assistant).
 */
export interface Message {
  /** Unique id for this message within the transcript. */
  id: string;
  /** "user" | "assistant" */
  role: "user" | "assistant";
  /** Markdown body. For assistant messages this may be built up incrementally. */
  content: string;
  /** Timestamp the message was created (ISO 8601). */
  timestamp: string;
  /** Tool calls made by the assistant during this turn, if any. */
  toolCalls?: ToolCall[];
}

/**
 * A single tool invocation.
 */
export interface ToolCall {
  /** Tool name (e.g. "read_file", "exec_shell"). */
  name: string;
  /** JSON-stringified arguments. */
  arguments: string;
  /** Tool result (JSON string or plain text), filled in when the tool completes. */
  result?: string;
  /** Whether the tool call has completed. */
  complete: boolean;
}

/**
 * SSE event emitted by the app-server /stream endpoint.
 */
export interface StreamEvent {
  event: "response_start" | "response_delta" | "response_end" | "tool_call_start" | "tool_call_result";
  /** Token text for response_delta; null for other event types. */
  text?: string;
  /** Tool name for tool_call_start. */
  tool_name?: string;
  /** Tool arguments for tool_call_start. */
  tool_arguments?: string;
  /** Tool result for tool_call_result. */
  tool_result?: string;
  /** Model id reported on response_start. */
  model?: string;
}

/**
 * Messages sent from the extension host to the webview.
 */
export type HostToWebview =
  | { type: "setMode"; mode: Mode }
  | { type: "addMessage"; message: Message }
  | { type: "updateMessage"; id: string; content: string }
  | { type: "updateToolCall"; messageId: string; toolCall: ToolCall }
  | { type: "clear" };

/**
 * Messages sent from the webview to the extension host.
 */
export type WebviewToHost =
  | { type: "send"; text: string; mode: Mode }
  | { type: "modeChange"; mode: Mode }
  | { type: "ready" };
