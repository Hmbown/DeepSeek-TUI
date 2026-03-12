const path = require("path");
const os = require("os");

const CHECKSUM_MANIFEST = "deepseek-artifacts-sha256.txt";

const ASSET_MATRIX = {
  linux: {
    x64: ["deepseek-linux-x64", "deepseek-tui-linux-x64"],
    // arm64: ["deepseek-linux-arm64", "deepseek-tui-linux-arm64"], // Uncomment when binaries are available
  },
  darwin: {
    x64: ["deepseek-macos-x64", "deepseek-tui-macos-x64"],
    arm64: ["deepseek-macos-arm64", "deepseek-tui-macos-arm64"],
  },
  win32: {
    x64: ["deepseek-windows-x64.exe", "deepseek-tui-windows-x64.exe"],
  },
};

function detectBinaryNames() {
  const platform = os.platform();
  const arch = os.arch();
  const defaults = ASSET_MATRIX[platform];
  if (!defaults) {
    const supported = Object.keys(ASSET_MATRIX).map(p => `'${p}'`).join(', ');
    throw new Error(`Unsupported platform: ${platform}. Supported platforms: ${supported}`);
  }
  const pair = defaults[arch];
  if (!pair) {
    const supported = Object.keys(defaults).map(a => `'${a}'`).join(', ');
    throw new Error(`Unsupported architecture: ${arch} on platform ${platform}. Supported architectures: ${supported}`);
  }
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

function releaseBaseUrl(version, repo = "Hmbown/DeepSeek-TUI") {
  const override =
    process.env.DEEPSEEK_TUI_RELEASE_BASE_URL || process.env.DEEPSEEK_RELEASE_BASE_URL;
  if (override) {
    const trimmed = String(override).trim();
    return trimmed.endsWith("/") ? trimmed : `${trimmed}/`;
  }
  return `https://github.com/${repo}/releases/download/v${version}/`;
}

function releaseAssetUrl(baseName, version, repo = "Hmbown/DeepSeek-TUI") {
  return new URL(baseName, releaseBaseUrl(version, repo)).toString();
}

function checksumManifestUrl(version, repo = "Hmbown/DeepSeek-TUI") {
  return releaseAssetUrl(CHECKSUM_MANIFEST, version, repo);
}

function releaseBinaryDirectory() {
  return path.join(__dirname, "..", "bin", "downloads");
}

function allAssetNames() {
  const names = [];
  for (const platformAssets of Object.values(ASSET_MATRIX)) {
    for (const pair of Object.values(platformAssets)) {
      names.push(pair[0], pair[1]);
    }
  }
  return Array.from(new Set(names));
}

function allReleaseAssetNames() {
  return [...allAssetNames(), CHECKSUM_MANIFEST];
}

module.exports = {
  allAssetNames,
  allReleaseAssetNames,
  CHECKSUM_MANIFEST,
  checksumManifestUrl,
  detectBinaryNames,
  executableName,
  releaseAssetUrl,
  releaseBaseUrl,
  releaseBinaryDirectory,
};
