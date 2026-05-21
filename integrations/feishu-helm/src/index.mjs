// feishu-helm — Feishu/Lark long-connection bridge with streaming CardKit
// output. Sibling integration to feishu-bridge: same command surface, same
// security model, same env contract; differs only in (1) streaming-card
// reply rendering, (2) optional approval card buttons, and (3) SSE
// auto-reconnect with since_seq.
//
// Run:  node src/index.mjs
// Validate config:  npm run validate:config -- --env /etc/deepseek/feishu-helm.env

import * as Lark from "@larksuiteoapi/node-sdk";

import {
  approvalCardJson,
  activeTurnBlock,
  cardOperatorAllowed,
  commandAction,
  helpText,
  incomingIdentity,
  isAllowed,
  latestRunningTurn,
  markdownToCard,
  pairingRefusalText,
  parseBool,
  parseCommand,
  parseList,
  parseApprovalDecisionArgs,
  parseTextContent,
  splitMessage,
  stripGroupPrefix
} from "./lib.mjs";

import { CardKitClient } from "./cardkit.mjs";
import { RuntimeClient, SseEventHandler, ThreadStore } from "./runtime.mjs";

const config = {
  appId: requiredEnv("FEISHU_APP_ID"),
  appSecret: requiredEnv("FEISHU_APP_SECRET"),
  domain: process.env.FEISHU_DOMAIN || "feishu",
  runtimeUrl: (process.env.DEEPSEEK_RUNTIME_URL || "http://127.0.0.1:7878").replace(/\/+$/, ""),
  runtimeToken: requiredEnv("DEEPSEEK_RUNTIME_TOKEN"),
  workspace: process.env.DEEPSEEK_WORKSPACE || process.cwd(),
  model: process.env.DEEPSEEK_MODEL || "auto",
  mode: process.env.DEEPSEEK_MODE || "agent",
  allowShell: parseBool(process.env.DEEPSEEK_ALLOW_SHELL, true),
  trustMode: parseBool(process.env.DEEPSEEK_TRUST_MODE, false),
  autoApprove: parseBool(process.env.DEEPSEEK_AUTO_APPROVE, false),
  allowlist: parseList(process.env.DEEPSEEK_CHAT_ALLOWLIST),
  allowUnlisted: parseBool(process.env.DEEPSEEK_ALLOW_UNLISTED, false),
  threadMapPath:
    process.env.FEISHU_THREAD_MAP_PATH || "/var/lib/deepseek-feishu-helm/thread-map.json",
  allowGroups: parseBool(process.env.FEISHU_ALLOW_GROUPS, false),
  requirePrefixInGroup: parseBool(process.env.FEISHU_REQUIRE_PREFIX_IN_GROUP, true),
  groupPrefix: process.env.FEISHU_GROUP_PREFIX || "/ds",
  maxReplyChars: Number(process.env.FEISHU_MAX_REPLY_CHARS || 3500),
  turnTimeoutMs: Number(process.env.DEEPSEEK_TURN_TIMEOUT_MS || 900000),
  useCards: parseBool(process.env.FEISHU_USE_CARDS, false),
  approvalCards: parseBool(process.env.FEISHU_APPROVAL_CARDS, false),
  cardkitBaseUrl:
    (process.env.FEISHU_CARDKIT_BASE_URL || "https://open.feishu.cn/open-apis").replace(/\/+$/, "")
};

// Hard refusals — fail fast rather than start in a misconfigured state.
{
  try {
    const u = new URL(config.runtimeUrl);
    const localHosts = new Set(["127.0.0.1", "localhost", "[::1]", "::1"]);
    if (!localHosts.has(u.hostname)) {
      throw new Error(`DEEPSEEK_RUNTIME_URL must point at localhost, got ${u.hostname}`);
    }
  } catch (err) {
    console.error(String(err?.message ?? err));
    process.exit(1);
  }
}

const sdkConfig = {
  appId: config.appId,
  appSecret: config.appSecret,
  domain: resolveLarkDomain(config.domain)
};

const lark = new Lark.Client(sdkConfig);
const wsClient = new Lark.WSClient({
  ...sdkConfig,
  loggerLevel: Lark.LoggerLevel?.info
});

