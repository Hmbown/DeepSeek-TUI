import test from "node:test";
import assert from "node:assert/strict";

import {
  activeTurnBlock,
  approvalCardJson,
  buildStreamCardJson,
  cardOperatorAllowed,
  codePointLen,
  commandAction,
  formatToolDisplay,
  isAllowed,
  lastLineSummary,
  markdownToCard,
  pairingRefusalText,
  parseApprovalDecisionArgs,
  parseBool,
  parseEnvText,
  parseCommand,
  parseList,
  parseTextContent,
  safeSliceByCodePoint,
  splitMessage,
  stripGroupPrefix,
  validateBridgeConfig
} from "../src/lib.mjs";

// ── shared with feishu-bridge (kept verbatim so regressions are spotted) ────

test("parseList trims empty values", () => {
  assert.deepEqual(parseList(" oc_1, ou_2 ,, "), ["oc_1", "ou_2"]);
});

test("parseBool accepts common truthy values", () => {
  assert.equal(parseBool("yes"), true);
  assert.equal(parseBool("0", true), false);
  assert.equal(parseBool(undefined, true), true);
});

test("parseTextContent reads Feishu JSON text content", () => {
  assert.equal(parseTextContent(JSON.stringify({ text: "hello" })), "hello");
});

test("parseEnvText handles comments, export, and quoted values", () => {
  assert.deepEqual(
    parseEnvText(`
      # ignored
      export FEISHU_DOMAIN="lark"
      DEEPSEEK_WORKSPACE='/opt/deepseek'
    `),
    {
      FEISHU_DOMAIN: "lark",
      DEEPSEEK_WORKSPACE: "/opt/deepseek"
    }
  );
});

test("stripGroupPrefix requires prefix in group chats", () => {
  assert.deepEqual(
    stripGroupPrefix("/ds inspect this", {
      chatType: "group",
      requirePrefix: true,
      prefix: "/ds"
    }),
    { accepted: true, text: "inspect this" }
  );
  assert.equal(
    stripGroupPrefix("inspect this", {
      chatType: "group",
      requirePrefix: true,
      prefix: "/ds"
    }).accepted,
    false
  );
});

test("stripGroupPrefix accepts DM text without group prefix", () => {
  assert.deepEqual(
    stripGroupPrefix("inspect this", {
      chatType: "p2p",
      requirePrefix: true,
      prefix: "/ds"
    }),
    { accepted: true, text: "inspect this" }
  );
});

test("parseCommand distinguishes prompts and slash commands", () => {
  assert.deepEqual(parseCommand("hello"), { name: "prompt", args: "hello" });
  assert.deepEqual(parseCommand("/allow abc remember"), {
    name: "allow",
    args: "abc remember"
  });
});

test("commandAction maps bridge commands and falls back to prompts", () => {
  assert.deepEqual(commandAction(parseCommand("/status")), { kind: "status" });
  assert.deepEqual(commandAction(parseCommand("/resume thread-1")), {
    kind: "resume",
    threadId: "thread-1"
  });
  assert.deepEqual(commandAction(parseCommand("/unknown value")), {
    kind: "prompt",
    prompt: "/unknown value"
  });
});

test("parseApprovalDecisionArgs extracts remember flag", () => {
  assert.deepEqual(parseApprovalDecisionArgs("ap_123 remember"), {
    approvalId: "ap_123",
    remember: true
  });
  assert.deepEqual(parseApprovalDecisionArgs(""), { approvalId: "", remember: false });
});

test("isAllowed checks chat and user identifiers", () => {
  assert.equal(isAllowed({ chatId: "oc_x", openId: "ou_y" }, ["ou_y"], false), true);
  assert.equal(isAllowed({ chatId: "oc_x" }, [], false), false);
  assert.equal(isAllowed({ chatId: "oc_x" }, [], true), true);
});

