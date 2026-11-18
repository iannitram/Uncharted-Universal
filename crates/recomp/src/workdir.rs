//! Generated C and libraries go to a work folder outside the clone, never into the repo.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum WorkDirError {
    InsideRepo,
    NotAbsolute,
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve the folder for `title` under `requested`; refuse a folder inside `repo_root`.
/// Symlinks are resolved on the part that exists.
pub fn resolve(requested: &Path, repo_root: &Path, title: &str) -> Result<PathBuf, WorkDirError> {
    if !requested.is_absolute() {
        return Err(WorkDirError::NotAbsolute);
    }
    let real = |p: &Path| {
        let n = normalize(p);
        let mut tail = Vec::new();
        let mut cur = n.as_path();
        loop {
            if let Ok(c) = cur.canonicalize() {
                return tail
                    .iter()
                    .rev()
                    .fold(c, |a, t: &&std::ffi::OsStr| a.join(t));
            }
            match (cur.parent(), cur.file_name()) {
                (Some(par), Some(name)) => {
                    tail.push(name);
                    cur = par;
                }
                _ => return n,
            }
        }
    };
    let dir = real(requested);
    if dir.starts_with(real(repo_root)) {
        return Err(WorkDirError::InsideRepo);
    }
    Ok(dir.join(title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_inside_the_clone() {
        let t = std::env::temp_dir().join("uu_recomp_wd_test");
        let repo = t.join("clone");
        assert_eq!(
            resolve(&repo.join("out"), &repo, "u1"),
            Err(WorkDirError::InsideRepo)
        );
        assert_eq!(
            resolve(&repo.join("x/../out"), &repo, "u1"),
            Err(WorkDirError::InsideRepo)
        );
        assert_eq!(
            resolve(Path::new("out"), &repo, "u1"),
            Err(WorkDirError::NotAbsolute)
        );
        let ok = resolve(&t.join("out"), &repo, "u1");
        assert!(ok.is_ok_and(|p| p.ends_with("out/u1")));
    }
}