const runtime = new RuntimeClient({ baseUrl: config.runtimeUrl, token: config.runtimeToken });
const threadStore = await ThreadStore.open(config.threadMapPath);
const cardkit = config.useCards
  ? new CardKitClient(
      { baseUrl: config.cardkitBaseUrl },
      tenantAccessToken,
      (m) => console.log(m)
    )
  : null;

const sse = new SseEventHandler(runtime, (m) => console.log(m));

// Per-thread streaming state. One agent_message at a time per thread; the
// daemon naturally serializes turns by awaiting each `runPrompt`. The
// `resolveTurn` callback is the bridge between the global SseEventHandler's
// callbacks (which fire by threadId) and the per-turn promise streamTurn awaits.
/**
 * @type {Map<string, {
 *   chatId: string,
 *   turnId: string | null,
 *   text: string,
 *   cardId: string | null,
 *   elementId: string,
 *   sequence: number,
 *   closed: boolean,
 *   sentProgressAt: number,
 *   resolveTurn: (result: {status?: string, error?: string, kind?: string}) => void
 * }>}
 */
const streamingByThread = new Map();

// ── SSE callbacks ───────────────────────────────────────────────────────────
//
// One SseEventHandler instance fans out to every active turn via streamingByThread
// lookup. SseEventHandler owns the auto-reconnect + since_seq replay logic
// (see runtime.mjs); this module only renders the events into Feishu.
//
// All callbacks early-return when the event's turn_id doesn't match the
// streaming state's turnId, so older-turn events still flowing through a
// thread's stream don't bleed into the active turn's reply.

sse.onDelta = (threadId, delta) => {
  if (delta.kind !== "agent_message") return;
  const st = streamingByThread.get(threadId);
  if (!st) return;
  if (delta.turn_id && st.turnId && delta.turn_id !== st.turnId) return;

  st.text += delta.delta;
  if (config.useCards) {
    if (!st.cardId && cardkit) {
      void initStreamingCard(st).catch((err) => {
        console.error(`[helm] init streaming card failed: ${err?.message ?? err}`);
      });
    }
    if (st.cardId && !st.closed) {
      flushStreamingCard(st).catch((err) => {
        console.error(`[helm] flush streaming card failed: ${err?.message ?? err}`);
      });
    }
    return;
  }
  // Plaintext mode: chunked progress sends, mirroring feishu-bridge semantics.
  if (st.text.length > config.maxReplyChars && Date.now() - st.sentProgressAt > 15000) {
    const chunk = st.text.slice(0, config.maxReplyChars);
    st.text = st.text.slice(config.maxReplyChars);
    st.sentProgressAt = Date.now();
    sendText(st.chatId, chunk).catch((err) => {
      console.error(`[helm] progress send failed: ${err?.message ?? err}`);
    });
  }
};

sse.onApprovalRequired = (threadId, approval, turnId) => {
  const st = streamingByThread.get(threadId);
  if (!st) return;
  if (turnId && st.turnId && turnId !== st.turnId) return;
  void deliverApproval(st.chatId, approval).catch((err) => {
    console.error(`[helm] deliver approval failed: ${err?.message ?? err}`);
  });
};

sse.onTurnLifecycle = (threadId, status, turn, turnId) => {
  const st = streamingByThread.get(threadId);
  if (!st) return;
  if (turnId && st.turnId && turnId !== st.turnId) return;
  if (["failed", "canceled", "interrupted"].includes(status)) {
    st.resolveTurn({ status, error: turn?.error });
  }
};

sse.onTurnCompleted = (threadId, turn) => {
  const st = streamingByThread.get(threadId);
  if (!st) return;
  if (turn?.id && st.turnId && turn.id !== st.turnId) return;
  st.resolveTurn({ status: turn?.status || "completed", error: turn?.error });
};

sse.onSeqProgress = (threadId, seq) => {
  const st = streamingByThread.get(threadId);
  if (!st) return;
  threadStore.patchChat(st.chatId, { lastSeq: seq }).catch(() => {
    // ThreadStore.save() already serialises; failures here are best-effort.
  });
};

// ── Lark event dispatcher ───────────────────────────────────────────────────

const dispatcher = new Lark.EventDispatcher({}).register({
  "im.message.receive_v1": async (data) => {
    void handleIncomingMessage(data).catch((err) => {
      console.error("[helm] handle message failed:", err);
    });
  },
  "card.action.trigger": async (data) => {
    try {
      return await handleCardAction(data);
    } catch (err) {
      console.error("[helm] card action failed:", err);
      return {};
    }
  }
});

