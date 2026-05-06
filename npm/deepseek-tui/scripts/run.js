const { spawnSync } = require("child_process");
const { getBinaryPath } = require("./install");

async function run(binaryName) {
  const binaryPath = await getBinaryPath(binaryName);
  const result = spawnSync(binaryPath, process.argv.slice(2), {
    stdio: "inherit",
  });
  if (result.error) {
    throw result.error;
  }
  process.exit(result.status ?? 1);
}

async function runDeepseek() {
  await run("deepseek");
}

async function runDeepseekTui() {
  await run("deepseek-tui");
}

module.exports = {
  run,
  runDeepseek,
  runDeepseekTui,
};

if (require.main === module) {
  const command = process.argv[1] || "";
  if (command.includes("tui")) {
    runDeepseekTui();
  } else {
    runDeepseek();
  }
}
