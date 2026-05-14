const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const installScript = fs.readFileSync(
  path.join(__dirname, "..", "scripts", "install.js"),
  "utf8",
);
const { installFailureHint } = require("../scripts/install");

test("install script checks Node support before loading helpers", () => {
  const guardIndex = installScript.indexOf("assertSupportedNode();");
  const firstRequireIndex = installScript.indexOf("require(");

  assert.notEqual(guardIndex, -1);
  assert.notEqual(firstRequireIndex, -1);
  assert.ok(guardIndex < firstRequireIndex);
});

test("install script remains parseable before the Node support guard runs", () => {
  assert.equal(installScript.includes("??"), false);
  assert.equal(installScript.includes("?."), false);
});

test("install failure hint explains release base override for blocked GitHub downloads", () => {
  const previous = process.env.DEEPSEEK_TUI_RELEASE_BASE_URL;
  delete process.env.DEEPSEEK_TUI_RELEASE_BASE_URL;
  try {
    const error = Object.assign(
      new Error(
        "fetch https://github.com/Hmbown/DeepSeek-TUI/releases/download/v0.8.19/deepseek-artifacts-sha256.txt failed after 5 attempts:\ngetaddrinfo ENOTFOUND github.com",
      ),
      { code: "ENOTFOUND" },
    );

    const hint = installFailureHint(error);

    assert.match(hint, /DEEPSEEK_TUI_RELEASE_BASE_URL/);
    assert.match(hint, /deepseek-artifacts-sha256\.txt/);
    assert.match(hint, /platform binaries/);
    assert.match(hint, /#npm-binary-download-times-out/);
  } finally {
    if (previous === undefined) {
      delete process.env.DEEPSEEK_TUI_RELEASE_BASE_URL;
    } else {
      process.env.DEEPSEEK_TUI_RELEASE_BASE_URL = previous;
    }
  }
});

test("install failure hint checks configured release base when override is already set", () => {
  const previous = process.env.DEEPSEEK_TUI_RELEASE_BASE_URL;
  process.env.DEEPSEEK_TUI_RELEASE_BASE_URL = "https://mirror.example/deepseek/";
  try {
    const error = Object.assign(new Error("download stalled"), {
      code: "EDOWNLOADTIMEOUT",
    });

    const hint = installFailureHint(error);

    assert.match(hint, /is set to https:\/\/mirror\.example\/deepseek\//);
    assert.match(hint, /deepseek-artifacts-sha256\.txt/);
    assert.doesNotMatch(hint, /If GitHub is unavailable/);
  } finally {
    if (previous === undefined) {
      delete process.env.DEEPSEEK_TUI_RELEASE_BASE_URL;
    } else {
      process.env.DEEPSEEK_TUI_RELEASE_BASE_URL = previous;
    }
  }
});
