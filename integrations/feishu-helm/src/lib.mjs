// Pure helpers for feishu-helm. The first half of this file is intentionally
// a verbatim port of integrations/feishu-bridge/src/lib.mjs so the two bridges
// share an identical command parser, allowlist, group-gate, env validator, and
// reply chunker. The second half adds helm-only helpers (code-point safe text
// slicing, tool-call summary formatting, markdown→CardKit JSON, the streaming
// card scaffold, and the extra env checks for FEISHU_USE_CARDS /
// FEISHU_APPROVAL_CARDS / FEISHU_CARDKIT_BASE_URL).

// ── Shared with feishu-bridge ───────────────────────────────────────────────

export function parseList(raw) {
  return String(raw || "")
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

export function parseBool(raw, fallback = false) {
  if (raw == null || raw === "") return fallback;
  return ["1", "true", "yes", "on"].includes(String(raw).trim().toLowerCase());
}

export function parseEnvText(raw) {
  const env = {};
  for (const line of String(raw || "").split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const normalized = trimmed.startsWith("export ") ? trimmed.slice(7).trim() : trimmed;
    const index = normalized.indexOf("=");
    if (index <= 0) continue;
    const key = normalized.slice(0, index).trim();
    let value = normalized.slice(index + 1).trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    env[key] = value;
  }
  return env;
}

export function cleanEnvValue(value) {
  return String(value ?? "").trim();
}

export function isPlaceholderValue(value) {
  const normalized = cleanEnvValue(value).toLowerCase();
  return (
    !normalized ||
    normalized.includes("replace-with") ||
    normalized.includes("xxxxxxxx") ||
    normalized === "changeme" ||
    normalized === "your_token_here"
  );
}

export function parseTextContent(content) {
  if (typeof content !== "string") return "";
  try {
    const parsed = JSON.parse(content);
    if (typeof parsed.text === "string") return parsed.text;
    if (typeof parsed.content === "string") return parsed.content;
  } catch {
    return content;
  }
  return content;
}

export function incomingIdentity(event) {
  const sender = event?.sender?.sender_id || {};
  const message = event?.message || {};
  return {
    chatId: message.chat_id || "",
    messageId: message.message_id || "",
    chatType: message.chat_type || "",
    messageType: message.message_type || "",
    openId: sender.open_id || "",
    unionId: sender.union_id || "",
    userId: sender.user_id || ""
  };
}

export function isAllowed(identity, allowlist, allowUnlisted = false) {
  if (allowUnlisted) return true;
  const allowed = new Set(allowlist);
  return [identity.chatId, identity.openId, identity.unionId, identity.userId]
    .filter(Boolean)
    .some((id) => allowed.has(id));
}

export function pairingRefusalText(identity) {
  return [
    "This chat is not in DEEPSEEK_CHAT_ALLOWLIST.",
    `chat_id=${identity.chatId}`,
    identity.openId ? `open_id=${identity.openId}` : "",
    identity.unionId ? `union_id=${identity.unionId}` : "",
    identity.userId ? `user_id=${identity.userId}` : ""
  ]
    .filter(Boolean)
    .join("\n");
}

export function stripGroupPrefix(text, { chatType, requirePrefix, prefix }) {
  const trimmed = String(text || "").trim();
  if (!trimmed) return { accepted: false, text: "" };
  if (!requirePrefix || chatType === "p2p") {
    return { accepted: true, text: trimmed };
  }
  const marker = prefix || "/ds";
  if (trimmed === marker) return { accepted: true, text: "/help" };
  if (trimmed.startsWith(`${marker} `)) {
    return { accepted: true, text: trimmed.slice(marker.length).trim() };
  }
  return { accepted: false, text: "" };
}

export function parseCommand(text) {
  const trimmed = String(text || "").trim();
  if (!trimmed.startsWith("/")) return { name: "prompt", args: trimmed };
  const [head, ...rest] = trimmed.split(/\s+/);
  return {
    name: head.slice(1).toLowerCase(),
    args: rest.join(" ").trim()
  };
}

export function parseApprovalDecisionArgs(args) {
  const parts = String(args || "")
    .split(/\s+/)
    .filter(Boolean);
  return {
    approvalId: parts[0] || "",
    remember: parts.slice(1).includes("remember")
  };
}

export function commandAction(command) {
  switch (command.name) {
    case "help":
      return { kind: "help" };
    case "status":
      return { kind: "status" };
    case "threads":
      return { kind: "threads" };
    case "new":
      return { kind: "new_thread" };
    case "resume":
      return { kind: "resume", threadId: command.args };
    case "interrupt":
      return { kind: "interrupt" };
    case "compact":
      return { kind: "compact" };
    case "allow":
      return { kind: "approval", decision: "allow", ...parseApprovalDecisionArgs(command.args) };
    case "deny":
      return { kind: "approval", decision: "deny", ...parseApprovalDecisionArgs(command.args) };
    case "prompt":
      return { kind: "prompt", prompt: command.args };
    default:
      return {
        kind: "prompt",
        prompt: `/${command.name}${command.args ? ` ${command.args}` : ""}`
      };
  }
}

export function splitMessage(text, maxChars = 3500) {
  const value = String(text || "");
  // Slice by code points, not UTF-16 code units, so emoji / supplementary-plane
  // CJK never split in the middle of a surrogate pair. Uses the helpers from
  // the helm extension half of this module.
  const len = codePointLen(value);
  if (len <= maxChars) return value ? [value] : [];
  const chunks = [];
  let cursor = 0;
  while (cursor < len) {
    chunks.push(safeSliceByCodePoint(value, cursor, cursor + maxChars));
    cursor += maxChars;
  }
  return chunks;
}

export function compactRuntimeError(status, body) {
  const message =
    body?.error?.message ||
    body?.message ||
    (typeof body === "string" ? body : JSON.stringify(body));
  return `Runtime API request failed (${status}): ${message}`;
}

export function latestRunningTurn(detail) {
  const turns = Array.isArray(detail?.turns) ? detail.turns : [];
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const turn = turns[index];
    if (["queued", "in_progress"].includes(turn?.status)) return turn;
  }
  return null;
}

