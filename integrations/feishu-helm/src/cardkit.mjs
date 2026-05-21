// CardKit v1 streaming-update client. Only used when FEISHU_USE_CARDS=true.
// Wraps three Feishu CardKit endpoints:
//   POST  /cardkit/v1/cards                          — create card entity
//   PUT   /cardkit/v1/cards/:id/elements/:eid/content — streaming text update
//   PATCH /cardkit/v1/cards/:id/settings              — update card config
//
// Streaming rules (per the public doc):
//   - `sequence` must strictly increase on each operation
//   - `content` is the FULL accumulated text — Feishu computes the diff
//   - If the new text is a prefix-superset of the previous text, the
//     typewriter effect continues; otherwise Feishu drops the animation and
//     renders a full replacement. Never tail-truncate (`slice(-n)`).
//
// Refs:
//   https://open.feishu.cn/document/cardkit-v1/streaming-updates-openapi-overview
//   https://open.feishu.cn/document/cardkit-v1/card-element/content
//   https://open.feishu.cn/document/cardkit-v1/card/settings

import { buildStreamCardJson, codePointLen, safeSliceByCodePoint } from "./lib.mjs";

/**
 * @typedef {Object} CardKitConfig
 * @property {string} baseUrl - https://open.feishu.cn/open-apis (or Lark equivalent)
 */

/**
 * @typedef {Object} StreamingConfig
 * @property {number} [print_frequency_ms]
 * @property {number} [print_step]
 * @property {'fast'|'delay'} [print_strategy]
 */

/**
 * @typedef {Object} StreamCardOptions
 * @property {string} [elementId]
 * @property {StreamingConfig} [streamingConfig]
 */

/**
 * @typedef {Object} CardCreateResult
 * @property {string} card_id
 * @property {string} element_id
 */

export class CardKitClient {
  /**
   * @param {CardKitConfig} config
   * @param {() => Promise<string>} getToken — function returning a fresh tenant_access_token
   * @param {(msg: string) => void} [logger]
   */
  constructor(config, getToken, logger = () => {}) {
    this.baseUrl = String(config.baseUrl || "https://open.feishu.cn/open-apis").replace(/\/$/, "");
    this.getToken = getToken;
    this.log = logger;
  }

  /**
   * Create a card entity with streaming_mode enabled.
   * @param {StreamCardOptions} [opts]
   * @returns {Promise<CardCreateResult>}
   */
  async createStreamCard(opts = {}) {
    const elementId = opts.elementId ?? "stream_content";
    const streamingConfig = opts.streamingConfig ?? {};
    const cardJson = buildStreamCardJson(elementId, streamingConfig);

    const token = await this.getToken();
    const url = `${this.baseUrl}/cardkit/v1/cards`;
    this.log(`[cardkit] POST ${url} element_id=${elementId}`);

    const res = await fetch(url, {
      method: "POST",
      headers: {
        authorization: `Bearer ${token}`,
        "content-type": "application/json; charset=utf-8"
      },
      body: JSON.stringify({ type: "card_json", data: cardJson })
    });

    const body = await readJsonSafe(res);
    if (!res.ok || body?.code !== 0) {
      const msg = `CardKit create failed: HTTP ${res.status} code=${body?.code} msg=${body?.msg}`;
      this.log(`[cardkit] ${msg}`);
      throw new Error(msg);
    }
    const cardId = body?.data?.card_id;
    if (!cardId) throw new Error("CardKit create: no card_id in response");
    this.log(`[cardkit] created card_id=${cardId} element_id=${elementId}`);
    return { card_id: cardId, element_id: elementId };
  }

