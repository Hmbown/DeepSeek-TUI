import { NextResponse } from "next/server";
import { getAgentEnv, getDraft, deleteDraft, validateSession, type CommunityAgentEnv } from "@/lib/community-agent";

export const dynamic = "force-dynamic";

async function checkAuth(req: Request, env: CommunityAgentEnv): Promise<{ ok: boolean; status?: number; error?: string }> {
  if (!env.MAINTAINER_TOKEN) {
    return { ok: false, status: 503, error: "MAINTAINER_TOKEN not configured" };
  }

  const cookieHeader = req.headers.get("cookie") ?? "";
  let sid: string | undefined;
  for (const c of cookieHeader.split(";")) {
    const [name, ...rest] = c.trim().split("=");
    if (name === "mt_sid") {
      sid = rest.join("=");
      break;
    }
  }

  if (!sid || !(await validateSession(env.CURATED_KV, sid))) {
    return { ok: false, status: 401, error: "unauthorized" };
  }
  return { ok: true };
}

export async function POST(req: Request) {
  const env = await getAgentEnv();
  const auth = await checkAuth(req, env);
  if (!auth.ok) {
    return NextResponse.json(
      { error: auth.error ?? "unauthorized" },
      { status: auth.status ?? 401, headers: { "Cache-Control": "no-store" } }
    );
  }

  const body = await req.json() as { action: string; draftKey: string; editedBody?: string; lang?: "en" | "zh" };
  const { action, draftKey, editedBody, lang } = body;

  if (!draftKey) {
    return NextResponse.json({ error: "missing draftKey" }, { status: 400 });
  }

  const draft = await getDraft(env.CURATED_KV, draftKey);
  if (!draft) {
    return NextResponse.json({ error: "draft not found" }, { status: 404 });
  }

  if (action === "discard") {
    await deleteDraft(env.CURATED_KV, draftKey);
    return NextResponse.json({ ok: true, action: "discarded" });
  }

  if (action === "post") {
    if (!env.MAINTAINER_GITHUB_PAT) {
      return NextResponse.json({ error: "MAINTAINER_GITHUB_PAT not configured" }, { status: 500 });
    }

    const commentBody = editedBody ?? (lang === "zh" ? draft.bodyZh : draft.bodyEn);

    if (draft.type === "digest") {
      return NextResponse.json({ ok: true, action: "digest-skipped", note: "Digest pages are not posted as comments" });
    }

    if (!draft.targetNumber) {
      return NextResponse.json({ error: "no target number" }, { status: 400 });
    }

    const repo = env.GITHUB_REPO ?? "Hmbown/deepseek-tui";
    const commentUrl = `https://api.github.com/repos/${repo}/issues/${draft.targetNumber}/comments`;

    const ghRes = await fetch(commentUrl, {
      method: "POST",
      headers: {
        Accept: "application/vnd.github+json",
        Authorization: `Bearer ${env.MAINTAINER_GITHUB_PAT}`,
        "X-GitHub-Api-Version": "2022-11-28",
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ body: commentBody }),
    });

    if (!ghRes.ok) {
      const text = await ghRes.text();
      return NextResponse.json({ error: `GitHub ${ghRes.status}: ${text}` }, { status: 502 });
    }

    // Mark as posted
    draft.posted = true;
    await env.CURATED_KV?.put(draftKey, JSON.stringify(draft), { expirationTtl: 60 * 60 * 24 * 7 });

    return NextResponse.json({ ok: true, action: "posted" });
  }

  return NextResponse.json({ error: "unknown action" }, { status: 400 });
}
