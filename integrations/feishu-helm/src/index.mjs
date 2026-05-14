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
  compactRuntimeError,
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
// daemon naturally serializes turns by awaiting each `runPrompt`.
/** @type {Map<string, {chatId: string, text: string, cardId: string | null, elementId: string, sequence: number, closed: boolean}>} */
const streamingByThread = new Map();

// ── SSE callbacks ───────────────────────────────────────────────────────────

sse.onDelta = (threadId, delta) => {
  if (delta.kind !== "agent_message") return;
  const st = streamingByThread.get(threadId);
  if (!st) return;
  st.text += delta.delta;
  if (config.useCards && st.cardId && !st.closed) {
    flushStreamingCard(st).catch((err) => {
      console.error(`[helm] flush streaming card failed: ${err?.message ?? err}`);
    });
  }
};

sse.onItemStarted = (threadId, item) => {
  if (item.kind !== "agent_message") return;
  const st = streamingByThread.get(threadId);
  if (!st || !config.useCards) return;
  // Lazy-create the streaming card on the first agent_message item.
  if (!st.cardId && cardkit) {
    void initStreamingCard(st).catch((err) => {
      console.error(`[helm] init streaming card failed: ${err?.message ?? err}`);
    });
  }
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
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), config.turnTimeoutMs);

  streamingByThread.set(threadId, {
    chatId,
    text: "",
    cardId: null,
    elementId: "agent_msg",
    sequence: 0,
    closed: false
  });

  let latestSeq = sinceSeq;
  let sentProgressAt = Date.now();

  try {
    const response = await fetch(
      `${config.runtimeUrl}/v1/threads/${encodeURIComponent(threadId)}/events?since_seq=${sinceSeq}`,
      {
        headers: { authorization: `Bearer ${config.runtimeToken}` },
        signal: controller.signal
      }
    );
    if (!response.ok) {
      const body = await readJsonSafe(response);
      throw new Error(compactRuntimeError(response.status, body));
    }

    for await (const record of readSse(response)) {
      latestSeq = Math.max(latestSeq, Number(record.seq || 0));
      await threadStore.patchChat(chatId, { lastSeq: latestSeq });

      if (turnId && record.turn_id && record.turn_id !== turnId) continue;

      if (record.event === "item.delta" && record.payload?.kind === "agent_message") {
        const st = streamingByThread.get(threadId);
        if (!st) continue;
        st.text += record.payload.delta || "";
        if (config.useCards) {
          // Lazy-create the streaming card on first delta.
          if (!st.cardId && cardkit) {
            await initStreamingCard(st);
          }
          if (st.cardId && !st.closed) {
            await flushStreamingCard(st);
          }
        } else if (st.text.length > config.maxReplyChars && Date.now() - sentProgressAt > 15000) {
          // Plaintext mode: chunked progress sends, matching feishu-bridge semantics.
          await sendText(chatId, st.text.slice(0, config.maxReplyChars));
          st.text = st.text.slice(config.maxReplyChars);
          sentProgressAt = Date.now();
        }
      }

      if (record.event === "approval.required") {
        await deliverApproval(chatId, record.payload || {});
      }

      if (record.event === "turn.lifecycle") {
        const status = record.payload?.turn?.status || record.payload?.status;
        if (["failed", "canceled", "interrupted"].includes(status)) {
          await closeStreamingCard(threadId);
          await sendText(chatId, `Turn ${status}.`);
          return;
        }
      }

      if (record.event === "turn.completed") {
        const turn = record.payload?.turn || {};
        const status = turn.status || "completed";
        const error = turn.error ? `\n${turn.error}` : "";
        await closeStreamingCard(threadId);
        const st = streamingByThread.get(threadId);
        const finalText = st?.text?.trim() || "Turn completed.";
        if (status !== "completed") {
          await sendText(chatId, `Turn ${status}.${error}`.trim());
        } else if (config.useCards) {
          // If we never created a streaming card (e.g. zero deltas), still
          // surface the final text as a non-streaming card.
          if (st && !st.cardId) {
            await sendCard(chatId, markdownToCard(finalText));
          }
        } else {
          await sendText(chatId, finalText);
        }
        return;
      }
    }
  } catch (err) {
    if (err?.name === "AbortError") {
      await sendText(chatId, `Turn timed out after ${Math.round(config.turnTimeoutMs / 1000)}s.`);
      return;
    }
    throw err;
  } finally {
    await closeStreamingCard(threadId);
    streamingByThread.delete(threadId);
    clearTimeout(timeout);
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

async function tenantAccessToken() {
  // Used by CardKitClient — fetch fresh tenant_access_token per call.
  // Lark's official SDK does cache internally for im.message but the cardkit
  // endpoints are not in the SDK, so we mint our own bearer here.
  const res = await fetch(`${config.cardkitBaseUrl}/auth/v3/tenant_access_token/internal`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ app_id: config.appId, app_secret: config.appSecret })
  });
  const body = await res.json();
  if (!res.ok || body?.code !== 0) {
    throw new Error(`tenant_access_token failed: HTTP ${res.status} code=${body?.code} ${body?.msg}`);
  }
  return body.tenant_access_token;
}

// ── SSE line reader (request-scoped; the long-running SseEventHandler is
// reserved for callers that want auto-reconnect, e.g. background SSE.) ──────

async function* readSse(response) {
  const decoder = new TextDecoder();
  let buffer = "";
  for await (const chunk of response.body) {
    buffer += decoder.decode(chunk, { stream: true });
    let boundary;
    while ((boundary = buffer.indexOf("\n\n")) >= 0) {
      const raw = buffer.slice(0, boundary).replace(/\r/g, "");
      buffer = buffer.slice(boundary + 2);
      let event = { event: "", data: "" };
      for (const line of raw.split("\n")) {
        if (line.startsWith("event:")) event.event = line.slice(6).trim();
        if (line.startsWith("data:")) event.data += line.slice(5).trim();
      }
      if (!event.data) continue;
      let parsed;
      try {
        parsed = JSON.parse(event.data);
      } catch {
        continue;
      }
      yield parsed;
    }
  }
}

async function readJsonSafe(response) {
  const text = await response.text();
  if (!text) return {};
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
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
