export const locales = ["en", "zh"] as const;
export type Locale = (typeof locales)[number];
export const defaultLocale: Locale = "en";

/** Set to "1" once the Gitee mirror at gitee.com/Hmbown/... exists. */
export const GITEE_ENABLED = process.env.NEXT_PUBLIC_GITEE_ENABLED === "1";

export function isValidLocale(x: string): x is Locale {
  return locales.includes(x as Locale);
}
