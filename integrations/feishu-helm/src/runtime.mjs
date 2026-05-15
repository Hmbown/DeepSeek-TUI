// Runtime API client + SSE event handler for deepseek serve --http.
// This module is the differentiator from feishu-bridge: it adds
// since_seq + exponential-backoff reconnect to the SSE stream so the bridge
// survives `deepseek serve` restarts and brief network blips without losing
// events.
//
// Verified against deepseek 0.8.31 serve --http --port 7878.

import fs from "node:fs/promises";
import path from "node:path";

import { compactRuntimeError } from "./lib.mjs";

// ── RuntimeClient ───────────────────────────────────────────────────────────

export class RuntimeClient {
  /**
   * @param {{baseUrl: string, token: string}} cfg
   */
  constructor(cfg) {
    this.baseUrl = String(cfg.baseUrl).replace(/\/+$/, "");
    this.token = cfg.token;
  }

  authHeaders() {
    return { authorization: `Bearer ${this.token}` };
  }

  /**
   * @param {string} route
   * @param {{method?: string, body?: any, auth?: boolean, signal?: AbortSignal}} [options]
   */
  async json(route, options = {}) {
    const res = await fetch(`${this.baseUrl}${route}`, {
      method: options.method || "GET",
      headers: {
        ...(options.auth === false ? {} : this.authHeaders()),
        ...(options.body ? { "content-type": "application/json" } : {})
      },
      body: options.body ? JSON.stringify(options.body) : undefined,
      signal: options.signal
    });
    const body = await readJsonSafe(res);
    if (!res.ok) {
      throw new Error(compactRuntimeError(res.status, body));
    }
    return body;
  }

  /**
   * Open the SSE event stream for a thread. Returns the raw fetch Response;
   * caller iterates `response.body` via `readSse`.
   *
   * @param {string} threadId
   * @param {number} [sinceSeq]
   * @param {AbortSignal} [signal]
   */
  async eventsStream(threadId, sinceSeq = 0, signal) {
    let url = `${this.baseUrl}/v1/threads/${encodeURIComponent(threadId)}/events`;
    if (sinceSeq > 0) url += `?since_seq=${sinceSeq}`;
    const res = await fetch(url, {
      headers: { accept: "text/event-stream", ...this.authHeaders() },
      signal
    });
    if (!res.ok) {
      const body = await readJsonSafe(res);
      throw new Error(compactRuntimeError(res.status, body));
    }
    return res;
  }
}

// ── ThreadStore ─────────────────────────────────────────────────────────────
//
// File-backed map of Feishu chat → runtime thread state, plus a bounded
// recent-message-id list for redelivery dedup. Mirrors the upstream
// feishu-bridge ThreadStore semantics: atomic tmp+rename writes, 0o700 dir,
// 0o600 file. Message dedup is persistent (capped at 200) so a WS redelivery
// after restart does NOT create a duplicate turn.

export class ThreadStore {
  static async open(filePath) {
    const store = new ThreadStore(filePath);
    await store.load();
    return store;
  }

  constructor(filePath) {
    this.filePath = filePath;
    this.data = { chats: {}, messages: [] };
    // Serialize concurrent save() calls via a promise chain. Without this,
    // two awaited writers race on the shared `${filePath}.tmp` and one
    // rename(2) can clobber the other's tmp file before it lands, producing
    // partial writes. Chaining means each save() awaits the previous one's
    // rename before starting its own write.
    /** @type {Promise<void>} */
    this._saveChain = Promise.resolve();
  }

