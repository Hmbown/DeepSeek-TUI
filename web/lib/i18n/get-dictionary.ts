import type { Locale } from "./config";

const dictionaries: Record<Locale, () => Promise<Record<string, unknown>>> = {
  en: () => import("./dictionaries/en").then((m) => m.default),
  zh: () => import("./dictionaries/zh").then((m) => m.default),
};

export async function getDictionary(locale: Locale) {
  return dictionaries[locale]();
}
