#!/usr/bin/env node
/**
 * Postinstall script - downloads the DeepSeek CLI binary for the current platform.
 */

const https = require("https");
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const VERSION = require("./package.json").version;
const REPO = "Hmbown/DeepSeek-TUI";

const PLATFORMS = {
  "linux-x64": "deepseek-linux-x64",
  "darwin-arm64": "deepseek-macos-arm64",
  "darwin-x64": "deepseek-macos-x64",
  "win32-x64": "deepseek-windows-x64.exe",
};

async function main() {
  const platform = `${process.platform}-${process.arch}`;
  const assetName = PLATFORMS[platform];

  if (!assetName) {
    console.error(`Unsupported platform: ${platform}`);
    console.error(`Supported: ${Object.keys(PLATFORMS).join(", ")}`);
    process.exit(1);
  }

  const binDir = path.join(__dirname, "bin");
  const binName = process.platform === "win32" ? "deepseek.exe" : "deepseek";
  const binPath = path.join(binDir, binName);

  // Skip if already exists
  if (fs.existsSync(binPath)) {
    console.log(`DeepSeek CLI already installed at ${binPath}`);
    return;
  }

  const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${assetName}`;
  console.log(`Downloading DeepSeek CLI v${VERSION}...`);

  fs.mkdirSync(binDir, { recursive: true });

  await download(url, binPath);

  // Make executable on Unix
  if (process.platform !== "win32") {
    fs.chmodSync(binPath, 0o755);
  }

  console.log(`Installed DeepSeek CLI to ${binPath}`);
}

function download(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);

    function doRequest(requestUrl) {
      https
        .get(requestUrl, (response) => {
          // Handle redirects
          if (response.statusCode >= 300 && response.statusCode < 400 && response.headers.location) {
            doRequest(response.headers.location);
            return;
          }

          if (response.statusCode !== 200) {
            reject(new Error(`Download failed: HTTP ${response.statusCode}`));
            return;
          }

          response.pipe(file);
          file.on("finish", () => {
            file.close();
            resolve();
          });
        })
        .on("error", (err) => {
          fs.unlink(dest, () => {});
          reject(err);
        });
    }

    doRequest(url);
  });
}

main().catch((err) => {
  console.error("Failed to install DeepSeek CLI:", err.message);
  process.exit(1);
});