  async load() {
    try {
      const raw = await fs.readFile(this.filePath, "utf8");
      const parsed = JSON.parse(raw);
      this.data = {
        chats: parsed?.chats && typeof parsed.chats === "object" ? parsed.chats : {},
        messages: Array.isArray(parsed?.messages) ? parsed.messages : []
      };
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }

  /**
   * Record an incoming Feishu message_id. Returns true if we've seen it
   * before — the caller MUST then bail to defeat redelivery.
   */
  async recordMessage(messageId) {
    if (!messageId) return false;
    if (this.data.messages.includes(messageId)) return true;
    this.data.messages.push(messageId);
    if (this.data.messages.length > 200) {
      this.data.messages = this.data.messages.slice(-200);
    }
    await this.save();
    return false;
  }

  async getChat(chatId) {
    return this.data.chats[chatId] || null;
  }

  async setChat(chatId, state) {
    this.data.chats[chatId] = state;
    await this.save();
    return state;
  }

  async patchChat(chatId, patch) {
    const current = this.data.chats[chatId] || {};
    this.data.chats[chatId] = { ...current, ...patch };
    await this.save();
    return this.data.chats[chatId];
  }

  save() {
    // Snapshot serialised data NOW so the on-disk image reflects this caller's
    // observed state, even if a later mutation runs before its turn at the
    // head of _saveChain.
    const snapshot = `${JSON.stringify(this.data, null, 2)}\n`;
    const next = this._saveChain.then(async () => {
      const dir = path.dirname(this.filePath);
      await fs.mkdir(dir, { recursive: true, mode: 0o700 });
      const tmp = `${this.filePath}.tmp`;
      await fs.writeFile(tmp, snapshot, { mode: 0o600 });
      await fs.rename(tmp, this.filePath);
    });
    // Swallow rejections in the chain itself so one failed write doesn't
    // poison every subsequent save(); callers still see the original
    // rejection via their own awaited promise.
    this._saveChain = next.catch(() => {});
    return next;
  }
}

// ── SSE handler with since_seq reconnect ───────────────────────────────────
//
// Differs from feishu-bridge's inline SSE reader in two ways:
//   1) On stream close, auto-reconnects with `?since_seq=<lastSeen>` and
//      exponential backoff (500ms → 30s cap). This means a `deepseek serve`
//      restart, a momentary network blip, or a server-side connection rotation
//      does not lose events.
//   2) Exposes a typed callback surface (onDelta, onApproval, onTurnCompleted,
//      onItemStarted, onItemCompleted, onItemFailed, onTurnLifecycle) so the
//      caller doesn't have to demultiplex the raw event stream.

export class SseEventHandler {
  /**
   * @param {RuntimeClient} runtime
   * @param {(msg: string) => void} [logger]
   */
  constructor(runtime, logger = () => {}) {
    this.runtime = runtime;
    this.log = logger;
    /** @type {Map<string, AbortController>} */
    this.connections = new Map();

    /** @type {((threadId: string, delta: {turn_id: string|null, kind: string, delta: string, item_id?: string}) => void) | null} */
    this.onDelta = null;
    /** @type {((threadId: string, turn: any) => void) | null} */
    this.onTurnCompleted = null;
    /** @type {((threadId: string, status: string, turn: any, turnId: string|null) => void) | null} */
    this.onTurnLifecycle = null;
    /** @type {((threadId: string, approval: any, turnId: string|null) => void) | null} */
    this.onApprovalRequired = null;
    /** @type {((threadId: string, item: any) => void) | null} */
    this.onItemStarted = null;
    /** @type {((threadId: string, item: any) => void) | null} */
    this.onItemCompleted = null;
    /** @type {((threadId: string, item: any) => void) | null} */
    this.onItemFailed = null;
    /** @type {((threadId: string, seq: number) => void) | null} */
    this.onSeqProgress = null;
  }

  /**
   * Connect to the SSE stream for `threadId`. Awaits the initial HTTP response
   * so callers can catch immediate auth/4xx failures; the read loop and any
   * reconnect attempts run in the background until `disconnect(threadId)` is
   * called.
   */
  async connect(threadId, sinceSeq = 0) {
    this.disconnect(threadId);
    const controller = new AbortController();
    this.connections.set(threadId, controller);

    let body;
    try {
      const res = await this.runtime.eventsStream(threadId, sinceSeq, controller.signal);
      if (!res.body) throw new Error("SSE response has no body");
      body = res.body;
    } catch (err) {
      this.connections.delete(threadId);
      throw err;
    }

    const lastSeqRef = { value: sinceSeq };
    void this.runReadLoop(threadId, body, controller, lastSeqRef);
  }

