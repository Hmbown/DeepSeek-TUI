//! System-skill installer: bundles skill-creator and auto-installs it on first launch.

use std::fs;
use std::path::Path;

const BUNDLED_SKILL_VERSION: &str = "1";
const SKILL_CREATOR_BODY: &str = include_str!("../../assets/skills/skill-creator/SKILL.md");

/// Install bundled system skills into `skills_dir`.
///
/// Behaviour:
/// - Fresh install (no marker, no dir): installs `skill-creator/SKILL.md` and writes
///   the version marker.
/// - Version bump (marker present with older version, dir present): re-installs.
/// - User deleted the dir while marker still present at same version: leaves it gone.
/// - Idempotent: calling twice with no changes is a no-op.
///
/// Errors are I/O errors from the filesystem; the caller should log them but not
/// abort startup.
pub fn install_system_skills(skills_dir: &Path) -> std::io::Result<()> {
    let marker = skills_dir.join(".system-installed-version");
    let target_dir = skills_dir.join("skill-creator");
    let target_file = target_dir.join("SKILL.md");

    let installed_version = fs::read_to_string(&marker)
        .ok()
        .map(|s| s.trim().to_string());
    let dir_exists = target_dir.exists();

    // Re-install only when BOTH conditions hold:
    //   (a) bundled version is newer than what is recorded in the marker, AND
    //   (b) the skill directory still exists (user hasn't intentionally deleted it).
    // Fresh install (no marker AND no dir) is also handled.
    let should_install = match (installed_version.as_deref(), dir_exists) {
        // Fresh install: neither marker nor directory.
        (None, false) => true,
        // Version bump: marker is outdated but directory still present.
        (Some(v), true) if v != BUNDLED_SKILL_VERSION => true,
        // Every other case: already installed at current version, or user deleted
        // the dir (respect that choice).
        _ => false,
    };

    if should_install {
        fs::create_dir_all(skills_dir)?;
        fs::create_dir_all(&target_dir)?;
        fs::write(&target_file, SKILL_CREATOR_BODY)?;
        fs::write(&marker, BUNDLED_SKILL_VERSION)?;
    }
    Ok(())
}

/// Remove the `skill-creator` system skill and its version marker.
///
/// Intended for tests and `deepseek setup --clean`.  Ignores missing files.
#[allow(dead_code)]
pub fn uninstall_system_skills(skills_dir: &Path) -> std::io::Result<()> {
    let marker = skills_dir.join(".system-installed-version");
    let target_dir = skills_dir.join("skill-creator");

    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)?;
    }
    if marker.exists() {
        fs::remove_file(&marker)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn skill_file(tmp: &TempDir) -> std::path::PathBuf {
        tmp.path().join("skill-creator").join("SKILL.md")
    }

    fn marker_file(tmp: &TempDir) -> std::path::PathBuf {
        tmp.path().join(".system-installed-version")
    }

    // ── fresh install ─────────────────────────────────────────────────────────

    #[test]
    fn fresh_install_creates_skill_and_marker() {
        let tmp = TempDir::new().unwrap();
        install_system_skills(tmp.path()).unwrap();

        assert!(skill_file(&tmp).exists(), "SKILL.md should be created");
        assert!(marker_file(&tmp).exists(), "marker should be created");

        let ver = fs::read_to_string(marker_file(&tmp)).unwrap();
        assert_eq!(ver.trim(), BUNDLED_SKILL_VERSION);
    }

    // ── idempotence ───────────────────────────────────────────────────────────

    #[test]
    fn calling_twice_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        install_system_skills(tmp.path()).unwrap();

        // Overwrite SKILL.md with sentinel to detect an undesired second write.
        fs::write(skill_file(&tmp), "sentinel").unwrap();

        install_system_skills(tmp.path()).unwrap();

        let contents = fs::read_to_string(skill_file(&tmp)).unwrap();
        assert_eq!(
            contents, "sentinel",
            "second install should not overwrite SKILL.md when version is current"
        );
    }

    // ── user deleted the directory ────────────────────────────────────────────

    #[test]
    fn user_deleted_dir_is_not_recreated() {
        let tmp = TempDir::new().unwrap();
        install_system_skills(tmp.path()).unwrap();

        // Simulate user deliberately removing the skill directory.
        fs::remove_dir_all(tmp.path().join("skill-creator")).unwrap();

        // Re-launch must NOT recreate the directory.
        install_system_skills(tmp.path()).unwrap();

        assert!(
            !skill_file(&tmp).exists(),
            "skill-creator must not be recreated after user deleted it"
        );
    }

    // ── version bump re-installs ──────────────────────────────────────────────

    #[test]
    fn outdated_marker_triggers_reinstall() {
        let tmp = TempDir::new().unwrap();

        // Simulate a previous install at a lower version.
        let skill_dir = tmp.path().join("skill-creator");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "old content").unwrap();
        fs::write(marker_file(&tmp), "0").unwrap(); // older than BUNDLED_SKILL_VERSION

        install_system_skills(tmp.path()).unwrap();

        let contents = fs::read_to_string(skill_file(&tmp)).unwrap();
        assert_ne!(
            contents, "old content",
            "outdated skill should be overwritten on version bump"
        );
        assert_eq!(
            contents, SKILL_CREATOR_BODY,
            "re-installed file must match the bundled body"
        );

        let ver = fs::read_to_string(marker_file(&tmp)).unwrap();
        assert_eq!(
            ver.trim(),
            BUNDLED_SKILL_VERSION,
            "marker should be updated"
        );
    }

    // ── uninstall ─────────────────────────────────────────────────────────────

    #[test]
    fn uninstall_removes_skill_and_marker() {
        let tmp = TempDir::new().unwrap();
        install_system_skills(tmp.path()).unwrap();
        uninstall_system_skills(tmp.path()).unwrap();

        assert!(!skill_file(&tmp).exists(), "SKILL.md should be removed");
        assert!(!marker_file(&tmp).exists(), "marker should be removed");
    }

    #[test]
    fn uninstall_on_clean_dir_is_a_noop() {
        let tmp = TempDir::new().unwrap();
        // Must not panic or error.
        uninstall_system_skills(tmp.path()).unwrap();
    }
}
