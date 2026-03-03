const fs = require("fs");
const https = require("https");
const http = require("http");
const { mkdir, chmod, stat, rename, readFile, writeFile } = fs.promises;
const { createWriteStream } = fs;
const { pipeline } = require("stream/promises");
const path = require("path");

const {
  detectBinaryNames,
  releaseAssetUrl,
  releaseBinaryDirectory,
} = require("./artifacts");

function resolvePackageVersion() {
  const pkg = require("../package.json");
  return process.env.DEEPSEEK_TUI_VERSION || process.env.DEEPSEEK_VERSION || pkg.version;
}

function resolveRepo() {
  return process.env.DEEPSEEK_TUI_GITHUB_REPO || process.env.DEEPSEEK_GITHUB_REPO || "Hmbown/DeepSeek-TUI";
}

function binaryPaths() {
  const { deepseek, tui } = detectBinaryNames();
  const releaseDir = releaseBinaryDirectory();
  return {
    deepseek: {
      asset: deepseek,
      target: path.join(releaseDir, process.platform === "win32" ? "deepseek.exe" : "deepseek"),
    },
    tui: {
      asset: tui,
      target: path.join(releaseDir, process.platform === "win32" ? "deepseek-tui.exe" : "deepseek-tui"),
    },
  };
}

async function httpGet(url) {
  const client = url.startsWith("https:") ? https : http;
  const response = await new Promise((resolve, reject) => {
    client.get(url, (res) => {
      const status = res.statusCode || 0;
      if (status >= 300 && status < 400 && res.headers.location) {
        resolve({ redirect: res.headers.location, response: null });
        return;
      }
      if (status !== 200) {
        reject(new Error(`Request failed with status ${status}: ${url}`));
        return;
      }
      resolve({ redirect: null, response: res });
    }).on("error", reject);
  });
  return response;
}

async function download(url, destination) {
  const resolved = await httpGet(url);
  if (resolved.redirect) {
    return download(resolved.redirect, destination);
  }
  await mkdir(path.dirname(destination), { recursive: true });
  await pipeline(resolved.response, createWriteStream(destination));
}

async function readLocalVersion(file) {
  return readFile(file, "utf8").catch(() => "");
}

async function fileExists(file) {
  try {
    const result = await stat(file);
    return result.isFile();
  } catch {
    return false;
  }
}

async function ensureBinary(targetPath, assetName, version, repo) {
  const marker = `${targetPath}.version`;
  const downloadIfNeeded =
    process.env.DEEPSEEK_TUI_FORCE_DOWNLOAD === "1" || process.env.DEEPSEEK_FORCE_DOWNLOAD === "1";
  if (!downloadIfNeeded) {
    const existing = await fileExists(targetPath);
    if (existing) {
      const markerVersion = await readLocalVersion(marker);
      if (markerVersion === String(version)) {
        return targetPath;
      }
    }
  }
  const url = releaseAssetUrl(assetName, version, repo);
  const destination = `${targetPath}.download`;
  await download(url, destination);
  if (process.platform !== "win32") {
    await chmod(destination, 0o755);
  }
  await rename(destination, targetPath);
  await writeFile(marker, String(version), "utf8");
  return targetPath;
}

async function run() {
  if (process.env.DEEPSEEK_TUI_DISABLE_INSTALL === "1" || process.env.DEEPSEEK_DISABLE_INSTALL === "1") {
    return;
  }
  const version = resolvePackageVersion();
  const repo = resolveRepo();
  const paths = binaryPaths();
  const releaseDir = releaseBinaryDirectory();
  await mkdir(releaseDir, { recursive: true });

  await Promise.all([
    ensureBinary(paths.deepseek.target, paths.deepseek.asset, version, repo),
    ensureBinary(paths.tui.target, paths.tui.asset, version, repo),
  ]);
}

async function getBinaryPath(name) {
  await run();
  const paths = binaryPaths();
  if (name === "deepseek") {
    return paths.deepseek.target;
  }
  if (name === "deepseek-tui") {
    return paths.tui.target;
  }
  throw new Error(`Unknown binary: ${name}`);
}

module.exports = {
  getBinaryPath,
  run,
};

if (require.main === module) {
  run().catch((error) => {
    console.error("deepseek-tui install failed:", error.message);
    process.exit(1);
  });
}