  async runReadLoop(threadId, initialBody, controller, lastSeqRef) {
    let body = initialBody;
    let attempt = 0;

    while (!controller.signal.aborted) {
      if (body) {
        try {
          await this.readStream(threadId, body, controller.signal, lastSeqRef);
          attempt = 0; // clean close — reset backoff
        } catch (err) {
          if (controller.signal.aborted) break;
          this.log(`[sse] thread ${threadId}: read error: ${err?.message ?? err}`);
        }
        body = null;
      }
      if (controller.signal.aborted) break;

      attempt += 1;
      const delay = Math.min(500 * Math.pow(2, Math.min(attempt - 1, 6)), 30000);
      this.log(`[sse] thread ${threadId}: reconnect in ${delay}ms (attempt ${attempt})`);
      await new Promise((r) => setTimeout(r, delay));
      if (controller.signal.aborted) break;

      try {
        const res = await this.runtime.eventsStream(threadId, lastSeqRef.value, controller.signal);
        if (res.body) {
          body = res.body;
          this.log(`[sse] thread ${threadId}: reconnected (since_seq=${lastSeqRef.value})`);
        }
      } catch (err) {
        if (controller.signal.aborted) break;
        this.log(`[sse] thread ${threadId}: reconnect failed: ${err?.message ?? err}`);
      }
    }
    this.connections.delete(threadId);
  }

  async readStream(threadId, body, signal, lastSeqRef) {
    const reader = body.getReader();
    const decoder = new TextDecoder("utf-8");
    let buffer = "";
    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) {
          if (buffer.length > 0) this.handleLine(threadId, buffer, lastSeqRef);
          return;
        }
        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split(/\r?\n/);
        buffer = lines.pop() ?? "";
        for (const line of lines) this.handleLine(threadId, line, lastSeqRef);
      }
    } catch (err) {
      if (signal.aborted) return;
      throw err;
    } finally {
      try { reader.releaseLock(); } catch {}
    }
  }

  handleLine(threadId, line, lastSeqRef) {
    const trimmed = line.trimEnd();
    if (!trimmed || trimmed.startsWith(":")) return;
    if (trimmed.startsWith("event: ")) return;
    if (!trimmed.startsWith("data: ")) return;
    let event;
    try {
      event = JSON.parse(trimmed.slice(6));
    } catch {
      return;
    }
    if (typeof event.seq === "number" && event.seq > lastSeqRef.value) {
      lastSeqRef.value = event.seq;
      if (this.onSeqProgress) {
        try {
          this.onSeqProgress(threadId, event.seq);
        } catch (err) {
          this.log(`[sse] onSeqProgress threw: ${err?.message ?? err}`);
        }
      }
    }
    this.dispatch(threadId, event);
  }

  dispatch(threadId, event) {
    const turnId = event.turn_id ?? null;
    switch (event.event) {
      case "item.delta":
        if (this.onDelta && event.payload) {
          this.onDelta(threadId, {
            turn_id: turnId,
            kind: event.payload.kind,
            delta: event.payload.delta ?? "",
            item_id: event.item_id ?? undefined
          });
        }
        return;
      case "turn.completed":
        if (this.onTurnCompleted && event.payload?.turn) {
          this.onTurnCompleted(threadId, event.payload.turn);
        }
        return;
      case "item.completed":
        if (this.onItemCompleted && event.payload?.item) {
          this.onItemCompleted(threadId, event.payload.item);
        }
        return;
      case "item.started":
        if (this.onItemStarted && event.payload?.item) {
          this.onItemStarted(threadId, event.payload.item);
        }
        return;
      case "item.failed":
        // Failed items go ONLY to onItemFailed. Falling through to
        // onItemCompleted would overwrite the failed status with `done`.
        if (this.onItemFailed && event.payload?.item) {
          this.onItemFailed(threadId, event.payload.item);
        }
        return;
      case "approval.required":
        if (this.onApprovalRequired) {
          this.onApprovalRequired(threadId, event.payload, turnId);
        }
        return;
      case "turn.lifecycle": {
        const status = event.payload?.turn?.status || event.payload?.status;
        if (this.onTurnLifecycle && status) {
          this.onTurnLifecycle(threadId, status, event.payload?.turn, turnId);
        }
        return;
      }
      default:
        return;
    }
  }

  disconnect(threadId) {
    const controller = this.connections.get(threadId);
    if (controller) {
      controller.abort();
      this.connections.delete(threadId);
    }
  }

  disconnectAll() {
    for (const [, controller] of this.connections.entries()) {
      controller.abort();
    }
    this.connections.clear();
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