test("pairingRefusalText includes allowlist identifiers", () => {
  const body = pairingRefusalText({
    chatId: "oc_chat",
    openId: "ou_user",
    unionId: "on_union",
    userId: "u_user"
  });
  assert.match(body, /chat_id=oc_chat/);
  assert.match(body, /open_id=ou_user/);
  assert.match(body, /union_id=on_union/);
  assert.match(body, /user_id=u_user/);
});

test("activeTurnBlock reports active queued or in-progress turn", () => {
  assert.equal(activeTurnBlock({ turns: [{ id: "done", status: "completed" }] }), null);
  assert.deepEqual(
    activeTurnBlock({
      turns: [
        { id: "old", status: "completed" },
        { id: "turn-2", status: "in_progress" }
      ]
    }),
    {
      turnId: "turn-2",
      message: "Thread already has active turn turn-2. Wait for it to finish or send /interrupt."
    }
  );
});

test("splitMessage chunks long text", () => {
  assert.deepEqual(splitMessage("abcdef", 2), ["ab", "cd", "ef"]);
});

test("splitMessage splits by code points, not UTF-16 units (no torn emoji)", () => {
  // Without code-point-safe splitting, "a🌟b" splits as ["a\uD83C", "\uDF1Fb"]
  // — two malformed strings with half a surrogate pair each. With the
  // helpers, the emoji stays intact: ["a🌟", "b"].
  const chunks = splitMessage("a🌟b", 2);
  assert.deepEqual(chunks, ["a🌟", "b"]);
  for (const chunk of chunks) {
    // Every chunk must round-trip through JSON.stringify (a torn surrogate
    // pair would be visible here as a replacement character).
    assert.equal(JSON.parse(JSON.stringify(chunk)), chunk);
  }
});

test("splitMessage handles CJK supplementary plane characters", () => {
  // 𠮷 is a supplementary-plane CJK ideograph (code point U+20BB7, two
  // UTF-16 code units). With UTF-16 slicing at 1, it splits in half.
  const chunks = splitMessage("a𠮷b", 2);
  assert.equal(chunks.length, 2);
  assert.equal(chunks[0], "a𠮷");
  assert.equal(chunks[1], "b");
});

test("validateBridgeConfig accepts locked-down DM config", () => {
  const result = validateBridgeConfig(
    {
      FEISHU_APP_ID: "cli_valid",
      FEISHU_APP_SECRET: "secret",
      FEISHU_DOMAIN: "lark",
      DEEPSEEK_RUNTIME_URL: "http://127.0.0.1:7878",
      DEEPSEEK_RUNTIME_TOKEN: "token-a",
      DEEPSEEK_WORKSPACE: "/opt/deepseek",
      DEEPSEEK_CHAT_ALLOWLIST: "oc_allowed",
      DEEPSEEK_ALLOW_UNLISTED: "false",
      FEISHU_THREAD_MAP_PATH: "/var/lib/deepseek-feishu-helm/thread-map.json",
      FEISHU_ALLOW_GROUPS: "false",
      FEISHU_REQUIRE_PREFIX_IN_GROUP: "true"
    },
    {
      workspaceRoot: "/opt/deepseek",
      runtimeEnv: {
        DEEPSEEK_RUNTIME_TOKEN: "token-a",
        DEEPSEEK_API_KEY: "sk-valid",
        DEEPSEEK_RUNTIME_PORT: "7878"
      }
    }
  );
  assert.equal(result.ok, true);
  assert.equal(result.errors.length, 0);
});

