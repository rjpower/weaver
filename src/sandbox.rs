use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SandboxLevel {
    Readonly,
    #[default]
    DefaultDev,
    Unrestricted,
}

impl fmt::Display for SandboxLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Readonly => write!(f, "readonly"),
            Self::DefaultDev => write!(f, "default_dev"),
            Self::Unrestricted => write!(f, "unrestricted"),
        }
    }
}

impl FromStr for SandboxLevel {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "readonly" => Ok(Self::Readonly),
            "default_dev" => Ok(Self::DefaultDev),
            "unrestricted" => Ok(Self::Unrestricted),
            _ => anyhow::bail!(
                "unknown sandbox level: {s} (expected: readonly, default_dev, unrestricted)"
            ),
        }
    }
}

/// Resolve the actual .git directory for a worktree.
/// If work_dir/.git is a file (worktree), read it to find the real gitdir,
/// then return the parent's parent (the .git root).
/// If it's a directory (main repo), return work_dir/.git directly.
pub fn resolve_git_dir(work_dir: &Path) -> Option<PathBuf> {
    let dot_git = work_dir.join(".git");
    if dot_git.is_file() {
        let content = std::fs::read_to_string(&dot_git).ok()?;
        let gitdir = content.strip_prefix("gitdir: ")?.trim();
        let path = Path::new(gitdir);
        Some(path.parent()?.parent()?.to_path_buf())
    } else if dot_git.is_dir() {
        Some(dot_git)
    } else {
        None
    }
}

/// Generate a macOS SBPL sandbox profile string for the given level and working directory.
///
/// Panics if called with `SandboxLevel::Unrestricted` — callers must skip sandboxing for that level.
pub fn generate_profile(level: SandboxLevel, work_dir: &Path) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let weaver_dir = format!("{home}/.weaver");
    let claude_dir = format!("{home}/.claude");

    let common = r#"(version 1)