console.log("Starting feishu-helm bridge");
console.log(`Runtime: ${config.runtimeUrl}`);
console.log(`Workspace: ${config.workspace}`);
console.log(`Cards: ${config.useCards ? "enabled" : "disabled (plaintext)"}`);
console.log(`Approval cards: ${config.approvalCards ? "enabled" : "disabled"}`);
if (!config.allowlist.length && !config.allowUnlisted) {
  console.log("No allowlist configured. Incoming chats will receive their IDs and be refused.");
}

wsClient.start({ eventDispatcher: dispatcher });

// ── Incoming message routing ────────────────────────────────────────────────

async function handleIncomingMessage(event) {
  const identity = incomingIdentity(event);
  if (!identity.chatId) return;

  if (identity.messageType && identity.messageType !== "text") {
    await sendText(identity.chatId, "Only text messages are supported in this bridge.");
    return;
  }

  const rawText = parseTextContent(event.message?.content || "");
  const scoped = stripGroupPrefix(rawText, {
    chatType: identity.chatType,
    requirePrefix: config.requirePrefixInGroup,
    prefix: config.groupPrefix
  });
  if (!scoped.accepted) return;

  // Persistent message-id dedup — defeats Feishu WS redelivery on restart.
  if (identity.messageId && (await threadStore.recordMessage(identity.messageId))) {
    return;
  }

  if (identity.chatType !== "p2p" && !config.allowGroups) {
    await sendText(
      identity.chatId,
      "Group chat control is disabled for this bridge. DM the bot, or set FEISHU_ALLOW_GROUPS=true and allowlist this chat."
    );
    return;
  }

  if (!isAllowed(identity, config.allowlist, config.allowUnlisted)) {
    await sendText(identity.chatId, pairingRefusalText(identity));
    return;
  }

  const command = parseCommand(scoped.text);
  await handleCommand(identity.chatId, command);
}

async function handleCommand(chatId, command) {
  const action = commandAction(command);
  switch (action.kind) {
    case "help":
      await sendText(chatId, helpText());
      return;
    case "status":
      await sendStatus(chatId);
      return;
    case "threads":
      await sendThreads(chatId);
      return;
    case "new_thread": {
      const state = await ensureThread(chatId, { forceNew: true });
      await sendText(chatId, `Created thread ${state.threadId}`);
      return;
    }
    case "resume":
      await resumeThread(chatId, action.threadId);
      return;
    case "interrupt":
      await interruptActiveTurn(chatId);
      return;
    case "compact":
      await compactThread(chatId);
      return;
    case "approval":
      await decideApproval(chatId, action);
      return;
    case "prompt":
      await runPrompt(chatId, action.prompt);
      return;
    default:
      await sendText(chatId, helpText());
  }
}

// ── Thread lifecycle ───────────────────────────────────────────────────────

async function ensureThread(chatId, { forceNew = false } = {}) {
  const existing = await threadStore.getChat(chatId);
  if (existing?.threadId && !forceNew) return existing;

  const thread = await runtime.json("/v1/threads", {
    method: "POST",
    body: {
      model: config.model,
      workspace: config.workspace,
      mode: config.mode,
      allow_shell: config.allowShell,
      trust_mode: config.trustMode,
      auto_approve: config.autoApprove,
      archived: false,
      system_prompt:
        "You are being controlled from a Feishu/Lark chat with rich card output. Keep status updates concise. Ask for tool approvals when needed; do not assume mobile messages imply blanket approval."
    }
  });

  const state = {
    threadId: thread.id,
    lastSeq: 0,
    activeTurnId: null,
    updatedAt: new Date().toISOString()
  };
  await threadStore.setChat(chatId, state);
  return state;
}

