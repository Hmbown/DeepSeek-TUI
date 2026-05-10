import { NextResponse } from "next/server";
import { getAgentEnv, safeEqual, createSession } from "@/lib/community-agent";

export const dynamic = "force-dynamic";

const ALLOWED_LOCALES = new Set(["en", "zh"]);

function pickLocale(value: string | null | undefined): string {
  if (!value) return "en";
  return ALLOWED_LOCALES.has(value) ? value : "en";
}

export async function POST(req: Request) {
  const env = await getAgentEnv();
  const url = new URL(req.url);
  const localeFromQuery = pickLocale(url.searchParams.get("locale"));

  if (!env.MAINTAINER_TOKEN) {
    return new NextResponse("Not configured", {
      status: 503,
      headers: { "Cache-Control": "no-store" },
    });
  }

  const form = await req.formData();
  const submitted = String(form.get("token") ?? "");
  const locale = pickLocale(String(form.get("locale") ?? localeFromQuery));

  const valid = await safeEqual(submitted, env.MAINTAINER_TOKEN);
  if (!valid) {
    return NextResponse.redirect(new URL(`/${locale}/admin?err=1`, req.url), {
      status: 303,
      headers: { "Cache-Control": "no-store" },
    });
  }

  const sid = await createSession(env.CURATED_KV);
  if (!sid) {
    return new NextResponse("Session storage unavailable", {
      status: 503,
      headers: { "Cache-Control": "no-store" },
    });
  }

  const res = NextResponse.redirect(new URL(`/${locale}/admin`, req.url), {
    status: 303,
    headers: { "Cache-Control": "no-store" },
  });
  res.cookies.set("mt_sid", sid, {
    path: "/",
    httpOnly: true,
    secure: true,
    sameSite: "strict",
    maxAge: 60 * 60 * 24,
  });
  return res;
}
