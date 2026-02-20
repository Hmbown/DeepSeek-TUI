# Wagmii-CLI Bug Hunt: Overnight Fix Sprint

## Mission Brief

You are a team of engineers tasked with fixing 9 identified bugs in the wagmii-cli configuration system. Work through each issue systematically, ensuring all fixes include tests and documentation updates where applicable.

**Repository:** `/Volumes/VIXinSSD/wagmii-cli`
**Language:** Rust
**Build command:** `cargo build`
**Test command:** `cargo test --all-features`
**Lint command:** `cargo clippy --all-targets --all-features`
**Format command:** `cargo fmt`

## Ground Rules

1. **Run tests after each fix** - Never move to the next issue until `cargo test` passes
2. **Run clippy after each fix** - Address any new warnings
3. **Commit after each completed issue** - Use conventional commits: `fix: <description>`
4. **Do not break existing functionality** - If unsure, add tests first to capture current behavior
5. **Update documentation** - If you change behavior, update README.md and config.example.toml

---

## Issue Queue (Work in Priority Order)

### ISSUE-1: Tilde Path Expansion Not Implemented [HIGH]

**Problem:** Config paths like `skills_dir = "~/.wagmii/skills"` are not expanded. `PathBuf::from("~/.wagmii/skills")` creates a literal path with `~` as a directory name.

**Files to modify:**
- `src/config.rs` (lines 154-190)
- `Cargo.toml` (add dependency if needed)

**Implementation:**
```rust
// Add to Cargo.toml if not present:
// shellexpand = "3"

// Create helper function in config.rs:
fn expand_path(path: &str) -> PathBuf {
    let expanded = shellexpand::tilde(path);
    PathBuf::from(expanded.as_ref())
}

// Update these methods to use expand_path():
// - skills_dir()
// - mcp_config_path()
// - notes_path()
// - memory_path()
// - output_dir()
```

**Required tests:**
```rust
#[test]
fn test_tilde_expansion_in_paths() {
    // Test that "~/.wagmii/skills" expands to home dir
    // Test that "/absolute/path" remains unchanged
    // Test that "./relative/path" remains unchanged
}
```

**Acceptance criteria:**
- [ ] `shellexpand` crate added to Cargo.toml (or use dirs::home_dir manually)
- [ ] All path getter methods expand tilde
- [ ] Tests pass for tilde, absolute, and relative paths
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes

---

### ISSUE-2: Silent Profile Fallback on Non-existent Profile [HIGH]

**Problem:** `wagmii --profile nonexistent` silently falls back to base config with no warning.

**Files to modify:**
- `src/config.rs` (function `apply_profile` around line 310)
- `src/main.rs` (handle the error/warning)

**Implementation options (choose one):**

**Option A - Return Result with error:**
```rust
fn apply_profile(config: ConfigFile, profile: Option<&str>) -> Result<Config> {
    if let Some(profile_name) = profile {
        let profiles = config.profiles.as_ref();
        match profiles.and_then(|p| p.get(profile_name)) {
            Some(override_cfg) => Ok(merge_config(config.base, override_cfg.clone())),
            None => Err(anyhow::anyhow!(
                "Profile '{}' not found. Available profiles: {}",
                profile_name,
                profiles.map(|p| p.keys().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_else(|| "none".to_string())
            )),
        }
    } else {
        Ok(config.base)
    }
}
```

**Option B - Warn but continue:**
```rust
fn apply_profile(config: ConfigFile, profile: Option<&str>) -> Config {
    if let Some(profile_name) = profile {
        if let Some(profiles) = &config.profiles {
            if let Some(override_cfg) = profiles.get(profile_name) {
                return merge_config(config.base, override_cfg.clone());
            }
            eprintln!(
                "Warning: Profile '{}' not found, using base config. Available: {}",
                profile_name,
                profiles.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        } else {
            eprintln!("Warning: Profile '{}' requested but no profiles defined", profile_name);
        }
    }
    config.base
}
```

