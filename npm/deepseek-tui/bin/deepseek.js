#!/usr/bin/env node

const { runDeepseek } = require("../scripts/run");

runDeepseek().catch((error) => {
  console.error("Failed to start deepseek:", error.message);
  process.exit(1);
});
