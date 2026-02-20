import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

const rootDir = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  test: {
    environment: "jsdom",
    setupFiles: ["./vitest.setup.ts"],
    include: ["lib/**/*.test.ts", "test/**/*.test.ts", "test/**/*.test.tsx"],
  },
  resolve: {
    alias: {
      "@": rootDir,
    },
  },
});