**Required tests:**
```rust
#[test]
fn test_nonexistent_profile_error() {
    // Create config with profiles.work defined
    // Request profile "nonexistent"
    // Verify error or warning is produced
}

#[test]
fn test_profile_with_no_profiles_section() {
    // Config with no [profiles.*] at all
    // Request any profile
    // Verify appropriate handling
}
```

**Acceptance criteria:**
- [ ] Non-existent profile produces clear error or warning
- [ ] Error message lists available profiles
- [ ] Tests cover: missing profile, no profiles section
- [ ] `cargo test` passes

---

### ISSUE-3: `/yolo` Command vs `--yolo` Flag Inconsistency [MEDIUM]

**Problem:** `/yolo` command doesn't set `trust_mode`, `approval_mode`, or `yolo` flag, unlike `--yolo`.

**Files to modify:**
- `src/commands/config.rs` (function `yolo` around line 141)

**Current code:**
```rust
pub fn yolo(app: &mut App) -> CommandResult {
    app.allow_shell = true;
    app.set_mode(AppMode::Agent);
    CommandResult::message("YOLO mode enabled - agent mode with shell execution!")
}
```

**Fixed code:**
```rust
pub fn yolo(app: &mut App) -> CommandResult {
    app.allow_shell = true;
    app.trust_mode = true;
    app.yolo = true;
    app.approval_mode = ApprovalMode::Auto;
    app.set_mode(AppMode::Agent);
    CommandResult::message(
        "YOLO mode enabled - agent mode + shell + trust + auto-approve!"
    )
}
```

**Add import if needed:**
```rust
use crate::tui::approval::ApprovalMode;
```

**Required tests:**
```rust
#[test]
fn test_yolo_command_sets_all_flags() {
    let mut app = create_test_app();
    yolo(&mut app);
    assert!(app.allow_shell);
    assert!(app.trust_mode);
    assert!(app.yolo);
    assert_eq!(app.approval_mode, ApprovalMode::Auto);
    assert_eq!(app.mode, AppMode::Agent);
}
```

**Acceptance criteria:**
- [ ] `/yolo` sets: allow_shell, trust_mode, yolo, approval_mode=Auto, mode=Agent
- [ ] Message updated to reflect all changes
- [ ] Test verifies all flags
- [ ] `cargo test` passes

---

### ISSUE-4: App.trust_mode Not Synced with Engine on Startup [MEDIUM]

**Problem:** `App::new()` always sets `trust_mode: false`, but engine gets `trust_mode: options.yolo`.

**Files to modify:**
- `src/tui/app.rs` (in `App::new()` around line 353)

**Current code:**
```rust
trust_mode: false,
```

**Fixed code:**
```rust
trust_mode: yolo,  // Sync with --yolo flag
```

**Acceptance criteria:**
- [ ] `App::new()` initializes `trust_mode` from `yolo` parameter
- [ ] `/config` command shows correct trust_mode on startup with `--yolo`
- [ ] `cargo test` passes

---

### ISSUE-5: config.example.toml Documents Unsupported Fields [MEDIUM]

**Problem:** Example config shows fields that don't exist in Config struct.

**Files to modify:**
- `config.example.toml`

**Changes:**

1. **Remove or comment out `anthropic_api_key`** (line 11):
```toml
# anthropic_api_key = "YOUR_ANTHROPIC_COMPAT_API_KEY"  # Not yet supported
```

2. **Remove or comment out `anthropic_base_url`** (line 18):
```toml
# anthropic_base_url = "https://api.wagmii.io/anthropic"  # Computed from base_url
```

3. **Mark `[compaction]` section as planned** (lines 55-63):
```toml
# ─────────────────────────────────────────────────────────────────────────────────
# Context Compaction (PLANNED - not yet implemented)
# ─────────────────────────────────────────────────────────────────────────────────
# [compaction]
# enabled = false
# token_threshold = 50000
# message_threshold = 50
# model = "Wagmii-M2.1"
# cache_summary = true
```