async function runPrompt(chatId, prompt) {
  if (!prompt.trim()) {
    await sendText(chatId, helpText());
    return;
  }
  const state = await ensureThread(chatId);
  const detail = await runtime.json(`/v1/threads/${encodeURIComponent(state.threadId)}`);
  const blocking = activeTurnBlock(detail, state);
  if (blocking) {
    await threadStore.patchChat(chatId, {
      activeTurnId: blocking.turnId,
      updatedAt: new Date().toISOString()
    });
    await sendText(chatId, blocking.message);
    return;
  }
  if (state.activeTurnId) {
    await threadStore.patchChat(chatId, { activeTurnId: null });
  }
  const sinceSeq = Number(detail.latest_seq || state.lastSeq || 0);

  const turnResponse = await runtime.json(
    `/v1/threads/${encodeURIComponent(state.threadId)}/turns`,
    {
      method: "POST",
      body: {
        prompt,
        input_summary: prompt.slice(0, 200),
        model: config.model,
        mode: config.mode,
        allow_shell: config.allowShell,
        trust_mode: config.trustMode,
        auto_approve: config.autoApprove
      }
    }
  );

  const turnId = turnResponse.turn?.id;
  await threadStore.patchChat(chatId, {
    activeTurnId: turnId || null,
    lastSeq: sinceSeq,
    updatedAt: new Date().toISOString()
  });
  await sendText(chatId, `Started turn ${turnId || "(unknown)"}`);

  try {
    await streamTurn(chatId, state.threadId, turnId, sinceSeq);
  } finally {
    await threadStore.patchChat(chatId, {
      activeTurnId: null,
      updatedAt: new Date().toISOString()
    });
  }
}

async function streamTurn(chatId, threadId, turnId, sinceSeq) {
  // Drive the turn entirely through SseEventHandler so the auto-reconnect +
  // since_seq replay path on runtime.mjs covers `deepseek serve` restarts and
  // transient network errors. Module-level callbacks (see top of file) push
  // results back here via `resolveTurn`.

  /** @type {(result: {status?: string, error?: string, kind?: string}) => void} */
  let resolveTurn;
  const turnDone = new Promise((res) => {
    resolveTurn = res;
  });

  streamingByThread.set(threadId, {
    chatId,
    turnId: turnId ?? null,
    text: "",
    cardId: null,
    elementId: "agent_msg",
    sequence: 0,
    closed: false,
    sentProgressAt: Date.now(),
    resolveTurn
  });

  const timeoutTimer = setTimeout(() => {
    resolveTurn({ kind: "timeout" });
  }, config.turnTimeoutMs);

  try {
    await sse.connect(threadId, sinceSeq);
    const result = await turnDone;
    await closeStreamingCard(threadId);
    const st = streamingByThread.get(threadId);
    const finalText = st?.text?.trim() || "Turn completed.";

    if (result.kind === "timeout") {
      await sendText(chatId, `Turn timed out after ${Math.round(config.turnTimeoutMs / 1000)}s.`);
    } else if (!result.status || result.status === "completed") {
      if (config.useCards) {
        // If we never created a streaming card (zero deltas), still surface
        // the final text as a non-streaming card.
        if (st && !st.cardId) {
          await sendCard(chatId, markdownToCard(finalText));
        }
      } else {
        await sendText(chatId, finalText);
      }
    } else {
      const error = result.error ? `\n${result.error}` : "";
      await sendText(chatId, `Turn ${result.status}.${error}`.trim());
    }
  } finally {
    clearTimeout(timeoutTimer);
    sse.disconnect(threadId);
    streamingByThread.delete(threadId);
  }
}

// ── Streaming card lifecycle ────────────────────────────────────────────────

async function initStreamingCard(st) {
  if (!cardkit) return;
  if (st.cardId) return;
  const result = await cardkit.createStreamCard({
    elementId: st.elementId,
    streamingConfig: { print_frequency_ms: 70, print_step: 1, print_strategy: "fast" }
  });
  await cardkit.sendCardToChat(st.chatId, result.card_id);
  st.cardId = result.card_id;
}

async function flushStreamingCard(st) {
  if (!cardkit || !st.cardId || st.closed) return;
  st.sequence += 1;
  try {
    await cardkit.updateContent(st.cardId, st.elementId, st.text, st.sequence);
  } catch (err) {
    console.error(`[helm] streaming card flush failed seq=${st.sequence}: ${err?.message ?? err}`);
    // Don't disable streaming on a single failure — the next delta will retry.
  }
}

async function closeStreamingCard(threadId) {
  if (!cardkit) return;
  const st = streamingByThread.get(threadId);
  if (!st || st.closed || !st.cardId) {
    if (st) st.closed = true;
    return;
  }
  st.closed = true;
  try {
    st.sequence += 1;
    await cardkit.updateContent(st.cardId, st.elementId, st.text, st.sequence);
  } catch {}
  try {
    st.sequence += 1;
    await cardkit.updateSettings(st.cardId, { config: { streaming_mode: false } }, st.sequence);
  } catch {}
}

