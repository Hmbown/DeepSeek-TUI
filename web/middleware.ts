import { NextRequest, NextResponse } from "next/server";
import { locales, defaultLocale } from "@/lib/i18n/config";

const COOKIE = "NEXT_LOCALE";

function detectLocale(req: NextRequest): string {
  // 1. Cookie
  const cookie = req.cookies.get(COOKIE)?.value;
  if (cookie && locales.includes(cookie as typeof locales[number])) return cookie;

  // 2. Accept-Language header
  const accept = req.headers.get("accept-language") ?? "";
  if (/^zh/i.test(accept.split(",")[0])) return "zh";

  return defaultLocale;
}

export function middleware(req: NextRequest) {
  const { pathname } = req.nextUrl;

  // Skip API routes, static files, _next
  if (
    pathname.startsWith("/api/") ||
    pathname.startsWith("/_next/") ||
    pathname.includes(".")
  ) {
    return NextResponse.next();
  }

  // Check if locale is already in path
  const seg = pathname.split("/")[1];
  if (locales.includes(seg as typeof locales[number])) {
    // Ensure cookie is set
    const res = NextResponse.next();
    res.cookies.set(COOKIE, seg, { path: "/", maxAge: 60 * 60 * 24 * 365 });
    return res;
  }

  // Redirect bare paths to detected locale
  const locale = detectLocale(req);
  const url = req.nextUrl.clone();
  url.pathname = `/${locale}${pathname}`;
  const res = NextResponse.redirect(url);
  res.cookies.set(COOKIE, locale, { path: "/", maxAge: 60 * 60 * 24 * 365 });
  return res;
}

export const config = {
  matcher: ["/((?!_next|api|favicon.ico|icon.svg|.*\\..*).*)"],
};
