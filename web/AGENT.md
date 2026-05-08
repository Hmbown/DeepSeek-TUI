# Community Assistant Agent

The community assistant is a set of Cloudflare Cron Triggers that call `deepseek-v4-flash` to draft triage comments, PR reviews, stale-issue nudges, duplicate suggestions, and weekly digests. **It never posts to GitHub directly.** Every output is a draft staged in Workers KV for maintainer review.

## Architecture

```
Cloudflare Cron Triggers
  └─ worker.ts scheduled() handler
       ├─ */30 min  → triage (new issues) + pr-review (new PRs)
       ├─ daily     → stale (30d inactive) + dupes (embed-similarity scan)
       ├─ weekly    → digest (Mon 09:00 UTC)
       └─ 6h       → curate (Today's Dispatch — pre-existing)

Drafts stored in Workers KV:
  draft:triage:<issue-number>
  draft:pr-review:<pr-number>
  draft:stale:<issue-number>
  draft:dupes:<issue-number>
  draft:digest:<year>-W<week>

Usage logged to:
  usage:<YYYY-MM-DD>
```

## Cron schedule

| Expression | Frequency | Tasks |
|---|---|---|
| `0 */6 * * *` | Every 6 hours | Today's Dispatch (curate) |
| `*/30 * * * *` | Every 30 min | Issue triage + PR review |
| `0 0 * * *` | Daily 00:00 UTC | Stale issue nudges + duplicate detection |
| `0 9 * * 1` | Monday 09:00 UTC | Weekly digest |

## Voice constraints

All drafts follow these rules:

- Calm, factual, never breathless.
- Never uses first person plural ("we"/"我们") — the maintainer is one person.
- Never commits to timing, prioritisation, or merge intent.
- Never apologises on the maintainer's behalf.
- Cites specific files / line numbers / linked issues when discussing code.
- Ends with: "— drafted by community assistant, pending maintainer review"
- Chinese drafts end with: "— 由社区助理草拟，待维护者审阅"
- Chinese output is rewritten in zh-CN, not machine-translated.

## Cost guardrails

- Each cron invocation caps at ~30k input tokens and ~2k output tokens.
- Issue/PR bodies are truncated to 1000–4000 chars before sending to the model.
- Deduplication: `hasFreshDraft` checks if a draft already exists that's newer than the item's `updated_at`. Skips if so.
- Token usage is logged to `usage:<YYYY-MM-DD>` KV keys (retained 90 days).
- If `DEEPSEEK_API_KEY` is missing or the API errors, the cron returns 200 with `{ skipped: true, reason }` — never crashes, never retry-loops.

## Maintainer review surface

Access at `/admin?token=<MAINTAINER_TOKEN>`.

- Lists all pending drafts with source link, draft body, and three actions:
  - **Post as comment** — calls GitHub REST API using `MAINTAINER_GITHUB_PAT`
  - **Edit & post** — opens a textarea for editing before posting
  - **Discard** — removes the draft from KV
- The auth token is set via `MAINTAINER_TOKEN` env var. Access sets an `mt` cookie for the session.
- **Nothing posts to GitHub without an explicit maintainer click.**

## Environment variables

| Variable | Required | Purpose |
|---|---|---|
| `DEEPSEEK_API_KEY` | Yes | DeepSeek API key for the community agent |
| `GITHUB_TOKEN` | Optional | Fine-grained PAT for GitHub API (raises rate limit) |
| `CRON_SECRET` | Optional | Shared secret for manual cron invocation |
| `MAINTAINER_TOKEN` | Optional | Auth token for /admin panel |
| `MAINTAINER_GITHUB_PAT` | Optional | GitHub PAT with `issues:write` scope for posting comments |

## Initial deployment

One-time setup before the first `npm run deploy`:

1. **Create the KV namespaces:**
   ```bash
   npx wrangler kv namespace create CURATED_KV
   npx wrangler kv namespace create NEXT_INC_CACHE_KV
   ```
   Copy the returned `id` values and paste them into the matching
   `wrangler.jsonc` bindings, replacing each `"REPLACE_WITH_KV_ID"`.

2. **Set secrets:**
   ```bash
   npx wrangler secret put DEEPSEEK_API_KEY
   npx wrangler secret put MAINTAINER_TOKEN
   npx wrangler secret put MAINTAINER_GITHUB_PAT
   npx wrangler secret put CRON_SECRET
   ```

3. **(Optional) Raise GitHub rate limit:**
   ```bash
   npx wrangler secret put GITHUB_TOKEN
   ```

4. **Verify:**
   ```bash
   npm run predeploy   # checks KV ID is set
   npm run deploy      # builds + deploys
   ```

## Kill switch

To disable the community agent entirely:

1. Remove all cron triggers from `wrangler.jsonc` except the original `0 */6 * * *` (curate).
2. Redeploy: `npm run deploy`.

The curate cron (Today's Dispatch) continues working independently. Individual tasks remain callable manually for testing through `/api/cron?task=triage`, `/api/cron?task=pr-review`, etc.

To disable a specific cron task, remove its cron expression from `wrangler.jsonc` and redeploy.

## Bilingual output

Every draft contains both `bodyEn` (English) and `bodyZh` (Chinese zh-CN). The admin panel shows the version matching the current locale. The zh version is rewritten natively by the model, not translated from English.
