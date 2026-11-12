//! `rm -rf` (`compat_remove_tree` of `compat_win.c`): deleting a save slot and its
//! `.sce_sys` folder (`runtime_savedata.c`).

use std::fs;
use std::io;
use std::path::Path;

/// Removes `path` and, for a directory, everything under it. Nothing at `path` is success.
///
/// A link (a symbolic link or, on Windows, any reparse point such as a junction) is removed
/// itself, never what it points to. On Windows a read-only file is made writable first, as
/// `DeleteFile` refuses it. An entry that cannot be removed does not stop the rest: the
/// first error is returned once everything else has been tried.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    let entry = match Entry::of(path) {
        Ok(entry) => entry,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !entry.directory || entry.link {
        if entry.read_only {
            make_writable(path);
        }
        return if entry.directory {
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        };
    }
    let mut first_error = None;
    if let Ok(children) = fs::read_dir(path) {
        for child in children {
            let result = child.and_then(|child| remove_tree(&child.path()));
            if let Err(error) = result {
                first_error.get_or_insert(error);
            }
        }
    }
    let removed = fs::remove_dir(path);
    match first_error {
        Some(error) => Err(error),
        None => removed,
    }
}

/// What `remove_tree` needs to know about one path, without following a link.
struct Entry {
    /// A directory, or a link to one (Windows keeps the directory flag on the link).
    directory: bool,
    link: bool,
    read_only: bool,
}

impl Entry {
    #[cfg(windows)]
    fn of(path: &Path) -> io::Result<Self> {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        let attributes = fs::symlink_metadata(path)?.file_attributes();
        Ok(Self {
            directory: attributes & FILE_ATTRIBUTE_DIRECTORY != 0,
            link: attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
            read_only: attributes & FILE_ATTRIBUTE_READONLY != 0,
        })
    }

    // Unlinking needs no write permission on the entry itself, only on its directory.
    #[cfg(not(windows))]
    fn of(path: &Path) -> io::Result<Self> {
        let kind = fs::symlink_metadata(path)?.file_type();
        Ok(Self {
            directory: kind.is_dir(),
            link: kind.is_symlink(),
            read_only: false,
        })
    }
}

/// Clears `FILE_ATTRIBUTE_READONLY` on `path` itself (not on a link's target). A failure
/// shows up as the removal's own error.
#[cfg(windows)]
fn make_writable(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_READONLY};
    let Ok(attributes) = fs::symlink_metadata(path).map(|m| m.file_attributes()) else {
        return;
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call.
    unsafe { SetFileAttributesW(wide.as_ptr(), attributes & !FILE_ATTRIBUTE_READONLY) };
}

#[cfg(not(windows))]
fn make_writable(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("uu-os-tree-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = remove_tree(&self.0);
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn read_only(path: &Path) {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions).unwrap();
    }

    /// A save slot as `runtime_savedata.c` deletes it: nested folders, made-up files, one of
    /// them read-only.
    #[test]
    fn os_remove_tree_removes_a_nested_tree_with_a_read_only_file() {
        let dir = TempDir::new("nested");
        let slot = dir.0.join("SAVEDATA00");
        fs::create_dir_all(slot.join("sce_sys/deeper")).unwrap();
        fs::write(slot.join("made-up.bin"), b"not a save").unwrap();
        fs::write(slot.join("sce_sys/param.sfo"), b"made up").unwrap();
        fs::write(slot.join("sce_sys/deeper/icon0.png"), b"made up").unwrap();
        read_only(&slot.join("sce_sys/param.sfo"));

        remove_tree(&slot).unwrap();
        assert!(!slot.exists());
        assert!(dir.0.exists(), "only the tree goes");
    }

    #[test]
    fn os_remove_tree_of_nothing_succeeds() {
        let dir = TempDir::new("missing");
        remove_tree(&dir.0.join("never-there")).unwrap();
        remove_tree(&dir.0.join("never-there/below")).unwrap();
    }

    #[test]
    fn os_remove_tree_removes_a_single_file_even_read_only() {
        let dir = TempDir::new("file");
        let file = dir.0.join("made-up.bin");
        fs::write(&file, b"made up").unwrap();
        read_only(&file);
        remove_tree(&file).unwrap();
        assert!(!file.exists());
        fs::create_dir(dir.0.join("empty")).unwrap();
        remove_tree(&dir.0.join("empty")).unwrap();
        assert!(!dir.0.join("empty").exists());
    }

    /// A link inside the tree goes; what it points to stays. Windows lets a test make a
    /// symbolic link only with Developer Mode or as Administrator: without either the check
    /// is skipped there.
    #[test]
    fn os_remove_tree_removes_links_not_their_targets() {
        let dir = TempDir::new("links");
        let outside = dir.0.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep.bin"), b"keep").unwrap();
        let tree = dir.0.join("tree");
        fs::create_dir(&tree).unwrap();

        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&outside, tree.join("to-outside")).and_then(|()| {
            std::os::unix::fs::symlink(outside.join("keep.bin"), tree.join("to-file"))
        });
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(&outside, tree.join("to-outside")).and_then(
            |()| std::os::windows::fs::symlink_file(outside.join("keep.bin"), tree.join("to-file")),
        );
        #[cfg(not(any(unix, windows)))]
        let made: io::Result<()> = Err(io::ErrorKind::Unsupported.into());
        if let Err(error) = made {
            eprintln!("skipped: cannot make a symbolic link here ({error})");
            return;
        }

        remove_tree(&tree).unwrap();
        assert!(!tree.exists());
        assert_eq!(fs::read(outside.join("keep.bin")).unwrap(), b"keep");

        // The link itself as the path: only the link goes.
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, dir.0.join("link")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&outside, dir.0.join("link")).unwrap();
        remove_tree(&dir.0.join("link")).unwrap();
        assert!(fs::symlink_metadata(dir.0.join("link")).is_err());
        assert!(outside.join("keep.bin").exists());
    }

    /// An entry that cannot go does not stop the others; the error still comes back.
    #[cfg(unix)]
    #[test]
    fn os_remove_tree_keeps_going_past_an_entry_it_cannot_remove() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("stuck");
        let tree = dir.0.join("tree");
        fs::create_dir_all(tree.join("locked")).unwrap();
        fs::write(tree.join("locked/inside.bin"), b"made up").unwrap();
        fs::write(tree.join("free.bin"), b"made up").unwrap();
        fs::set_permissions(tree.join("locked"), fs::Permissions::from_mode(0o500)).unwrap();
        // Root may unlink anyway: then there is nothing to check.
        let root = fs::write(tree.join("locked/probe"), b"").is_ok();

        let result = remove_tree(&tree);
        fs::set_permissions(tree.join("locked"), fs::Permissions::from_mode(0o700)).ok();
        if root {
            return;
        }
        assert!(result.is_err());
        assert!(!tree.join("free.bin").exists(), "the rest was still removed");
        assert!(tree.join("locked/inside.bin").exists());
    }
}
