# Feishu Lighthouse v0.8.37 Plan

Goal: make Feishu/Lark on Tencent Lighthouse a supported remote-control path
for `deepseek serve --http`.

## Release Shape

- The public teaching path is Tencent-native: CNB source/build/deploy,
  Lighthouse runtime, Feishu/Lark phone control, and optional EdgeOne for a
  deliberate public HTTPS edge.
- `deepseek serve --http` runs as a localhost systemd service on the VPS.
- `integrations/feishu-bridge` receives Feishu/Lark messages over long
  connection mode and calls the runtime API with a bearer token.
- `/opt/whalebro` is the remote workspace root.
- `/opt/whalebro/deepseek-tui` is required.
- `/opt/whalebro/whalescale` is available when product work is needed.
- Direct-message control is the default phone workflow.

## Current Foundation

- Bridge source: `integrations/feishu-bridge/`
- Tencent deploy assets: `deploy/tencent-lighthouse/`
- VPS scripts: `scripts/tencent-lighthouse/`
- Config validator: `integrations/feishu-bridge/scripts/validate-config.mjs`
- VPS doctor: `scripts/tencent-lighthouse/doctor.sh`
- Remote-first tutorial: `docs/TENCENT_CLOUD_REMOTE_FIRST.md`
- CNB deploy templates: `deploy/tencent-lighthouse/cnb/`
- Runbook: `docs/TENCENT_LIGHTHOUSE_HK.md`
- Computer Use handoff: `docs/TENCENT_LIGHTHOUSE_HANDOFF_PROMPT.md`

## v0.8.37 Work

1. Create a release branch for this lane, then update the runbook branch value
   once it is pushed.
2. Add a Lighthouse doctor script that checks Ubuntu packages, Node version,
   installed `deepseek` binaries, systemd unit files, env files, runtime health,
   bridge process status, and localhost bind.
3. Add a bridge config validator that checks required env vars, token presence
   on both services, domain selection, allowlist state, group-mode settings, and
   writable thread-map path.
4. Add bridge tests for event dedupe, allowlist pairing, command dispatch,
   group prefix handling, active-turn protection, and approval command parsing.
5. Add a manual end-to-end checklist for a fresh Lighthouse VM:
   `/status`, prompt, `/interrupt`, approval allow/deny, `/threads`, `/resume`,
   service restart, reboot persistence.
6. Tighten setup docs around the exact Feishu/Lark console fields:
   bot capability, message permissions, `im.message.receive_v1`, long
   connection mode, app release, bot DM pairing, and chat allowlist capture.
7. Add bridge logging that is useful in `journalctl`: startup config summary,
   connection status, received message id, chosen thread id, turn id, approval
   id, and compact runtime errors.
8. Add a release-note entry describing the Lighthouse + Feishu/Lark remote
   control path and the supported first setup flow.
9. Add the CNB + Lighthouse + EdgeOne teaching shape without activating a live
   CNB deployment pipeline before secrets, deploy key, and quota policy are
   explicit.

## Acceptance

- A clean Tencent Lighthouse Ubuntu instance can be bootstrapped from the
  documented branch.
- The Tencent-native onboarding doc explains when to use CNB, when to use
  Lighthouse, and when EdgeOne is optional rather than required.
- CNB deploy examples are present but non-active until copied into `.cnb.yml`
  and `.cnb/tag_deploy.yml`.
- `deepseek-runtime.service` starts and `/health` responds locally.
- `deepseek-feishu-bridge.service` connects through long connection mode.
- A Feishu/Lark phone DM can create a thread, run a prompt, interrupt a turn,
  list threads, resume a thread, and answer a tool approval.
- `/status` reports runtime version, bind host, auth state, workspace, git repo,
  branch, and dirty counts.
- After reboot, both services return to the same working state.

## References

- Tencent Lighthouse firewall docs:
  `https://intl.cloud.tencent.com/document/product/1103/41393`
- Tencent Lighthouse SSH key docs:
  `https://intl.cloud.tencent.com/ind/document/product/1103/41392`
- Lark/Feishu Node SDK:
  `https://github.com/larksuite/node-sdk`
