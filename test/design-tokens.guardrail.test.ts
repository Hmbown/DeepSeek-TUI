import fs from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

describe("design token guardrails", () => {
  it("keeps required status and motion tokens in globals.css", () => {
    const cssPath = path.resolve(process.cwd(), "app/globals.css");
    const css = fs.readFileSync(cssPath, "utf8");

    const requiredTokens = [
      "--brand-blue",
      "--radius-lg",
      "--space-4",
      "--font-base",
      "--state-success",
      "--state-warning",
      "--state-danger",
      "--dur-mid",
      "--ease-standard",
    ];

    for (const token of requiredTokens) {
      expect(css.includes(token), `Missing token ${token}`).toBe(true);
    }
  });
});
