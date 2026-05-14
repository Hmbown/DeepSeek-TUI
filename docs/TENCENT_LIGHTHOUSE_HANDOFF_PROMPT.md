# Tencent Lighthouse + Lark Setup Handoff Prompt

Use this prompt with a Computer Use capable agent when you are ready to create
the Tencent Lighthouse instance and Lark/Feishu app.

```text
You are taking over a live setup task on my Mac. Use Computer Use/browser UI for the Tencent Cloud and Feishu/Lark consoles. Require explicit confirmation before purchases, external submissions, sending bot messages to other people, deleting files, or entering secrets.

Goal:
Set up a Tencent Cloud Lighthouse Hong Kong VPS and a Feishu/Lark self-built bot so I can control a remote /opt/whalebro workspace from my phone while traveling in China.

Repo/workspace:
- Canonical repo: /Volumes/VIXinSSD/whalebro/deepseek-tui
- Product repo to include on the VPS when requested: /Volumes/VIXinSSD/whalebro/whalescale
- Read /Volumes/VIXinSSD/whalebro/AGENTS.md and /Volumes/VIXinSSD/whalebro/deepseek-tui/AGENTS.md before editing.
- The repo now has a first-pass deployment/runbook under:
  - docs/TENCENT_LIGHTHOUSE_HK.md
  - docs/FEISHU_LIGHTHOUSE_V0_8_37_PLAN.md
  - integrations/feishu-bridge/
  - deploy/tencent-lighthouse/
  - scripts/tencent-lighthouse/
- Current working branch with this setup: work/v0.8.37-feishu-lighthouse. Verify it is pushed before relying on a VPS git clone.
- Current CNB mirror for this branch: https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git refs/heads/work/v0.8.37-feishu-lighthouse.
- Remote-first overview: docs/TENCENT_CLOUD_REMOTE_FIRST.md.
- CNB deploy templates are non-active examples under deploy/tencent-lighthouse/cnb/.

Important architecture:
- Use plain Ubuntu 24.04 LTS on Tencent Lighthouse Hong Kong.
- Buy the HK Linux 2 vCPU / 4 GB / 70 GB / 30M / 2 TB per month plan first, preferably 1 month.
- The runtime must stay bound to 127.0.0.1:7878 on the VPS.
- The phone-facing channel is the Feishu/Lark bot long connection service.
- CNB is the preferred source/deploy lane once the branch exists there.
- EdgeOne is optional and should only front a deliberate public HTTPS service; do not expose /v1 runtime endpoints through it.
- Direct message control is the MVP. Keep FEISHU_ALLOW_GROUPS=false initially.
- The VPS workspace root is /opt/whalebro.
- Required checkout: /opt/whalebro/deepseek-tui.
- Optional checkout if I want the full active workspace: /opt/whalebro/whalescale.
- Use /opt/whalebro/worktrees for worktrees intentionally created on the VPS.
- If these deployment files are not pushed to Git yet, either help me push the branch first or copy the current local checkout to the VPS. A fresh VPS clone cannot see uncommitted local files.

Secrets to collect from me interactively:
- Tencent Cloud login/session if not already logged in.
- SSH public key to add to Lighthouse.
- DeepSeek API key for /etc/deepseek/runtime.env.
- Runtime bearer token: generate with openssl rand -hex 32.
- Feishu/Lark App ID and App Secret from the self-built app.

Tencent Cloud steps:
1. Open Tencent Cloud Lighthouse purchase page.
2. Select Hong Kong, China region.
3. Select plain Ubuntu 24.04 LTS or latest Ubuntu LTS.
4. Select the HK 2c/4G/70G monthly plan first.
5. Use SSH key login, not password login.
6. Confirm firewall/security group keeps SSH open.
7. Ask me before clicking final purchase/checkout.
8. After purchase, record the public IP and SSH command.

Feishu/Lark steps:
1. Open Feishu China or Lark international developer console, whichever matches my account.
2. Create an enterprise self-built app.
3. Enable bot capability.
4. Add message receive/send permissions required for text DMs.
5. Add event subscription for im.message.receive_v1.
6. Use long connection/WebSocket mode.
7. Publish/release the app as required by the console.
8. Add the bot to my own DM chat first.

VPS setup steps:
1. SSH into the instance.
2. Clone the repo from CNB when available and run docs/TENCENT_LIGHTHOUSE_HK.md exactly, adapting only branch/repo URL if needed.
3. Run:
   sudo DEEPSEEK_REPO_URL=https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git DEEPSEEK_REPO_BRANCH=work/v0.8.37-feishu-lighthouse bash scripts/tencent-lighthouse/bootstrap-ubuntu.sh
   If I confirm I want whalescale on the VPS immediately, use:
   sudo DEEPSEEK_REPO_URL=https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git DEEPSEEK_REPO_BRANCH=work/v0.8.37-feishu-lighthouse WHALEBRO_EXTRA_REPOS='whalescale=https://github.com/Hmbown/whalescale.git' bash scripts/tencent-lighthouse/bootstrap-ubuntu.sh
   Use SSH remotes instead if the repo is private or I need push access from the VPS.
4. Install Rust 1.88+ for the deepseek user via rustup minimal profile.
5. Build/install both binaries:
   cargo install --path crates/cli --locked --force
   cargo install --path crates/tui --locked --force
6. Run:
   sudo bash scripts/tencent-lighthouse/install-services.sh
7. Edit /etc/deepseek/runtime.env and /etc/deepseek/feishu-bridge.env.
8. Validate bridge/runtime config:
   sudo -u deepseek node /opt/deepseek/bridge/scripts/validate-config.mjs --env /etc/deepseek/feishu-bridge.env --runtime-env /etc/deepseek/runtime.env --workspace-root /opt/whalebro --check-filesystem
9. Start deepseek-runtime and verify:
   curl -s http://127.0.0.1:7878/health
10. Start deepseek-feishu-bridge and tail logs.
11. Run:
   sudo bash /opt/whalebro/deepseek-tui/scripts/tencent-lighthouse/doctor.sh
12. Pair by temporarily setting DEEPSEEK_ALLOW_UNLISTED=true if needed, DM the bot, copy the returned chat_id, set DEEPSEEK_CHAT_ALLOWLIST to that chat_id, then turn DEEPSEEK_ALLOW_UNLISTED=false.

Validation:
- From phone DM, send /status.
- Confirm the bot reports runtime, version, bind host, and workspace status.
- Send a harmless prompt: "summarize git status".
- Confirm the runtime bind host is 127.0.0.1.
- Validate /interrupt, /threads, /resume, /allow, and /deny from the phone DM.
- Run systemctl status for both services.
- Restart both services and confirm /status still works.
- Reboot the instance and confirm both services return active.
- Capture final IP, SSH command, service status, and any remaining blockers.
```
