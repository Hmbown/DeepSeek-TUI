const path = require("path");
const os = require("os");

const ASSET_MATRIX = {
  linux: {
    x64: ["deepseek-linux-x64", "deepseek-tui-linux-x64"],
    default: ["deepseek-linux-x64", "deepseek-tui-linux-x64"],
  },
  darwin: {
    x64: ["deepseek-macos-x64", "deepseek-tui-macos-x64"],
    arm64: ["deepseek-macos-arm64", "deepseek-tui-macos-arm64"],
    default: ["deepseek-macos-x64", "deepseek-tui-macos-x64"],
  },
  win32: {
    x64: ["deepseek-windows-x64.exe", "deepseek-tui-windows-x64.exe"],
    default: ["deepseek-windows-x64.exe", "deepseek-tui-windows-x64.exe"],
  },
};

function detectBinaryNames() {
  const platform = os.platform();
  const arch = os.arch();
  const defaults = ASSET_MATRIX[platform];
  if (!defaults) {
    throw new Error(`Unsupported platform: ${platform}`);
  }
  const pair = defaults[arch] || defaults.default;
  return {
    platform,
    arch,
    deepseek: pair[0],
    tui: pair[1],
  };
}

function executableName(base, platform) {
  return platform === "win32" ? `${base}.exe` : base;
}

function releaseAssetUrl(baseName, version, repo = "Hmbown/DeepSeek-TUI") {
  return `https://github.com/${repo}/releases/download/v${version}/${baseName}`;
}

function releaseBinaryDirectory() {
  return path.join(__dirname, "..", "bin", "downloads");
}

module.exports = {
  detectBinaryNames,
  executableName,
  releaseAssetUrl,
  releaseBinaryDirectory,
};