(deny default)
(allow file-read*)
(allow process-exec)
(allow process-fork)
(allow signal)
(allow sysctl-read)
(allow mach-lookup)
(allow mach-register)
(allow ipc-posix-shm)
(allow file-ioctl)
(allow file-write* (subpath "/private/tmp"))
(allow file-write* (subpath "/private/var/folders"))
(allow file-write* (literal "/dev/null"))
(allow file-write* (literal "/dev/tty"))"#;

    match level {
        SandboxLevel::Readonly => {
            format!(
                "{common}\n\
                 (allow network-outbound (remote ip \"localhost:*\"))\n\
                 (allow network-bind (local ip \"localhost:*\"))\n\
                 (allow file-write* (subpath \"{weaver_dir}\"))\n\
                 (allow file-write* (subpath \"{claude_dir}\"))"
            )
        }
        SandboxLevel::DefaultDev => {
            let work_dir_str = work_dir.to_string_lossy();
            let cargo_dir = format!("{home}/.cargo");
            let npm_dir = format!("{home}/.npm");

            let mut profile = format!(
                "{common}\n\
                 (allow network-outbound)\n\
                 (allow system-socket)\n\
                 (allow file-write* (subpath \"{work_dir_str}\"))\n\
                 (allow file-write* (subpath \"{weaver_dir}\"))\n\
                 (allow file-write* (subpath \"{claude_dir}\"))\n\
                 (allow file-write* (subpath \"{cargo_dir}\"))\n\
                 (allow file-write* (subpath \"{npm_dir}\"))\n\
                 (allow network-bind (local ip \"localhost:*\"))\n\
                 (allow network-inbound (local ip \"localhost:*\"))\n\
                 (allow file-write* (literal \"/dev/dtracehelper\"))"
            );

            if let Some(git_root) = resolve_git_dir(work_dir) {
                let git_str = git_root.to_string_lossy();
                profile.push_str(&format!("\n(allow file-write* (subpath \"{git_str}\"))"));
            }

            profile
        }
        SandboxLevel::Unrestricted => {
            unreachable!("should not generate profile for unrestricted")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_sandbox_level_from_str() {
        assert_eq!(
            "readonly".parse::<SandboxLevel>().unwrap(),
            SandboxLevel::Readonly
        );
        assert_eq!(
            "default_dev".parse::<SandboxLevel>().unwrap(),
            SandboxLevel::DefaultDev
        );
        assert_eq!(
            "unrestricted".parse::<SandboxLevel>().unwrap(),
            SandboxLevel::Unrestricted
        );
        assert!("bogus".parse::<SandboxLevel>().is_err());
    }

    #[test]
    fn test_sandbox_level_display() {
        for level in [
            SandboxLevel::Readonly,
            SandboxLevel::DefaultDev,
            SandboxLevel::Unrestricted,
        ] {
            let s = level.to_string();
            let parsed: SandboxLevel = s.parse().unwrap();
            assert_eq!(parsed, level);
        }
    }

    #[test]
    fn test_sandbox_level_serde() {
        for level in [
            SandboxLevel::Readonly,
            SandboxLevel::DefaultDev,
            SandboxLevel::Unrestricted,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let back: SandboxLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(back, level);
        }
    }

    #[test]
    fn test_readonly_profile_no_workdir_write() {
        let dir = TempDir::new().unwrap();
        let profile = generate_profile(SandboxLevel::Readonly, dir.path());
        let dir_str = dir.path().to_string_lossy();
        assert!(
            !profile.contains(&*dir_str),
            "readonly profile should not contain work_dir subpath"
        );
    }

    #[test]
    fn test_default_dev_profile_has_workdir() {
        let dir = TempDir::new().unwrap();
        let profile = generate_profile(SandboxLevel::DefaultDev, dir.path());
        let dir_str = dir.path().to_string_lossy();
        assert!(
            profile.contains(&*dir_str),
            "default_dev profile should contain work_dir subpath"
        );
    }

    #[test]
    fn test_default_dev_profile_has_git_dir() {
        let dir = TempDir::new().unwrap();
        let fake_git = TempDir::new().unwrap();
        let worktree_dir = fake_git.path().join(".git/worktrees/test");
        fs::create_dir_all(&worktree_dir).unwrap();

        fs::write(
            dir.path().join(".git"),
            format!("gitdir: {}\n", worktree_dir.display()),
        )
        .unwrap();

        let profile = generate_profile(SandboxLevel::DefaultDev, dir.path());
        let git_str = fake_git.path().join(".git").to_string_lossy().to_string();
        assert!(
            profile.contains(&git_str),
            "default_dev profile should contain resolved .git dir: {git_str}\nprofile: {profile}"
        );
    }

    #[test]
    fn test_resolve_git_dir_worktree() {
        let work = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        let worktree_dir = repo.path().join(".git/worktrees/mybranch");
        fs::create_dir_all(&worktree_dir).unwrap();

        fs::write(
            work.path().join(".git"),
            format!("gitdir: {}\n", worktree_dir.display()),
        )
        .unwrap();

        let resolved = resolve_git_dir(work.path()).unwrap();
        assert_eq!(resolved, repo.path().join(".git"));
    }

    #[test]
    fn test_resolve_git_dir_regular_repo() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let resolved = resolve_git_dir(dir.path()).unwrap();
        assert_eq!(resolved, dir.path().join(".git"));
    }

    #[test]
    fn test_resolve_git_dir_missing() {
        let dir = TempDir::new().unwrap();
        assert!(resolve_git_dir(dir.path()).is_none());
    }

    #[test]
    fn test_readonly_profile_denies_general_network() {
        let dir = TempDir::new().unwrap();
        let profile = generate_profile(SandboxLevel::Readonly, dir.path());
        assert!(
            !profile.contains("(allow network-outbound)"),
            "readonly profile should not allow general network-outbound"
        );
        assert!(
            !profile.contains("(allow system-socket)"),
            "readonly profile should not allow system-socket"
        );
        assert!(
            profile.contains(r#"(allow network-outbound (remote ip "localhost:*"))"#),
            "readonly profile should allow localhost-only network"
        );
    }

    #[test]
    fn test_default_dev_profile_has_network() {
        let dir = TempDir::new().unwrap();
        let profile = generate_profile(SandboxLevel::DefaultDev, dir.path());
        assert!(
            profile.contains("(allow network-outbound)"),
            "default_dev profile should allow network-outbound"
        );
        assert!(
            profile.contains("(allow system-socket)"),
            "default_dev profile should allow system-socket"
        );
    }
}
