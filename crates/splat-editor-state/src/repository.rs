//! The code host of the Git repository a document lies in, which `@name`, `#12` and `!7` in its text point into.

use std::path::{Path, PathBuf};

/// A repository on GitHub or GitLab, which `@name`, `#123` and `!123` point into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    host: String,
    /// `owner/name`, or deeper with GitLab's groups.
    path: String,
    gitlab: bool,
}

impl Repository {
    /// The web page a reference such as `@anna`, `#12` or `!7` stands for.
    pub fn url(&self, reference: &str) -> Option<String> {
        let (kind, name) = reference.split_at_checked(1)?;
        let base = format!("https://{}", self.host);
        match (kind, self.gitlab) {
            ("@", _) => Some(format!("{base}/{name}")),
            ("#", false) => Some(format!("{base}/{}/issues/{name}", self.path)),
            ("#", true) => Some(format!("{base}/{}/-/issues/{name}", self.path)),
            ("!", true) => Some(format!("{base}/{}/-/merge_requests/{name}", self.path)),
            _ => None,
        }
    }
}

/// The folder of the repository `dir` lies in.
pub fn repository_root(dir: &Path) -> Option<&Path> {
    dir.ancestors().find(|d| d.join(".git").exists())
}

/// The repository the folder lies in, from the `origin` remote of its Git configuration. It reads the disk.
pub fn find_repository(dir: &Path) -> Option<Repository> {
    let git = dir.ancestors().map(|d| d.join(".git")).find(|g| g.exists())?;
    let git_dir = if git.is_file() {
        // A worktree or submodule names its real Git folder in a file.
        let text = git_file(&git)?;
        let target = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
        let target = if target.is_absolute() {
            target
        } else {
            git.parent()?.join(target)
        };
        let common = target.join("commondir");
        match git_file(&common) {
            Some(rel) => target.join(rel.trim()),
            None => target,
        }
    } else {
        git
    };
    let config = git_file(&git_dir.join("config"))?;
    remote_repository(&config)
}

/// Far more than any Git configuration holds.
const MAX_GIT_FILE: u64 = 1 << 20;

/// The text of a small regular file: opening a named pipe from an unpacked archive would wait for a writer for good.
fn git_file(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_GIT_FILE {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// The repository the `origin` remote of a Git configuration points to.
pub fn remote_repository(config: &str) -> Option<Repository> {
    let mut in_origin = false;
    let mut url = None;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line.replace(' ', "") == "[remote\"origin\"]";
        } else if in_origin
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "url"
        {
            url = Some(value.trim().to_string());
        }
    }
    let url = url?;
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://"))
        .unwrap_or(&url);
    let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
    let (host, path) = rest.split_once([':', '/'])?;
    // `ssh://host:2222/group/repo`: the port is no part of the web address.
    let path = match path.split_once('/') {
        Some((port, tail)) if port.chars().all(|c| c.is_ascii_digit()) => tail,
        _ => path,
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git").to_string();
    let host = host.to_ascii_lowercase();
    let gitlab = host.contains("gitlab");
    (host == "github.com" || gitlab).then_some(Repository { host, path, gitlab })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_lead_to_their_code_host() {
        let config = |url: &str| format!("[core]\n\tbare = false\n[remote \"origin\"]\n\turl = {url}\n");
        let github = remote_repository(&config("git@github.com:anna/splat.git")).unwrap();
        assert_eq!(
            github.url("#12").as_deref(),
            Some("https://github.com/anna/splat/issues/12")
        );
        assert_eq!(github.url("@bob").as_deref(), Some("https://github.com/bob"));
        assert_eq!(github.url("!3"), None);
        assert_eq!(github.url(""), None);
        let gitlab = remote_repository(&config("https://gitlab.example.com/team/sub/app.git")).unwrap();
        assert_eq!(
            gitlab.url("!7").as_deref(),
            Some("https://gitlab.example.com/team/sub/app/-/merge_requests/7")
        );
        let port = remote_repository(&config("ssh://git@gitlab.example.com:2222/team/app.git")).unwrap();
        assert_eq!(
            port.url("#1").as_deref(),
            Some("https://gitlab.example.com/team/app/-/issues/1")
        );
        assert_eq!(remote_repository(&config("https://example.com/x/y.git")), None);
        assert_eq!(
            remote_repository("[remote \"upstream\"]\n\turl = git@github.com:a/b.git\n"),
            None
        );
    }

    #[test]
    fn worktrees_are_found_from_a_folder_inside_them() {
        let root = std::env::temp_dir().join(format!("splat-worktree-{}", std::process::id()));
        let main_git = root.join("main/.git");
        let worktree = root.join("worktree");
        std::fs::create_dir_all(main_git.join("worktrees/wt")).unwrap();
        std::fs::create_dir_all(worktree.join("docs")).unwrap();
        std::fs::write(
            main_git.join("config"),
            "[remote \"origin\"]\n\turl = git@github.com:anna/splat.git\n",
        )
        .unwrap();
        std::fs::write(main_git.join("worktrees/wt/commondir"), "../..\n").unwrap();
        std::fs::write(worktree.join(".git"), "gitdir: ../main/.git/worktrees/wt\n").unwrap();
        let found = find_repository(&worktree.join("docs")).unwrap();
        assert_eq!(
            found.url("#1").as_deref(),
            Some("https://github.com/anna/splat/issues/1")
        );
        assert_eq!(repository_root(&worktree.join("docs")), Some(worktree.as_path()));
        assert_eq!(
            find_repository(&root.join("main")).and_then(|r| r.url("@a")).as_deref(),
            Some("https://github.com/a")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_pipe_in_place_of_a_git_file_is_not_opened() {
        let root = std::env::temp_dir().join(format!("splat-git-pipe-{}", std::process::id()));
        let git = root.join(".git");
        std::fs::create_dir_all(&git).unwrap();
        let Ok(status) = std::process::Command::new("mkfifo").arg(git.join("config")).status() else {
            eprintln!("mkfifo is absent: skipped");
            return;
        };
        assert!(status.success());
        assert!(find_repository(&root).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