// ── Approval delivery ───────────────────────────────────────────────────────

async function deliverApproval(chatId, approval) {
  const id = approval.approval_id || approval.id || "";
  const lines = [
    "Approval required",
    `tool=${approval.tool_name || "unknown"}`,
    `approval_id=${id}`,
    approval.description || "",
    "",
    `Reply /allow ${id}`,
    `Reply /deny ${id}`
  ]
    .filter(Boolean)
    .join("\n");
  await sendText(chatId, lines);

  if (config.approvalCards && config.useCards) {
    // Surface the same approval as a card with buttons in addition to the
    // text fallback. Either path goes through the same allowlist check.
    await sendCard(chatId, approvalCardJson(approval));
  }
}

// ── Card action handler (button-press approvals) ────────────────────────────

async function handleCardAction(event) {
  if (!cardOperatorAllowed(event, config.allowlist, config.allowUnlisted)) {
    return { toast: { type: "error", content: "Not authorized to approve from this account." } };
  }
  const value = event?.action?.value || {};
  const decision = value.action;
  const approvalId = value.approval_id || value.id;
  if (!approvalId || !["allow", "deny"].includes(decision)) {
    return { toast: { type: "error", content: "Malformed approval action." } };
  }
  try {
    await runtime.json(`/v1/approvals/${encodeURIComponent(approvalId)}`, {
      method: "POST",
      body: { decision, remember: false }
    });
  } catch (err) {
    return { toast: { type: "error", content: `Approval failed: ${err?.message ?? err}` } };
  }
  return { toast: { type: "success", content: `Approval ${approvalId}: ${decision}` } };
}

// ── Status / threads / resume / interrupt / compact / approval ──────────────

async function sendStatus(chatId) {
  const [health, runtimeInfo, workspace] = await Promise.all([
    runtime.json("/health", { auth: false }),
    runtime.json("/v1/runtime/info"),
    runtime.json("/v1/workspace/status")
  ]);
  await sendText(
    chatId,
    [
      `runtime=${health.status || "unknown"}`,
      `version=${runtimeInfo.version || "unknown"}`,
      `bind=${runtimeInfo.bind_host}:${runtimeInfo.port}`,
      `auth_required=${runtimeInfo.auth_required}`,
      `workspace=${workspace.workspace}`,
      `git_repo=${workspace.git_repo}`,
      workspace.branch ? `branch=${workspace.branch}` : "",
      `staged=${workspace.staged} unstaged=${workspace.unstaged} untracked=${workspace.untracked}`,
      `cards=${config.useCards} approval_cards=${config.approvalCards}`
    ]
      .filter(Boolean)
      .join("\n")
  );
}

async function sendThreads(chatId) {
  const threads = await runtime.json("/v1/threads/summary?limit=8&include_archived=true");
  if (!Array.isArray(threads) || !threads.length) {
    await sendText(chatId, "No runtime threads yet.");
    return;
  }
  await sendText(
    chatId,
    threads
      .map((thread) => {
        const status = thread.latest_turn_status || "none";
        return `${thread.id} [${status}] ${thread.title || thread.preview || ""}`;
      })
      .join("\n")
  );
}

async function resumeThread(chatId, args) {
  const threadId = (args || "").trim();
  if (!threadId) {
    await sendText(chatId, "Usage: /resume <thread_id>");
    return;
  }
  const detail = await runtime.json(`/v1/threads/${encodeURIComponent(threadId)}`);
  await threadStore.setChat(chatId, {
    threadId,
    lastSeq: Number(detail.latest_seq || 0),
    activeTurnId: null,
    updatedAt: new Date().toISOString()
  });
  await sendText(chatId, `Resumed thread ${threadId}`);
}

async function interruptActiveTurn(chatId) {
  const state = await threadStore.getChat(chatId);
  if (!state?.threadId) {
    await sendText(chatId, "No runtime thread recorded for this chat.");
    return;
  }
  const detail = await runtime.json(`/v1/threads/${encodeURIComponent(state.threadId)}`);
  const runningTurn = latestRunningTurn(detail);
  const turnId = state.activeTurnId || runningTurn?.id;
  if (!turnId) {
    await sendText(chatId, "No active turn recorded for this chat.");
    return;
  }
  await runtime.json(
    `/v1/threads/${encodeURIComponent(state.threadId)}/turns/${encodeURIComponent(turnId)}/interrupt`,
    { method: "POST" }
  );
  await threadStore.patchChat(chatId, {
    activeTurnId: turnId,
    updatedAt: new Date().toISOString()
  });
  await sendText(chatId, `Interrupt requested for ${turnId}`);
}

