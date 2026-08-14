use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanDepth {
    All,
    Depth(usize),
}

impl ScanDepth {
    fn max_depth(self) -> Option<usize> {
        match self {
            ScanDepth::All => None,
            ScanDepth::Depth(depth) => Some(depth),
        }
    }
}

pub fn parse_scan_depth(value: &str) -> Result<ScanDepth, String> {
    let normalized = value.trim();
    if normalized.eq_ignore_ascii_case("all") {
        return Ok(ScanDepth::All);
    }

    let depth: usize = normalized
        .parse()
        .map_err(|_| format!("invalid scan depth: {value}. Use a positive integer or \"all\"."))?;

    if depth == 0 {
        return Err("scan depth must be a positive integer or \"all\"".to_string());
    }

    Ok(ScanDepth::Depth(depth))
}

/// Check if the current working directory is inside a git repository.
/// Walks up from the cwd looking for `.git` (directory or gitlink file).
/// Honors `GIT_DIR` if set. Avoids spawning `git` so the common "not in a
/// repo" path stays cheap.
pub fn is_inside_git_repo() -> bool {
    if std::env::var_os("GIT_DIR").is_some() {
        return true;
    }
    let Ok(start) = std::env::current_dir() else {
        return false;
    };
    let mut cur: &Path = start.as_path();
    loop {
        if cur.join(".git").exists() {
            return true;
        }
        match cur.parent() {
            Some(parent) => cur = parent,
            None => return false,
        }
    }
}

/// Find all git repositories under the given root, honoring scan depth.
pub fn find_git_repos_in(root: &Path, scan_depth: ScanDepth) -> Result<Vec<PathBuf>> {
    let mut repos = Vec::new();
    scan_dir(root, 0, scan_depth.max_depth(), &mut repos)?;
    repos.sort();
    Ok(repos)
}

fn scan_dir(
    dir: &Path,
    depth: usize,
    max_depth: Option<usize>,
    repos: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();

        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            let git_dir = path.join(".git");
            if git_dir.exists() {
                repos.push(path);
                continue;
            }

            let next_depth = depth + 1;
            let should_descend = max_depth.is_none_or(|max| next_depth < max);
            if should_descend {
                // A subdirectory that's unreadable (permissions, or removed
                // mid-scan) shouldn't abort discovery for the rest of the
                // workspace; skip that subtree and keep going.
                let _ = scan_dir(&path, next_depth, max_depth, repos);
            }
        }
    }

    Ok(())
}

/// Extract just the repository name from a path
pub fn repo_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Display a repository path relative to the given root when possible.
pub fn repo_display_name(path: &Path, root: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.to_string_lossy().to_string(),
        _ => repo_name(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_repo_name() {
        let path = PathBuf::from("/home/user/src/my-repo");
        assert_eq!(repo_name(&path), "my-repo");
    }

    #[test]
    fn test_repo_name_root() {
        let path = PathBuf::from("/");
        assert_eq!(repo_name(&path), "unknown");
    }

    #[test]
    fn test_repo_display_name_relative() {
        let root = PathBuf::from("/tmp/workspace");
        let repo = root.join("nested").join("repo");
        let expected = PathBuf::from("nested").join("repo");
        assert_eq!(repo_display_name(&repo, &root), expected.to_string_lossy());
    }

    #[test]
    fn test_repo_display_name_fallback() {
        let root = PathBuf::from("/tmp/workspace");
        let repo = PathBuf::from("/other/place/repo");
        assert_eq!(repo_display_name(&repo, &root), "repo");
    }

    #[test]
    fn test_parse_scan_depth() {
        assert_eq!(parse_scan_depth("1").unwrap(), ScanDepth::Depth(1));
        assert_eq!(parse_scan_depth("all").unwrap(), ScanDepth::All);
        assert_eq!(parse_scan_depth("ALL").unwrap(), ScanDepth::All);
        assert!(parse_scan_depth("0").is_err());
        assert!(parse_scan_depth("nope").is_err());
    }

    #[test]
    fn test_find_git_repos_depth_limits() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path();

        create_repo(root.join("repo1"), true);
        create_repo(root.join("repo2"), false);
        create_repo(root.join("nested/repo3"), true);
        create_repo(root.join("nested/deeper/repo4"), true);
        create_repo(root.join("boundary"), true);
        create_repo(root.join("boundary/child"), true);

        let mut depth1 = find_git_repos_in(root, ScanDepth::Depth(1)).unwrap();
        let mut expected_depth1 = vec![
            root.join("boundary"),
            root.join("repo1"),
            root.join("repo2"),
        ];
        depth1.sort();
        expected_depth1.sort();
        assert_eq!(depth1, expected_depth1);

        let mut depth2 = find_git_repos_in(root, ScanDepth::Depth(2)).unwrap();
        let mut expected_depth2 = vec![
            root.join("boundary"),
            root.join("repo1"),
            root.join("repo2"),
            root.join("nested/repo3"),
        ];
        depth2.sort();
        expected_depth2.sort();
        assert_eq!(depth2, expected_depth2);

        let mut depth_all = find_git_repos_in(root, ScanDepth::All).unwrap();
        let mut expected_depth_all = vec![
            root.join("boundary"),
            root.join("repo1"),
            root.join("repo2"),
            root.join("nested/repo3"),
            root.join("nested/deeper/repo4"),
        ];
        depth_all.sort();
        expected_depth_all.sort();
        assert_eq!(depth_all, expected_depth_all);
    }

    #[test]
    #[cfg(unix)]
    fn test_find_git_repos_skips_unreadable_subdirectory() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path();

        create_repo(root.join("good-repo"), true);

        let blocked = root.join("blocked");
        create_repo(blocked.join("hidden-repo"), true);
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).expect("chmod 000");

        let result = find_git_repos_in(root, ScanDepth::All);

        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755))
            .expect("restore permissions for cleanup");

        assert_eq!(result.unwrap(), vec![root.join("good-repo")]);
    }

    #[test]
    #[cfg(unix)]
    fn test_find_git_repos_skips_symlinks() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path();

        create_repo(root.join("real-repo"), true);
        symlink(root.join("real-repo"), root.join("link-to-repo")).expect("create symlink");

        fs::create_dir_all(root.join("plain-dir")).expect("create plain dir");
        create_repo(root.join("plain-dir/nested-repo"), true);
        symlink(root.join("plain-dir"), root.join("link-to-dir")).expect("create symlink");

        let repos = find_git_repos_in(root, ScanDepth::All).unwrap();
        assert_eq!(
            repos,
            vec![root.join("plain-dir/nested-repo"), root.join("real-repo")]
        );
    }

    fn create_repo(path: PathBuf, git_dir: bool) {
        fs::create_dir_all(&path).expect("create repo dir");
        let git_path = path.join(".git");
        if git_dir {
            fs::create_dir_all(git_path).expect("create .git dir");
        } else {
            fs::write(git_path, "").expect("create .git file");
        }
    }
}
