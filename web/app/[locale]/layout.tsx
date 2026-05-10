import type { Metadata } from "next";
import { Nav } from "@/components/nav";
import { Footer } from "@/components/footer";
import { locales, type Locale } from "@/lib/i18n/config";

export function generateStaticParams() {
  return locales.map((locale) => ({ locale }));
}

export async function generateMetadata({ params }: { params: Promise<{ locale: string }> }): Promise<Metadata> {
  const { locale } = await params;
  const isZh = locale === "zh";
  return {
    title: isZh ? "DeepSeek TUI · 终端原生编程智能体" : "DeepSeek TUI · 深度求索 终端",
    description: isZh
      ? "基于 DeepSeek V4 的开源终端编程智能体。支持 100 万 token 上下文、MCP 协议、沙箱执行。"
      : "Terminal-native coding agent built on DeepSeek V4. Open source. Community site for installation, docs, roadmap, and live activity from the Hmbown/deepseek-tui repo.",
    metadataBase: new URL("https://deepseek-tui.com"),
    openGraph: {
      title: isZh ? "DeepSeek TUI · 终端原生编程智能体" : "DeepSeek TUI",
      description: isZh
        ? "基于 DeepSeek V4 的开源终端编程智能体。"
        : "Terminal-native coding agent built on DeepSeek V4.",
      url: "https://deepseek-tui.com",
      siteName: "DeepSeek TUI",
      type: "website",
    },
    twitter: { card: "summary_large_image" },
    alternates: {
      languages: {
        en: "/en",
        zh: "/zh",
      },
    },
  };
}

export default async function LocaleLayout({
  children,
  params,
}: {
  children: React.ReactNode;
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await params;

  return (
    <>
      <Nav locale={locale as Locale} />
      <main>{children}</main>
      <Footer locale={locale as Locale} />
    </>
  );
}