async function compactThread(chatId) {
  const state = await ensureThread(chatId);
  const result = await runtime.json(`/v1/threads/${encodeURIComponent(state.threadId)}/compact`, {
    method: "POST",
    body: { reason: "phone bridge request" }
  });
  await sendText(chatId, `Compaction started: ${result.turn?.id || "unknown turn"}`);
}

async function decideApproval(chatId, action) {
  const decision = action.decision;
  const { approvalId, remember } =
    action.approvalId != null ? action : parseApprovalDecisionArgs(action.args);
  if (!approvalId) {
    await sendText(chatId, `Usage: /${decision} <approval_id>${decision === "allow" ? " [remember]" : ""}`);
    return;
  }
  await runtime.json(`/v1/approvals/${encodeURIComponent(approvalId)}`, {
    method: "POST",
    body: { decision, remember }
  });
  await sendText(chatId, `Approval ${approvalId}: ${decision}${remember ? " and remember" : ""}`);
}

// ── Feishu send helpers ─────────────────────────────────────────────────────

async function sendText(chatId, text) {
  const createMessage =
    lark.im?.v1?.message?.create?.bind(lark.im.v1.message) ||
    lark.im?.message?.create?.bind(lark.im.message);
  if (!createMessage) throw new Error("Lark SDK client does not expose im message create API");
  for (const chunk of splitMessage(text, config.maxReplyChars)) {
    await createMessage({
      params: { receive_id_type: "chat_id" },
      data: {
        receive_id: chatId,
        msg_type: "text",
        content: JSON.stringify({ text: chunk })
      }
    });
  }
}

async function sendCard(chatId, cardJson) {
  const createMessage =
    lark.im?.v1?.message?.create?.bind(lark.im.v1.message) ||
    lark.im?.message?.create?.bind(lark.im.message);
  if (!createMessage) throw new Error("Lark SDK client does not expose im message create API");
  await createMessage({
    params: { receive_id_type: "chat_id" },
    data: {
      receive_id: chatId,
      msg_type: "interactive",
      content: cardJson
    }
  });
}

// Used by CardKitClient — mint tenant_access_token, cached until ~1 minute
// before expiry. Without the cache we'd hit /auth on every streaming PUT,
// which fires several times per second during an agent_message.
let cachedTenantToken = null;
let tenantTokenExpiresAt = 0;
let tenantTokenInflight = null;

async function tenantAccessToken() {
  if (cachedTenantToken && Date.now() < tenantTokenExpiresAt) {
    return cachedTenantToken;
  }
  if (tenantTokenInflight) return tenantTokenInflight;

  tenantTokenInflight = (async () => {
    const res = await fetch(`${config.cardkitBaseUrl}/auth/v3/tenant_access_token/internal`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ app_id: config.appId, app_secret: config.appSecret })
    });
    const body = await res.json();
    if (!res.ok || body?.code !== 0) {
      throw new Error(
        `tenant_access_token failed: HTTP ${res.status} code=${body?.code} ${body?.msg}`
      );
    }
    cachedTenantToken = body.tenant_access_token;
    // Lark advertises ~2h expiry; refresh 60 s early to defeat clock skew. If
    // `expire` is missing, conservatively assume 7100 s (just under 2 h).
    const expireSec = typeof body.expire === "number" ? body.expire : 7100;
    tenantTokenExpiresAt = Date.now() + Math.max(60, expireSec - 60) * 1000;
    return cachedTenantToken;
  })().finally(() => {
    tenantTokenInflight = null;
  });

  return tenantTokenInflight;
}

function requiredEnv(name) {
  const value = process.env[name];
  if (!value || !value.trim()) throw new Error(`${name} is required`);
  return value.trim();
}

function resolveLarkDomain(domain) {
  const normalized = String(domain || "feishu").toLowerCase();
  if (normalized === "lark") return Lark.Domain?.Lark || "https://open.larksuite.com";
  if (normalized === "feishu") return Lark.Domain?.Feishu || "https://open.feishu.cn";
  return domain;
}