export function activeTurnBlock(detail, state = {}) {
  const runningTurn = latestRunningTurn(detail);
  if (!runningTurn) return null;
  return {
    turnId: runningTurn.id || state.activeTurnId || "",
    message: `Thread already has active turn ${
      runningTurn.id || state.activeTurnId || "(unknown)"
    }. Wait for it to finish or send /interrupt.`
  };
}

export function validateBridgeConfig(env, options = {}) {
  const runtimeEnv = options.runtimeEnv || null;
  const workspaceRoot = options.workspaceRoot || "";
  const errors = [];
  const warnings = [];
  const info = [];
  const add = (list, code, message) => list.push({ code, message });

  for (const key of [
    "FEISHU_APP_ID",
    "FEISHU_APP_SECRET",
    "DEEPSEEK_RUNTIME_URL",
    "DEEPSEEK_RUNTIME_TOKEN",
    "DEEPSEEK_WORKSPACE",
    "FEISHU_THREAD_MAP_PATH"
  ]) {
    const value = cleanEnvValue(env[key]);
    if (!value) {
      add(errors, "missing_required", `${key} is required`);
    } else if (isPlaceholderValue(value)) {
      add(errors, "placeholder_value", `${key} still contains a placeholder value`);
    }
  }

  const domain = cleanEnvValue(env.FEISHU_DOMAIN || "feishu").toLowerCase();
  if (!["feishu", "lark"].includes(domain) && !/^https:\/\/open\./.test(domain)) {
    add(errors, "invalid_domain", "FEISHU_DOMAIN must be feishu, lark, or an https://open.* URL");
  }

  const runtimeUrl = cleanEnvValue(env.DEEPSEEK_RUNTIME_URL || "http://127.0.0.1:7878");
  try {
    const parsed = new URL(runtimeUrl);
    const localHosts = new Set(["127.0.0.1", "localhost", "[::1]", "::1"]);
    if (!["http:", "https:"].includes(parsed.protocol)) {
      add(errors, "invalid_runtime_url", "DEEPSEEK_RUNTIME_URL must use http or https");
    }
    if (!localHosts.has(parsed.hostname)) {
      add(errors, "remote_runtime_url", "DEEPSEEK_RUNTIME_URL must point at localhost");
    }
  } catch {
    add(errors, "invalid_runtime_url", "DEEPSEEK_RUNTIME_URL is not a valid URL");
  }

  const workspace = cleanEnvValue(env.DEEPSEEK_WORKSPACE);
  if (workspace && !workspace.startsWith("/")) {
    add(errors, "relative_workspace", "DEEPSEEK_WORKSPACE must be an absolute path");
  }
  if (
    workspace &&
    workspaceRoot &&
    workspace !== workspaceRoot &&
    !workspace.startsWith(`${workspaceRoot}/`)
  ) {
    add(warnings, "workspace_root", `DEEPSEEK_WORKSPACE is outside ${workspaceRoot}`);
  }

  const threadMapPath = cleanEnvValue(env.FEISHU_THREAD_MAP_PATH);
  if (threadMapPath && !threadMapPath.startsWith("/")) {
    add(errors, "relative_thread_map", "FEISHU_THREAD_MAP_PATH must be an absolute path");
  }

  const allowGroups = parseBool(env.FEISHU_ALLOW_GROUPS, false);
  const requirePrefix = parseBool(env.FEISHU_REQUIRE_PREFIX_IN_GROUP, true);
  const allowUnlisted = parseBool(env.DEEPSEEK_ALLOW_UNLISTED, false);
  const allowlist = parseList(env.DEEPSEEK_CHAT_ALLOWLIST);

  if (!allowlist.length && allowUnlisted) {
    add(warnings, "pairing_mode_open", "DEEPSEEK_ALLOW_UNLISTED=true leaves first-pairing mode open");
  } else if (!allowlist.length) {
    add(warnings, "not_paired", "DEEPSEEK_CHAT_ALLOWLIST is empty; all chats will be refused");
  }
  if (allowGroups && allowUnlisted) {
    add(errors, "open_group_control", "Group control cannot be enabled while unlisted chats are allowed");
  }
  if (allowGroups && !requirePrefix) {
    add(warnings, "group_without_prefix", "Group control is enabled without requiring FEISHU_GROUP_PREFIX");
  }
  if (!allowGroups) {
    add(info, "dm_only", "Direct-message control is enabled; group chats are disabled");
  }

  const maxReplyChars = Number(env.FEISHU_MAX_REPLY_CHARS || 3500);
  if (!Number.isFinite(maxReplyChars) || maxReplyChars < 100) {
    add(errors, "invalid_max_reply_chars", "FEISHU_MAX_REPLY_CHARS must be at least 100");
  }
  const turnTimeoutMs = Number(env.DEEPSEEK_TURN_TIMEOUT_MS || 900000);
  if (!Number.isFinite(turnTimeoutMs) || turnTimeoutMs < 1000) {
    add(errors, "invalid_turn_timeout", "DEEPSEEK_TURN_TIMEOUT_MS must be at least 1000");
  }

  if (runtimeEnv) {
    const runtimeToken = cleanEnvValue(runtimeEnv.DEEPSEEK_RUNTIME_TOKEN);
    const bridgeToken = cleanEnvValue(env.DEEPSEEK_RUNTIME_TOKEN);
    if (!runtimeToken) {
      add(errors, "missing_runtime_token", "runtime.env is missing DEEPSEEK_RUNTIME_TOKEN");
    } else if (isPlaceholderValue(runtimeToken)) {
      add(errors, "placeholder_runtime_token", "runtime.env DEEPSEEK_RUNTIME_TOKEN is still a placeholder");
    } else if (bridgeToken && bridgeToken !== runtimeToken) {
      add(errors, "token_mismatch", "Runtime and bridge DEEPSEEK_RUNTIME_TOKEN values do not match");
    }

    const apiKey = cleanEnvValue(runtimeEnv.DEEPSEEK_API_KEY);
    if (!apiKey) {
      add(warnings, "missing_api_key", "runtime.env is missing DEEPSEEK_API_KEY");
    } else if (isPlaceholderValue(apiKey)) {
      add(warnings, "placeholder_api_key", "runtime.env DEEPSEEK_API_KEY is still a placeholder");
    }

    const runtimePort = Number(runtimeEnv.DEEPSEEK_RUNTIME_PORT || 7878);
    if (!Number.isInteger(runtimePort) || runtimePort <= 0 || runtimePort > 65535) {
      add(errors, "invalid_runtime_port", "DEEPSEEK_RUNTIME_PORT must be a valid TCP port");
    }
  }

  // ── feishu-helm specific extensions ───────────────────────────────────────
  const useCards = parseBool(env.FEISHU_USE_CARDS, false);
  const approvalCards = parseBool(env.FEISHU_APPROVAL_CARDS, false);
  const cardkitUrl = cleanEnvValue(env.FEISHU_CARDKIT_BASE_URL || "https://open.feishu.cn/open-apis");
  if (cardkitUrl) {
    try {
      const parsed = new URL(cardkitUrl);
      if (parsed.protocol !== "https:") {
        add(errors, "insecure_cardkit_url", "FEISHU_CARDKIT_BASE_URL must use https://");
      }
      const okHost =
        parsed.hostname === "open.feishu.cn" || parsed.hostname === "open.larksuite.com";
      if (!okHost) {
        add(warnings, "unexpected_cardkit_host", `FEISHU_CARDKIT_BASE_URL host is unusual: ${parsed.hostname}`);
      }
    } catch {
      add(errors, "invalid_cardkit_url", "FEISHU_CARDKIT_BASE_URL is not a valid URL");
    }
  }
  if (approvalCards && !useCards) {
    add(warnings, "approval_cards_without_cards", "FEISHU_APPROVAL_CARDS=true has no effect unless FEISHU_USE_CARDS=true");
  }
  if (useCards) add(info, "cards_enabled", "Streaming cards enabled (FEISHU_USE_CARDS=true)");
  if (approvalCards) add(info, "approval_cards_enabled", "Approval card buttons enabled (FEISHU_APPROVAL_CARDS=true)");

  return {
    ok: errors.length === 0,
    errors,
    warnings,
    info
  };
}

