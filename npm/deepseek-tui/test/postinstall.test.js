const assert = require("node:assert/strict");
const test = require("node:test");

const pkg = require("../package.json");
const { _internal } = require("../scripts/install");

test("postinstall opts into optional install mode", () => {
  assert.equal(pkg.scripts.postinstall, "node scripts/install.js --optional");
});

test("optional install can be enabled by command-line flag", () => {
  assert.equal(_internal.isOptionalInstall(["--optional"], {}), true);
  assert.equal(_internal.isOptionalInstall([], {}), false);
  assert.equal(_internal.isOptionalInstall([], { DEEPSEEK_TUI_OPTIONAL_INSTALL: "1" }), true);
});

test("optional mode only changes install-time defaults", () => {
  assert.equal(_internal.maxAttempts("install", { DEEPSEEK_TUI_OPTIONAL_INSTALL: "1" }), 1);
  assert.equal(_internal.maxAttempts("runtime", { DEEPSEEK_TUI_OPTIONAL_INSTALL: "1" }), 5);
  assert.equal(_internal.defaultTimeoutMs("install", { DEEPSEEK_TUI_OPTIONAL_INSTALL: "1" }), 15_000);
  assert.equal(_internal.defaultTimeoutMs("runtime", { DEEPSEEK_TUI_OPTIONAL_INSTALL: "1" }), 300_000);
  assert.equal(_internal.defaultStallMs("install", { DEEPSEEK_TUI_OPTIONAL_INSTALL: "1" }), 5_000);
  assert.equal(_internal.defaultStallMs("runtime", { DEEPSEEK_TUI_OPTIONAL_INSTALL: "1" }), 30_000);
});

test("optional install only swallows retryable download failures", () => {
  assert.equal(
    _internal.shouldIgnoreInstallFailure("install", new Error("socket hang up"), ["--optional"], {}),
    true,
  );

  const timedOut = new Error("download exceeded total timeout of 15000 ms");
  timedOut.code = "EDOWNLOADTIMEOUT";
  assert.equal(
    _internal.shouldIgnoreInstallFailure("install", timedOut, ["--optional"], {}),
    true,
  );

  const unsupported = new Error("Unsupported platform: freebsd");
  assert.equal(
    _internal.shouldIgnoreInstallFailure("install", unsupported, ["--optional"], {}),
    false,
  );

  const badChecksum = new Error("Checksum mismatch for deepseek-linux-x64");
  assert.equal(
    _internal.shouldIgnoreInstallFailure("install", badChecksum, ["--optional"], {}),
    false,
  );
});

test("optional install still swallows wrapped http 5xx failures", async () => {
  const http5xx = new Error("Request failed with status 502: https://example.invalid");
  http5xx.name = "HttpStatusError";
  http5xx.status = 502;

  await assert.rejects(
    _internal.withRetry("fetch https://example.invalid", async () => {
      throw http5xx;
    }, "install"),
    (wrapped) => {
      assert.equal(wrapped.name, "HttpStatusError");
      assert.equal(wrapped.status, 502);
      assert.equal(
        _internal.shouldIgnoreInstallFailure("install", wrapped, ["--optional"], {}),
        true,
      );
      return true;
    },
  );
});
