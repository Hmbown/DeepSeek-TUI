/**
 * Cloudflare KV access via the OpenNext binding helper.
 * Falls back to in-memory cache for `next dev` outside of `wrangler dev`.
 */
import type { CuratedDispatch } from "./types";

const MEM = new Map<string, string>();

interface KVNamespace {
  get(key: string): Promise<string | null>;
  put(key: string, value: string, opts?: { expirationTtl?: number }): Promise<void>;
  list(opts?: { prefix?: string; limit?: number }): Promise<{ keys: { name: string }[] }>;
  delete(key: string): Promise<void>;
}

interface CloudflareEnv {
  CURATED_KV?: KVNamespace;
  DEEPSEEK_API_KEY?: string;
  GITHUB_TOKEN?: string;
  CRON_SECRET?: string;
  GITHUB_REPO?: string;
}

export async function getEnv(): Promise<CloudflareEnv> {
  try {
    const mod = await import("@opennextjs/cloudflare");
    const ctx = await mod.getCloudflareContext({ async: true });
    return ctx.env as CloudflareEnv;
  } catch {
    return {
      DEEPSEEK_API_KEY: process.env.DEEPSEEK_API_KEY,
      GITHUB_TOKEN: process.env.GITHUB_TOKEN,
      CRON_SECRET: process.env.CRON_SECRET,
      GITHUB_REPO: process.env.GITHUB_REPO,
    };
  }
}

export async function getDispatch(): Promise<CuratedDispatch | null> {
  const env = await getEnv();
  const raw = env.CURATED_KV ? await env.CURATED_KV.get("dispatch:latest") : MEM.get("dispatch:latest") ?? null;
  if (!raw) return null;
  try {
    return JSON.parse(raw) as CuratedDispatch;
  } catch {
    return null;
  }
}

export async function putDispatch(d: CuratedDispatch): Promise<void> {
  const env = await getEnv();
  const value = JSON.stringify(d);
  if (env.CURATED_KV) {
    await env.CURATED_KV.put("dispatch:latest", value, { expirationTtl: 60 * 60 * 24 * 7 });
  } else {
    MEM.set("dispatch:latest", value);
  }
}