export function formatValidationReport(result) {
  const lines = ["Feishu helm config validation"];
  for (const item of result.errors) lines.push(`[fail] ${item.message}`);
  for (const item of result.warnings) lines.push(`[warn] ${item.message}`);
  for (const item of result.info) lines.push(`[info] ${item.message}`);
  if (result.ok) lines.push("[ok] No blocking config errors found");
  return lines.join("\n");
}

export function helpText() {
  return [
    "DeepSeek phone bridge commands (feishu-helm):",
    "/help - show this help",
    "/status - runtime and workspace status",
    "/threads - recent runtime threads",
    "/new - create a new thread for this chat",
    "/resume <thread_id> - bind this chat to an existing thread",
    "/interrupt - interrupt the active turn",
    "/compact - compact the current thread",
    "/allow <approval_id> [remember] - approve a pending tool call",
    "/deny <approval_id> - deny a pending tool call",
    "",
    "Anything else is sent as a DeepSeek prompt.",
    "Set FEISHU_USE_CARDS=true for streaming markdown cards;",
    "FEISHU_APPROVAL_CARDS=true to additionally surface approvals as buttons."
  ].join("\n");
}

// ── feishu-helm specific helpers ────────────────────────────────────────────

/** UTF-8 / surrogate-pair safe code-point length. */
export function codePointLen(s) {
  return [...String(s || "")].length;
}