test("validateBridgeConfig rejects unsafe group pairing and token mismatch", () => {
  const result = validateBridgeConfig(
    {
      FEISHU_APP_ID: "cli_valid",
      FEISHU_APP_SECRET: "secret",
      FEISHU_DOMAIN: "feishu",
      DEEPSEEK_RUNTIME_URL: "http://127.0.0.1:7878",
      DEEPSEEK_RUNTIME_TOKEN: "bridge-token",
      DEEPSEEK_WORKSPACE: "/opt/deepseek",
      DEEPSEEK_ALLOW_UNLISTED: "true",
      FEISHU_THREAD_MAP_PATH: "/var/lib/deepseek-feishu-helm/thread-map.json",
      FEISHU_ALLOW_GROUPS: "true",
      FEISHU_REQUIRE_PREFIX_IN_GROUP: "false"
    },
    {
      workspaceRoot: "/opt/deepseek",
      runtimeEnv: {
        DEEPSEEK_RUNTIME_TOKEN: "runtime-token",
        DEEPSEEK_API_KEY: "replace-with-deepseek-platform-key"
      }
    }
  );
  assert.equal(result.ok, false);
  const codes = result.errors.map((item) => item.code).join(",");
  assert.match(codes, /open_group_control/);
  assert.match(codes, /token_mismatch/);
  const warns = result.warnings.map((item) => item.code).join(",");
  assert.match(warns, /group_without_prefix/);
});

test("validateBridgeConfig rejects placeholder DEEPSEEK_RUNTIME_TOKEN", () => {
  const base = {
    FEISHU_APP_ID: "cli_valid",
    FEISHU_APP_SECRET: "secret",
    DEEPSEEK_RUNTIME_URL: "http://127.0.0.1:7878",
    DEEPSEEK_WORKSPACE: "/opt/deepseek",
    FEISHU_THREAD_MAP_PATH: "/var/lib/deepseek-feishu-helm/thread-map.json"
  };
  for (const placeholder of ["your_token_here", "replace-with-long-random-token", ""]) {
    const result = validateBridgeConfig({ ...base, DEEPSEEK_RUNTIME_TOKEN: placeholder });
    assert.equal(result.ok, false, `placeholder "${placeholder}" should fail`);
  }
});

test("validateBridgeConfig rejects non-localhost DEEPSEEK_RUNTIME_URL", () => {
  const base = {
    FEISHU_APP_ID: "cli_valid",
    FEISHU_APP_SECRET: "secret",
    DEEPSEEK_RUNTIME_TOKEN: "token-a",
    DEEPSEEK_WORKSPACE: "/opt/deepseek",
    FEISHU_THREAD_MAP_PATH: "/var/lib/deepseek-feishu-helm/thread-map.json"
  };
  const result = validateBridgeConfig({
    ...base,
    DEEPSEEK_RUNTIME_URL: "https://example.com:7878"
  });
  assert.equal(result.ok, false);
  assert.match(result.errors.map((item) => item.code).join(","), /remote_runtime_url/);
});

// ── feishu-helm specific ────────────────────────────────────────────────────

test("validateBridgeConfig warns when approval_cards is on without use_cards", () => {
  const base = {
    FEISHU_APP_ID: "cli_valid",
    FEISHU_APP_SECRET: "secret",
    DEEPSEEK_RUNTIME_URL: "http://127.0.0.1:7878",
    DEEPSEEK_RUNTIME_TOKEN: "token-a",
    DEEPSEEK_WORKSPACE: "/opt/deepseek",
    FEISHU_THREAD_MAP_PATH: "/var/lib/deepseek-feishu-helm/thread-map.json",
    DEEPSEEK_CHAT_ALLOWLIST: "oc_allowed",
    FEISHU_USE_CARDS: "false",
    FEISHU_APPROVAL_CARDS: "true"
  };
  const result = validateBridgeConfig(base);
  assert.match(
    result.warnings.map((item) => item.code).join(","),
    /approval_cards_without_cards/
  );
});

test("validateBridgeConfig rejects non-https cardkit base url", () => {
  const base = {
    FEISHU_APP_ID: "cli_valid",
    FEISHU_APP_SECRET: "secret",
    DEEPSEEK_RUNTIME_URL: "http://127.0.0.1:7878",
    DEEPSEEK_RUNTIME_TOKEN: "token-a",
    DEEPSEEK_WORKSPACE: "/opt/deepseek",
    FEISHU_THREAD_MAP_PATH: "/var/lib/deepseek-feishu-helm/thread-map.json",
    FEISHU_CARDKIT_BASE_URL: "http://open.feishu.cn/open-apis"
  };
  const result = validateBridgeConfig(base);
  assert.equal(result.ok, false);
  assert.match(result.errors.map((item) => item.code).join(","), /insecure_cardkit_url/);
});

