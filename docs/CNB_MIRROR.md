# CNB Cool mirror

`cnb.cool/deepseek-tui.com/DeepSeek-TUI` is a one-way mirror of this
GitHub repository for users on networks where GitHub is slow or blocked
(primarily mainland China). The mirror receives every push to `main` and
every `v*` release tag.

## How it works

The mirror is maintained by the [`Sync to CNB`](../.github/workflows/sync-cnb.yml)
GitHub Actions workflow:

- **Trigger:** `push` to `main`, `push` of any `v*` tag, or
  `workflow_dispatch` for manual recovery.
- **Auth:** HTTPS basic auth as user `cnb` with the `CNB_GIT_TOKEN`
  repository secret as the password.
- **Scope:** only the ref that triggered the run is pushed. Tag pushes
  push exactly that tag. Branch pushes push only `main`
  (`--force-with-lease`). Feature branches and dependabot refs are
  intentionally *not* mirrored.
- **Concurrency:** runs are serialized via a `cnb-sync` concurrency
  group so the back-to-back `main` push and tag push from
  `auto-tag.yml` cannot race each other.
- **Retry:** each push is retried up to three times with linear
  backoff (5s, 10s) before the workflow gives up.

## Verifying the mirror after a release

After `release.yml` completes for a `vX.Y.Z` tag, the CNB mirror
should have both the new commit on `main` and the new tag:

```bash
# Quick check: does the new tag exist on CNB?
git ls-remote https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git \
    refs/tags/vX.Y.Z

# Quick check: is CNB's main at the same commit as origin/main?
gh_main=$(git ls-remote https://github.com/Hmbown/DeepSeek-TUI.git refs/heads/main | awk '{print $1}')
cnb_main=$(git ls-remote https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git refs/heads/main | awk '{print $1}')
test "$gh_main" = "$cnb_main" && echo "in sync" || echo "DIVERGED: gh=$gh_main cnb=$cnb_main"
```

Or check the workflow run directly:

```bash
gh run list --workflow=sync-cnb.yml --repo Hmbown/DeepSeek-TUI --limit 5
```

If the most recent run for the release tag is `success`, the mirror
caught it. If it's `failure`, follow the manual fallback below.

## Manual fallback

If the workflow fails for any reason (CNB rate-limit, token expired,
GitHub outage, etc.), the maintainer can push to CNB by hand from
their local checkout. This works because the CNB token is a personal
PAT — the same token used by the workflow lives in the maintainer's
password manager.

### One-time setup

```bash
# Add the CNB remote alongside origin.
git remote add cnb https://cnb:${CNB_TOKEN}@cnb.cool/deepseek-tui.com/DeepSeek-TUI.git

# Or, if you don't want the token in your shell history:
git remote add cnb https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git
# (you'll be prompted for username `cnb` and password ${CNB_TOKEN}
#  on the first push; subsequent pushes use the credential helper.)
```

### Sync a release manually

```bash
# Make sure main is current.
git fetch origin
git checkout main
git reset --hard origin/main

# Push main first, then the tag. Order matters: CNB should see the
# commit before the tag that points at it.
git push cnb main --force-with-lease
git push cnb vX.Y.Z
```

### Re-trigger the workflow manually

If the workflow is healthy but happened to fail on the release run
(e.g. a transient CNB outage that's since cleared), retrigger it
without pushing anything:

```bash
gh workflow run sync-cnb.yml --repo Hmbown/DeepSeek-TUI
```

`workflow_dispatch` runs against the workflow's default branch
(`main`), so this will sync the current `main` to CNB. To re-sync
a specific tag, the manual `git push cnb` path above is the way.

## Rotating `CNB_GIT_TOKEN`

If the workflow starts failing with auth errors and the token has
expired:

1. Log in to `cnb.cool` and generate a new personal access token
   with `repo` (push) scope.
2. Update the `CNB_GIT_TOKEN` repository secret:
   ```bash
   gh secret set CNB_GIT_TOKEN --repo Hmbown/DeepSeek-TUI
   ```
3. Re-trigger the workflow on a recent commit:
   ```bash
   gh workflow run sync-cnb.yml --repo Hmbown/DeepSeek-TUI
   ```
4. Confirm the run succeeds via `gh run list --workflow=sync-cnb.yml`.

## Binary release assets

CNB is a code mirror only — it does not host binary release assets.
Users behind GitHub-blocking networks who need the prebuilt binaries
have two options:

- **`cargo install`** from the CNB mirror:
  ```bash
  cargo install --git https://cnb.cool/deepseek-tui.com/DeepSeek-TUI --tag vX.Y.Z deepseek-tui-cli
  cargo install --git https://cnb.cool/deepseek-tui.com/DeepSeek-TUI --tag vX.Y.Z deepseek-tui
  ```
  (Both binaries are required — the dispatcher and the TUI ship
  separately; see `AGENTS.md` for the two-binary install rationale.)

- **`DEEPSEEK_TUI_RELEASE_BASE_URL`** environment variable, if a
  third-party CDN mirror of the GitHub Release assets exists. The
  npm wrapper installer in `npm/deepseek-tui/scripts/install.js`
  reads this variable to redirect binary downloads. The directory
  pointed to must contain `deepseek-artifacts-sha256.txt` and the
  platform binaries; format matches a GitHub Release asset
  directory.

A first-party binary CDN mirror for CNB users is on the v0.8.32+
roadmap; it is not part of v0.8.31.