4. **Mark `[rlm]` section as planned** (lines 65-72):
```toml
# ─────────────────────────────────────────────────────────────────────────────────
# RLM Sandbox Configuration (PLANNED - not yet implemented)
# ─────────────────────────────────────────────────────────────────────────────────
# [rlm]
# max_context_chars = 10000000
# ...
```

**Acceptance criteria:**
- [ ] Unsupported fields commented out with explanatory notes
- [ ] Users copying example won't expect non-working features
- [ ] File still serves as useful reference

---

### ISSUE-6: save_api_key Overly Broad Line Matching [LOW]

**Problem:** `line.trim_start().starts_with("api_key")` matches `api_key_backup` etc.

**Files to modify:**
- `src/config.rs` (function `save_api_key` around line 372)

**Current code:**
```rust
if line.trim_start().starts_with("api_key") {
```

**Fixed code:**
```rust
// Match "api_key" followed by optional whitespace and "="
let trimmed = line.trim_start();
if trimmed.starts_with("api_key")
    && trimmed[7..].trim_start().starts_with('=')
{
```

**Or use regex (add `regex` crate if preferred):**
```rust
use regex::Regex;
lazy_static! {
    static ref API_KEY_LINE: Regex = Regex::new(r"^\s*api_key\s*=").unwrap();
}
if API_KEY_LINE.is_match(line) {
```

**Simpler alternative without regex:**
```rust
let trimmed = line.trim_start();
if trimmed == "api_key" || trimmed.starts_with("api_key =") || trimmed.starts_with("api_key=") {
```

**Required tests:**
```rust
#[test]
fn test_save_api_key_doesnt_match_similar_keys() {
    // Create config with:
    // api_key_backup = "old"
    // api_key = "current"
    // Save new key
    // Verify api_key_backup unchanged, api_key updated
}
```

**Acceptance criteria:**
- [ ] Only `api_key = ` lines are matched, not `api_key_*`
- [ ] Test with config containing `api_key_backup`
- [ ] `cargo test` passes

---

### ISSUE-7: README Missing WAGMII_MEMORY_PATH [LOW]

**Problem:** README documents env vars but misses `WAGMII_MEMORY_PATH`.

**Files to modify:**
- `README.md` (around line 90)

**Current:**
```markdown
- `WAGMII_ALLOW_SHELL`, `WAGMII_SKILLS_DIR`, `WAGMII_MCP_CONFIG`, `WAGMII_NOTES_PATH`
```

**Fixed:**
```markdown
- `WAGMII_ALLOW_SHELL`, `WAGMII_SKILLS_DIR`, `WAGMII_MCP_CONFIG`, `WAGMII_NOTES_PATH`, `WAGMII_MEMORY_PATH`
```

**Also add `WAGMII_OUTPUT_DIR` and `WAGMII_MAX_SUBAGENTS` if missing.**

**Acceptance criteria:**
- [ ] All env vars from `apply_env_overrides()` documented in README
- [ ] List matches actual implementation

---

### ISSUE-8: No Validation of Empty Config After Profile Merge [LOW]

**Problem:** Profile can override `api_key` to empty, no early error.

**Files to modify:**
- `src/config.rs` (after `apply_profile` or in `Config::load`)

**Implementation:**
```rust
impl Config {
    /// Validate that critical config fields are present
    pub fn validate(&self) -> Result<()> {
        // api_key is optional at load time (onboarding handles missing key)
        // But if explicitly set to empty string, that's suspicious
        if let Some(ref key) = self.api_key {
            if key.trim().is_empty() {
                anyhow::bail!("api_key cannot be empty string");
            }
        }
        Ok(())
    }
}

// In Config::load(), after apply_profile and apply_env_overrides:
config.validate()?;
```