test("codePointLen counts emoji as one", () => {
  assert.equal(codePointLen("a🌟b"), 3);
  assert.equal(codePointLen("hello"), 5);
  assert.equal(codePointLen(""), 0);
});

test("safeSliceByCodePoint preserves emoji and CJK", () => {
  assert.equal(safeSliceByCodePoint("hi🌟world", 0, 3), "hi🌟");
  assert.equal(safeSliceByCodePoint("你好世界", 1, 3), "好世");
});

test("lastLineSummary keeps short text intact", () => {
  assert.equal(lastLineSummary("short", 50), "short");
});

test("lastLineSummary returns last non-empty line on overflow", () => {
  assert.equal(lastLineSummary("first\n\nlast line", 5), "last ...");
});

test("formatToolDisplay summarizes common parameter shapes", () => {
  assert.equal(formatToolDisplay(JSON.stringify({ path: "/x/y" })), "path=/x/y");
  assert.equal(
    formatToolDisplay(JSON.stringify({ prompt: "hello world" })),
    "prompt=hello world..."
  );
  assert.equal(formatToolDisplay(JSON.stringify({ name: "do-thing" })), "name=do-thing");
  assert.equal(formatToolDisplay(JSON.stringify({ number: 42 })), "#42");
  assert.equal(formatToolDisplay(JSON.stringify({ command: "ls -la" })), "cmd=ls -la");
  assert.equal(formatToolDisplay("not-json"), "");
  assert.equal(
    formatToolDisplay(JSON.stringify({ a: "alpha", b: "beta", c: "gamma" })),
    "a=alpha, b=beta, c=gamma"
  );
});

test("buildStreamCardJson includes streaming config knobs", () => {
  const json = buildStreamCardJson("agent_msg", { print_frequency_ms: 30, print_step: 5 });
  const card = JSON.parse(json);
  assert.equal(card.schema, "2.0");
  assert.equal(card.config.streaming_mode, true);
  assert.equal(card.config.streaming_config.print_frequency_ms.default, 30);
  assert.equal(card.config.streaming_config.print_step.default, 5);
  assert.equal(card.body.elements[0].tag, "markdown");
  assert.equal(card.body.elements[0].element_id, "agent_msg");
  assert.equal(card.body.elements[0].content, "");
});

test("markdownToCard wraps text in a single markdown element", () => {
  const json = markdownToCard("# heading\n\nbody");
  const card = JSON.parse(json);
  assert.equal(card.body.elements.length, 1);
  assert.equal(card.body.elements[0].tag, "markdown");
  assert.equal(card.body.elements[0].content, "# heading\n\nbody");
});

test("approvalCardJson surfaces approve / deny buttons with the approval_id", () => {
  const json = approvalCardJson({
    approval_id: "ap_abc",
    tool_name: "shell.exec",
    description: "rm -rf foo"
  });
  const card = JSON.parse(json);
  const actions = card.body.elements.find((e) => e.tag === "action").actions;
  assert.equal(actions.length, 2);
  const allow = actions.find((a) => a.value.action === "allow");
  const deny = actions.find((a) => a.value.action === "deny");
  assert.equal(allow.value.approval_id, "ap_abc");
  assert.equal(deny.value.approval_id, "ap_abc");
  assert.equal(deny.type, "danger");
});

test("cardOperatorAllowed gates approve / deny actions by the allowlist", () => {
  const event = {
    chat_id: "oc_chat",
    operator: { open_id: "ou_alice" }
  };
  assert.equal(cardOperatorAllowed(event, ["ou_alice"], false), true);
  assert.equal(cardOperatorAllowed(event, ["ou_other"], false), false);
  assert.equal(cardOperatorAllowed(event, [], true), true);
  assert.equal(cardOperatorAllowed(event, [], false), false);
});
