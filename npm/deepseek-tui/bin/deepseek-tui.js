#!/usr/bin/env node

const { runDeepseekTui } = require("../scripts/run");

runDeepseekTui().catch((error) => {
  console.error("Failed to start deepseek-tui:", error.message);
  process.exit(1);
});
