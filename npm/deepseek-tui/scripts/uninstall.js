const fs = require("fs");
const path = require("path");

const downloadDir = path.join(__dirname, "..", "bin", "downloads");

function removeDir(dir) {
  if (!fs.existsSync(dir)) return;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      removeDir(full);
    } else {
      fs.unlinkSync(full);
    }
  }
  fs.rmdirSync(dir);
}

try {
  removeDir(downloadDir);
  console.log(deepseek-tui: cleaned up );
} catch (err) {
  console.error(deepseek-tui: cleanup failed (), skipping.);
  process.exit(0);
}