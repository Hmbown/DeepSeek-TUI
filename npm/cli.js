#!/usr/bin/env node
/**
 * CLI wrapper - executes the downloaded DeepSeek binary.
 */

const { spawn } = require("child_process");
const path = require("path");
const fs = require("fs");

const binDir = path.join(__dirname, "bin");
const binName = process.platform === "win32" ? "deepseek.exe" : "deepseek";
const binPath = path.join(binDir, binName);

// Check for override
const override = process.env.DEEPSEEK_CLI_PATH;
const effectivePath = override && fs.existsSync(override) ? override : binPath;

if (!fs.existsSync(effectivePath)) {
  console.error("DeepSeek CLI binary not found.");
  console.error("Try reinstalling: npm install -g @hmbown/deepseek-tui");
  process.exit(1);
}

// Spawn the binary with all arguments
const child = spawn(effectivePath, process.argv.slice(2), {
  stdio: "inherit",
});

child.on("error", (err) => {
  console.error("Failed to start DeepSeek CLI:", err.message);
  process.exit(1);
});

child.on("exit", (code) => {
  process.exit(code ?? 0);
});
