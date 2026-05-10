"use client";

import { useRouter, usePathname } from "next/navigation";
import { locales, type Locale } from "@/lib/i18n/config";

const LABELS: Record<Locale, string> = { en: "EN", zh: "中文" };

export function LocaleSwitcher({ current }: { current: string }) {
  const router = useRouter();
  const pathname = usePathname();

  const switchTo = current === "zh" ? "en" : "zh";
  const other = LABELS[switchTo as Locale];

  const handleClick = () => {
    // Replace locale segment in path
    const segments = pathname.split("/");
    if (locales.includes(segments[1] as Locale)) {
      segments[1] = switchTo;
    } else {
      segments.splice(1, 0, switchTo);
    }
    const newPath = segments.join("/") || `/${switchTo}`;
    document.cookie = `NEXT_LOCALE=${switchTo};path=/;max-age=${60 * 60 * 24 * 365}`;
    router.push(newPath);
  };

  return (
    <button
      onClick={handleClick}
      className="inline-flex items-center gap-1.5 px-2.5 py-1 hairline-t hairline-b hairline-l hairline-r font-mono text-[0.7rem] uppercase tracking-wider hover:bg-paper-deep transition-colors"
      title={current === "zh" ? "Switch to English" : "切换到中文"}
    >
      <span className="font-cjk normal-case tracking-normal">{other}</span>
    </button>
  );
}