  /**
   * Stream-update the text content of an element. `content` MUST be the full
   * accumulated text — Feishu computes the diff. Sequence must strictly
   * increase across calls for the same card.
   *
   * @param {string} cardId
   * @param {string} elementId
   * @param {string} content     full accumulated text (not delta)
   * @param {number} sequence    strictly-increasing op counter
   * @param {string} [uuid]      optional idempotency key
   */
  async updateContent(cardId, elementId, content, sequence, uuid) {
    const token = await this.getToken();
    const url = `${this.baseUrl}/cardkit/v1/cards/${encodeURIComponent(cardId)}/elements/${encodeURIComponent(elementId)}/content`;

    // CardKit caps content at 100k code points. Always keep PREFIX intact: if
    // the new text isn't a prefix-superset of the previous, Feishu drops the
    // typewriter animation and renders a full replacement.
    //
    // JS `.length` and `.slice()` operate on UTF-16 code units, which corrupts
    // emoji and supplementary-plane CJK at the cap boundary. Use code-point
    // helpers so 100000 means 100000 user-visible characters.
    const cpLen = codePointLen(content);
    const safeContent = cpLen > 100000 ? safeSliceByCodePoint(content, 0, 100000) : content;
    if (cpLen > 100000) {
      this.log(`[cardkit] content truncated from ${cpLen} to 100000 code points`);
    }

    const body = { content: safeContent, sequence };
    if (uuid) body.uuid = uuid;

    const res = await fetch(url, {
      method: "PUT",
      headers: {
        authorization: `Bearer ${token}`,
        "content-type": "application/json; charset=utf-8"
      },
      body: JSON.stringify(body)
    });

    const resp = await readJsonSafe(res);
    if (!res.ok || resp?.code !== 0) {
      this.log(`[cardkit] updateContent failed: HTTP ${res.status} code=${resp?.code} msg=${resp?.msg} seq=${sequence}`);
      throw new Error(`CardKit updateContent: code=${resp?.code} ${resp?.msg}`);
    }
  }

  /**
   * Update card settings — used to close streaming_mode after the final flush.
   * Non-fatal on failure: CardKit auto-closes streaming after 10 minutes.
   *
   * @param {string} cardId
   * @param {Record<string, unknown>} settings
   * @param {number} sequence
   * @param {string} [uuid]
   */
  async updateSettings(cardId, settings, sequence, uuid) {
    const token = await this.getToken();
    const url = `${this.baseUrl}/cardkit/v1/cards/${encodeURIComponent(cardId)}/settings`;

    const body = { settings: JSON.stringify(settings), sequence };
    if (uuid) body.uuid = uuid;

    const res = await fetch(url, {
      method: "PATCH",
      headers: {
        authorization: `Bearer ${token}`,
        "content-type": "application/json; charset=utf-8"
      },
      body: JSON.stringify(body)
    });

    const resp = await readJsonSafe(res);
    if (!res.ok || resp?.code !== 0) {
      // Non-fatal — let the daemon continue. Document this asymmetry vs
      // updateContent which throws.
      this.log(`[cardkit] updateSettings failed: HTTP ${res.status} code=${resp?.code} msg=${resp?.msg} seq=${sequence}`);
    }
  }

  /**
   * Deliver a previously-created card entity to a Feishu chat as a msg_type=card
   * message. Returns the Feishu message_id on success.
   *
   * @param {string} chatId
   * @param {string} cardId
   * @returns {Promise<string|null>}
   */
  async sendCardToChat(chatId, cardId) {
    const token = await this.getToken();
    const url = `${this.baseUrl}/im/v1/messages?receive_id_type=chat_id`;
    const content = JSON.stringify({ type: "card", data: { card_id: cardId } });
    this.log(`[cardkit] sending card ${cardId} to chat ${chatId}`);

    const res = await fetch(url, {
      method: "POST",
      headers: {
        authorization: `Bearer ${token}`,
        "content-type": "application/json; charset=utf-8"
      },
      body: JSON.stringify({
        receive_id: chatId,
        msg_type: "interactive",
        content
      })
    });

    const body = await readJsonSafe(res);
    if (!res.ok || body?.code !== 0) {
      this.log(`[cardkit] sendCardToChat failed: HTTP ${res.status} code=${body?.code} msg=${body?.msg}`);
      return null;
    }
    const msgId = body?.data?.message_id;
    this.log(`[cardkit] sent card ${cardId} to chat ${chatId} → message_id=${msgId ?? "?"}`);
    return msgId ?? null;
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