/**
 * Code-point safe slice. JS `String.slice` operates on UTF-16 code units, which
 * breaks emoji and CJK supplementary planes. We need this to feed CardKit's
 * streaming PUT /content which interprets the string as user-visible
 * characters and silently corrupts mid-codepoint truncations.
 */
export function safeSliceByCodePoint(s, start, end) {
  return [...String(s || "")].slice(start, end).join("");
}

/** Pull a one-line preview from multi-line tool / file change output. */
export function lastLineSummary(text, maxLen) {
  const value = String(text || "");
  if (value.length <= maxLen) return value;
  const lines = value.split("\n").filter((l) => l.trim());
  const last = lines.length > 0 ? lines[lines.length - 1].trim() : value;
  return last.length > maxLen ? `${last.slice(0, maxLen)}...` : last;
}

/**
 * Format a tool_call's JSON parameter payload as a concise human label.
 * Handles common parameter shapes (path, prompt, content, command, ...) and
 * falls back to the first few keys.
 */
export function formatToolDisplay(paramJson) {
  let p;
  try {
    p = JSON.parse(paramJson);
  } catch {
    return "";
  }
  if (!p || typeof p !== "object") return "";
  if (p.path || p.file_path || p.filePath) {
    return `path=${p.path || p.file_path || p.filePath}`;
  }
  if (p.prompt) return `prompt=${String(p.prompt).slice(0, 80)}...`;
  if (p.content) return `${String(p.content).slice(0, 80)}...`;
  if (p.code) return `code=${String(p.code).slice(0, 80)}...`;
  if (p.name) return `name=${p.name}`;
  if (p.number !== undefined && p.number !== null) return `#${p.number}`;
  if (p.task_id) return `task=${String(p.task_id).slice(-8)}`;
  if (p.tool_name) return `tool=${p.tool_name}`;
  if (p.command) return `cmd=${String(p.command).slice(0, 60)}`;
  const keys = Object.keys(p);
  if (keys.length === 1) {
    const v = String(p[keys[0]]).slice(0, 80);
    return `${keys[0]}=${v}`;
  }
  if (keys.length > 0) {
    return keys
      .slice(0, 3)
      .map((k) => `${k}=${String(p[k]).slice(0, 30)}`)
      .join(", ");
  }
  return "";
}

