# Feishu / Lark Helm

A sibling integration to [`feishu-bridge`](../feishu-bridge/) that adds three
features on top of the same security model:

1. **Streaming markdown cards** — instead of plain text, agent replies stream
   into a Feishu / Lark CardKit card with a typewriter effect, so progress is
   visible character-by-character. Plain text is still the default; cards are
   opt-in via `FEISHU_USE_CARDS=true`.
2. **SSE auto-reconnect with `since_seq`** — on a `deepseek serve` restart or a
   transient network blip, the bridge reconnects with `?since_seq=<lastSeqSeen>`
   and exponential backoff (500 ms → 30 s cap), so no events are lost mid-turn.
3. **Approval card buttons** — when `FEISHU_APPROVAL_CARDS=true` (and cards are
   enabled), runtime approval requests are surfaced as Approve / Deny buttons
   on a card in addition to the `/allow` and `/deny` text commands. Operator
   identity is still checked against `DEEPSEEK_CHAT_ALLOWLIST` before the
   runtime sees the decision.

The command surface (`/status`, `/threads`, `/new`, `/resume <id>`,
`/interrupt`, `/compact`, `/allow <id> [remember]`, `/deny <id>`), the
allowlist / DM gating / group-prefix logic, the runtime-URL localhost
requirement, and the placeholder-token / token-mismatch validator rules are
all identical to `feishu-bridge`. Both bridges import the same shape of pure
helpers from their respective `src/lib.mjs` and pass the same
`validateBridgeConfig` checks plus three card-specific extensions.

Security model (unchanged from `feishu-bridge`):

- `deepseek serve --http` stays bound to `127.0.0.1`.
- `/v1/*` runtime calls require `DEEPSEEK_RUNTIME_TOKEN`.
- Feishu / Lark chats must be allowlisted unless `DEEPSEEK_ALLOW_UNLISTED=true`
  is set for first pairing.
- Direct messages are the intended control surface. Group chat control is
  disabled unless `FEISHU_ALLOW_GROUPS=true`, and group messages must carry
  the `FEISHU_GROUP_PREFIX` (default `/ds`) unless
  `FEISHU_REQUIRE_PREFIX_IN_GROUP=false`.
- Card-button approvals validate the *clicker's* identity (the
  `event.operator.open_id` / `union_id` / `user_id` carried on
  `card.action.trigger`) against the same allowlist text approvals use.
- `mode`, `auto_approve`, `trust_mode`, and `allow_shell` are env-driven; the
  defaults match `feishu-bridge` (`mode=agent`, `auto_approve=false`,
  `trust_mode=false`, `allow_shell=true`).

## Setup

```bash
cd /opt/deepseek/helm
npm install --omit=dev
cp .env.example /etc/deepseek/feishu-helm.env
sudoedit /etc/deepseek/feishu-helm.env
node src/index.mjs
```

Validate before starting:

```bash
npm run validate:config -- \
  --env /etc/deepseek/feishu-helm.env \
  --runtime-env /etc/deepseek/runtime.env \
  --workspace-root /opt/deepseek \
  --check-filesystem
```

A systemd unit symmetric to `deepseek-feishu-bridge.service` works:

```bash
sudo systemctl enable --now deepseek-runtime deepseek-feishu-helm
sudo journalctl -u deepseek-feishu-helm -f
```

## Commands

- `/status`
- `/threads`
- `/new`
- `/resume <thread_id>`
- `/interrupt`
- `/compact`
- `/allow <approval_id> [remember]`
- `/deny <approval_id>`

Anything else is sent as a prompt. In group chats with `FEISHU_ALLOW_GROUPS=true`
and the default `FEISHU_REQUIRE_PREFIX_IN_GROUP=true`, messages must start with
the configured prefix:

```text
/ds check git status and tell me what is dirty
```

## Opt-in card behavior

Cards are off by default; the bridge sends plain text just like
`feishu-bridge`. To enable:

```env
FEISHU_USE_CARDS=true
FEISHU_APPROVAL_CARDS=true       # requires FEISHU_USE_CARDS=true
FEISHU_CARDKIT_BASE_URL=https://open.feishu.cn/open-apis
```

When cards are enabled, an `agent_message` item streams text into a single
CardKit card via the streaming PUT `/content` endpoint, with strictly
increasing `sequence` numbers (as required by the public CardKit contract).
On `turn.completed` / `turn.lifecycle` failed / canceled, the card is closed
with `updateSettings({ config: { streaming_mode: false } })`. If creating the
card fails (CardKit unavailable, no `tenant_access_token`), the bridge falls
back to a plain-text reply for that turn so the user still sees the answer.

## Tests

```bash
npm test
```

Covers the shared pure helpers (parseList / parseBool / parseEnvText /
stripGroupPrefix / parseCommand / commandAction / parseApprovalDecisionArgs /
isAllowed / pairingRefusalText / activeTurnBlock / splitMessage /
validateBridgeConfig) plus the helm-specific helpers (codePointLen /
safeSliceByCodePoint / lastLineSummary / formatToolDisplay / buildStreamCardJson
/ markdownToCard / approvalCardJson / cardOperatorAllowed) and the extra
`validateBridgeConfig` rules for `FEISHU_USE_CARDS`, `FEISHU_APPROVAL_CARDS`,
and `FEISHU_CARDKIT_BASE_URL`.

## Choosing between feishu-bridge and feishu-helm

- Use `feishu-bridge` if you want the smallest possible attack surface, a
  phone-only DM control surface, and plain-text replies. It depends on nothing
  beyond `@larksuiteoapi/node-sdk`.
- Use `feishu-helm` if you want streaming markdown card output, button-based
  approvals, and resilient SSE event delivery. It also depends only on
  `@larksuiteoapi/node-sdk`; CardKit is reached over HTTP using the same
  `tenant_access_token` the SDK already mints.

Both bridges can connect to the same `deepseek serve --http`. Run only one at
a time against a given Feishu app id — they share the same WS event
subscription and would otherwise double-consume incoming messages.
