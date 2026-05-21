#!/usr/bin/env node

const crypto = require("crypto");
const fs = require("fs/promises");
const path = require("path");

const {
  allAssetNames,
  CHECKSUM_MANIFEST,
  detectBinaryNames,
} = require("../../npm/deepseek-tui/scripts/artifacts");

async function sha256(filePath) {
  const content = await fs.readFile(filePath);
  return crypto.createHash("sha256").update(content).digest("hex");
}

async function main() {
  const prepareAllAssets =
    process.env.DEEPSEEK_TUI_PREPARE_ALL_ASSETS === "1" ||
    process.env.DEEPSEEK_PREPARE_ALL_ASSETS === "1";
  const outputDir = path.resolve(
    process.argv[2] || path.join("target", "npm-release-assets"),
  );
  const buildDir = path.resolve(
    process.argv[3] || path.join("target", "release"),
  );
  const { deepseek, tui } = detectBinaryNames();
  const isWindows = process.platform === "win32";

  const assets = [
    {
      source: path.join(buildDir, isWindows ? "deepseek.exe" : "deepseek"),
      target: deepseek,
    },
    {
      source: path.join(buildDir, isWindows ? "deepseek-tui.exe" : "deepseek-tui"),
      target: tui,
    },
  ];

  if (prepareAllAssets) {
    for (const assetName of allAssetNames()) {
      if (assets.some((asset) => asset.target === assetName)) {
        continue;
      }
      assets.push({
        source: assetName.startsWith("deepseek-tui")
          ? path.join(buildDir, isWindows ? "deepseek-tui.exe" : "deepseek-tui")
          : path.join(buildDir, isWindows ? "deepseek.exe" : "deepseek"),
        target: assetName,
      });
    }
  }

  await fs.mkdir(outputDir, { recursive: true });

  const manifestLines = [];
  for (const asset of assets) {
    const outputPath = path.join(outputDir, asset.target);
    await fs.copyFile(asset.source, outputPath);
    manifestLines.push(`${await sha256(outputPath)}  ${asset.target}`);
  }


  // Windows: generate .bat launcher that prefers Windows Terminal
  if (isWindows) {
    const batContent = [
      "@echo off",
      "set NO_ANIMATIONS=1",
      "where wt >nul 2>nul",
      "if %ERRORLEVEL% EQU 0 (",
      "    wt --title DeepSeek-TUI cmd /k ""%~dp0deepseek-tui-windows-x64.exe"",
      ") else (",
      "    "%~dp0deepseek-tui-windows-x64.exe"",
      ")",
      "",
    ].join("\r\n");
    const batPath = path.join(outputDir, "deepseek-tui.bat");
    await fs.writeFile(batPath, batContent, "utf8");
    const batHash = await sha256(batPath);
    manifestLines.push(`${batHash}  deepseek-tui.bat`);
    console.log(`Generated ${batPath}`);
  }

  manifestLines.sort();
  const manifestPath = path.join(outputDir, CHECKSUM_MANIFEST);
  await fs.writeFile(manifestPath, `${manifestLines.join("\n")}\n`, "utf8");

  console.log(`Prepared ${assets.length} assets in ${outputDir}`);
  console.log(`Wrote checksum manifest ${manifestPath}`);
}

main().catch((error) => {
  console.error("Failed to prepare local release assets:", error.message);
  process.exit(1);
});