**Required tests:**
```rust
#[test]
fn test_empty_api_key_rejected() {
    // Config with api_key = ""
    // Verify validation error
}
```

**Acceptance criteria:**
- [ ] Empty string api_key produces clear error
- [ ] Test covers this case
- [ ] Normal missing api_key still works (onboarding flow)

---

### ISSUE-9: Hook timeout_secs Not Actually Enforced [INFO]

**Problem:** Timeout config exists but isn't used.

**Files to modify:**
- `src/hooks.rs` (function `execute_sync` around line 542)

**Option A - Implement timeout (recommended):**
```rust
use std::time::Duration;
use std::process::Stdio;

fn execute_sync(&self, hook: &Hook, env_vars: &HashMap<String, String>) -> HookResult {
    let started = Instant::now();
    let working_dir = self.config.working_dir.clone()
        .unwrap_or_else(|| self.default_working_dir.clone());

    let timeout_secs = self.config.default_timeout_secs.unwrap_or(hook.timeout_secs);
    let timeout = Duration::from_secs(timeout_secs);

    let mut child = match Self::build_shell_command(&hook.command)
        .current_dir(&working_dir)
        .envs(env_vars)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return HookResult {
            name: hook.name.clone(),
            success: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration: started.elapsed(),
            error: Some(format!("Failed to spawn hook: {e}")),
        },
    };

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => {
            let stdout = child.stdout.take()
                .map(|mut s| { let mut buf = String::new(); s.read_to_string(&mut buf).ok(); buf })
                .unwrap_or_default();
            let stderr = child.stderr.take()
                .map(|mut s| { let mut buf = String::new(); s.read_to_string(&mut buf).ok(); buf })
                .unwrap_or_default();
            HookResult {
                name: hook.name.clone(),
                success: status.success(),
                exit_code: status.code(),
                stdout,
                stderr,
                duration: started.elapsed(),
                error: None,
            }
        }
        Ok(None) => {
            // Timeout - kill the process
            let _ = child.kill();
            HookResult {
                name: hook.name.clone(),
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                duration: started.elapsed(),
                error: Some(format!("Hook timed out after {}s", timeout_secs)),
            }
        }
        Err(e) => HookResult {
            name: hook.name.clone(),
            success: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration: started.elapsed(),
            error: Some(format!("Failed to wait for hook: {e}")),
        },
    }
}
```

**Note:** You'll need `wait_timeout` from the `wait-timeout` crate, or implement with threads.

**Option B - Remove unused config (simpler):**
- Remove `timeout_secs` from Hook struct
- Remove `default_timeout_secs` from HooksConfig
- Update config.example.toml if it mentions timeout

**Acceptance criteria (if implementing timeout):**
- [ ] Hooks killed after timeout
- [ ] Error message indicates timeout
- [ ] Test with sleep command exceeding timeout

---

## Final Checklist

Before declaring victory:

```bash
# All tests pass
cargo test --all-features

# No clippy warnings
cargo clippy --all-targets --all-features -- -D warnings

# Code formatted
cargo fmt --check

# Build succeeds
cargo build --release
```

## Commit Strategy

After each issue:
```bash
git add -A
git commit -m "fix(config): <issue description>

- <bullet point of changes>
- <bullet point of changes>

Fixes ISSUE-N from bug-hunt-fixes.md"
```

After all issues:
```bash
git commit -m "chore: complete config bug hunt sprint

Fixed 9 issues:
- Tilde path expansion
- Profile validation
- /yolo command parity
- trust_mode sync
- config.example.toml accuracy
- save_api_key precision
- README env vars
- Empty config validation
- Hook timeout enforcement"
```

---

## Questions? Blockers?

If you encounter:
- **Unclear requirements:** Make the safer choice and document your decision
- **Conflicting changes:** The higher-priority issue wins
- **Test failures unrelated to your change:** Note them but continue
- **Need for new dependencies:** Prefer stdlib solutions, but crates.io is fine if needed

Good luck, team. Ship it.
