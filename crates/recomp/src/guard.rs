//! Where `uu-recomp` may write. The library carries the owner's game code, so the output is
//! never inside a git checkout (it could be committed) or inside the dump.

use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Why an output folder was refused, or could not be resolved.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// The folder, or one above it, is a git checkout (has a `.git`).
    InGitCheckout(PathBuf),
    /// The folder is inside the game dump it is built from.
    InGameDir,
    /// The path could not be resolved (a component is not a folder, or a symlink loops).
    Unresolvable(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::InGitCheckout(root) => write!(
                f,
                "the output folder is inside the git checkout {}: generated code must never be committed",
                root.display()
            ),
            Refused::InGameDir => write!(f, "the output folder is inside the game folder"),
            Refused::Unresolvable(why) => write!(f, "the output folder cannot be resolved: {why}"),
        }
    }
}

impl std::error::Error for Refused {}

/// `~` and `~/...` at the start of a path, with `home` for the home folder.
pub fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix("~/"), path == "~", home) {
        (Some(rest), _, Some(home)) => home.join(rest),
        (_, true, Some(home)) => home.to_path_buf(),
        _ => PathBuf::from(path),
    }
}

/// The absolute, symlink-free form of `path` even where the last components do not exist yet:
/// the deepest existing ancestor is canonicalized and the rest is appended, `..` resolved.
pub fn resolve(path: &Path) -> Result<PathBuf, Refused> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| Refused::Unresolvable(e.to_string()))?
            .join(path)
    };
    let mut normal = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other.as_os_str()),
        }
    }
    let mut existing = normal.clone();
    let mut rest = Vec::new();
    while !existing.exists() {
        match existing.file_name() {
            Some(name) => rest.push(name.to_owned()),
            None => break,
        }
        if !existing.pop() {
            break;
        }
    }
    let mut resolved = existing
        .canonicalize()
        .map_err(|e| Refused::Unresolvable(format!("{}: {e}", existing.display())))?;
    for name in rest.iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

/// The nearest folder at or above `path` that holds a `.git` (a folder, or the file a worktree
/// or submodule has).
pub fn git_checkout_of(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|dir| dir.join(".git").symlink_metadata().is_ok())
        .map(Path::to_path_buf)
}

/// The resolved output folder, or why it is refused.
pub fn check_output_dir(out: &Path, game_dir: &Path) -> Result<PathBuf, Refused> {
    let out = resolve(out)?;
    if let Some(root) = git_checkout_of(&out) {
        return Err(Refused::InGitCheckout(root));
    }
    if let Ok(game) = resolve(game_dir) {
        if out.starts_with(&game) {
            return Err(Refused::InGameDir);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("uu-recomp-guard-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn home_is_expanded_only_at_the_start() {
        let home = Path::new("/Users/someone");
        assert_eq!(
            expand_home("~/Games/x", Some(home)),
            Path::new("/Users/someone/Games/x")
        );
        assert_eq!(expand_home("~", Some(home)), home);
        assert_eq!(expand_home("a/~/b", Some(home)), Path::new("a/~/b"));
        assert_eq!(expand_home("~/x", None), Path::new("~/x"));
    }

    #[test]
    fn a_folder_that_does_not_exist_yet_is_resolved_under_its_ancestor() {
        let dir = temp("resolve");
        let out = resolve(&dir.join("a/b/../c")).unwrap();
        assert_eq!(out, dir.join("a/c"));
    }

    #[test]
    fn a_git_checkout_refuses_every_folder_in_it_even_one_not_yet_made() {
        let dir = temp("git");
        fs::create_dir_all(dir.join(".git")).unwrap();
        let game = temp("git-game");
        for inside in [dir.clone(), dir.join("recomp-out/u1"), dir.join("a/b/c")] {
            assert_eq!(
                check_output_dir(&inside, &game),
                Err(Refused::InGitCheckout(dir.clone())),
                "{}",
                inside.display()
            );
        }
    }

    #[test]
    fn a_worktree_git_file_counts_as_a_checkout() {
        let dir = temp("worktree");
        fs::write(dir.join(".git"), "gitdir: /elsewhere\n").unwrap();
        let game = temp("worktree-game");
        assert!(matches!(
            check_output_dir(&dir.join("out"), &game),
            Err(Refused::InGitCheckout(_))
        ));
    }

    #[test]
    fn a_symlink_into_a_checkout_does_not_get_around_it() {
        let dir = temp("symlink");
        fs::create_dir_all(dir.join("repo/.git")).unwrap();
        fs::create_dir_all(dir.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(dir.join("repo"), dir.join("elsewhere/link")).unwrap();
        let game = temp("symlink-game");
        assert!(matches!(
            check_output_dir(&dir.join("elsewhere/link/out"), &game),
            Err(Refused::InGitCheckout(_))
        ));
    }

    #[test]
    fn the_game_folder_is_refused_and_a_plain_folder_is_not() {
        let dir = temp("plain");
        let game = dir.join("game");
        fs::create_dir_all(&game).unwrap();
        assert_eq!(
            check_output_dir(&game.join("out"), &game),
            Err(Refused::InGameDir)
        );
        assert_eq!(
            check_output_dir(&dir.join("out/u1"), &game),
            Ok(dir.join("out/u1"))
        );
    }
}