/**
 * Build a Feishu / Lark JSON 2.0 card scaffold with streaming_mode=true. Returns
 * the JSON-encoded card body the CardKit create API expects. Element is empty
 * markdown — actual text is fed in via PUT /content streaming updates.
 *
 * See https://open.feishu.cn/document/cardkit-v1/streaming-updates-openapi-overview
 */
export function buildStreamCardJson(elementId, sc = {}) {
  const freq = sc.print_frequency_ms ?? 70;
  const step = sc.print_step ?? 1;
  const strategy = sc.print_strategy ?? "fast";
  const card = {
    schema: "2.0",
    header: { title: { content: "", tag: "plain_text" } },
    config: {
      streaming_mode: true,
      update_multi: true,
      wide_screen_mode: true,
      enable_forward: false,
      summary: { content: "Thinking..." },
      streaming_config: {
        print_frequency_ms: { default: freq, android: freq, ios: freq, pc: freq },
        print_step: { default: step, android: step, ios: step, pc: step },
        print_strategy: strategy
      }
    },
    body: {
      elements: [{ tag: "markdown", content: "", element_id: elementId }]
    }
  };
  return JSON.stringify(card);
}

/**
 * Render a closed (non-streaming) interactive markdown card body for a chunk
 * of text. Used to deliver final agent_message replies as a card when
 * FEISHU_USE_CARDS=true (the streaming card path is for the in-progress
 * typewriter; this path produces a final, non-streaming card).
 */
export function markdownToCard(text) {
  return JSON.stringify({
    schema: "2.0",
    config: { wide_screen_mode: true, enable_forward: false },
    body: {
      elements: [{ tag: "markdown", content: String(text || "") }]
    }
  });
}

/**
 * Approval card body with Approve / Deny buttons. The button payload carries
 * the approval_id so the card.action handler can route it. Operator identity
 * is still checked against the chat allowlist before dispatching to the
 * runtime — see src/index.mjs handleCardAction.
 */
export function approvalCardJson(approval) {
  const id = approval?.approval_id || approval?.id || "";
  const tool = approval?.tool_name || "unknown";
  const description = approval?.description || "";
  return JSON.stringify({
    schema: "2.0",
    config: { wide_screen_mode: true, enable_forward: false },
    header: { title: { content: `Approval required: ${tool}`, tag: "plain_text" } },
    body: {
      elements: [
        description
          ? { tag: "markdown", content: description }
          : { tag: "markdown", content: `approval_id=${id}` },
        {
          tag: "action",
          actions: [
            {
              tag: "button",
              text: { tag: "plain_text", content: "Approve" },
              type: "primary",
              value: { action: "allow", approval_id: id }
            },
            {
              tag: "button",
              text: { tag: "plain_text", content: "Deny" },
              type: "danger",
              value: { action: "deny", approval_id: id }
            }
          ]
        }
      ]
    }
  });
}

/**
 * Operator allowlist check for card action callbacks. Lark's
 * card.action.trigger event carries the clicker identity in
 * `event.operator`. Even when the card is visible to the whole group, only
 * users in the chat allowlist are permitted to approve / deny.
 */
export function cardOperatorAllowed(event, allowlist, allowUnlisted = false) {
  if (allowUnlisted) return true;
  const op = event?.operator || event?.action?.operator || {};
  const identity = {
    chatId: event?.chat_id || event?.context?.open_chat_id || "",
    openId: op.open_id || op.operator_id?.open_id || "",
    unionId: op.union_id || op.operator_id?.union_id || "",
    userId: op.user_id || op.operator_id?.user_id || ""
  };
  return isAllowed(identity, allowlist, false);
}
