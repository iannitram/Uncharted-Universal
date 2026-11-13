//! `tests/test_file_mods.c`'s cases, and the rest of `src/runtime_file.c`'s behaviour: path
//! translation, case folding, the mod overlay, positioned and sequential IO, listings. The
//! mount and descriptor tables are process-wide, so every test holds [`LOCK`].

use super::*;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

/// A game tree and a user directory under a fresh temporary directory, configured as the
/// mounts; removed again on drop.
struct Scratch {
    root: PathBuf,
    _guard: MutexGuard<'static, ()>,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let guard = lock(&LOCK);
        let root = std::env::temp_dir().join(format!("uu-file-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("game")).unwrap();
        configure(bytes(&root.join("game")), bytes(&root.join("user")));
        Self {
            root,
            _guard: guard,
        }
    }

    fn game(&self, name: &str) -> PathBuf {
        self.root.join("game").join(name)
    }

    fn user(&self, name: &str) -> PathBuf {
        self.root.join("user").join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn bytes(path: &Path) -> &[u8] {
    path.to_str().unwrap().as_bytes()
}

/// A mod file linked into the game tree: a symbolic link, or a hard link on Windows
/// (`bb_test_link` in `tests/portable.h`).
fn link(target: &Path, at: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, at).unwrap();
    #[cfg(not(unix))]
    fs::hard_link(target, at).unwrap();
}

fn read(fd: c_int, size: usize) -> Result<Vec<u8>> {
    let mut buffer = vec![0u8; size];
    // SAFETY: `size` writable bytes.
    let n = unsafe { read_into(fd, buffer.as_mut_ptr(), size as u64, -1) }?;
    buffer.truncate(n as usize);
    Ok(buffer)
}

fn pread(fd: c_int, size: usize, offset: i64) -> Result<Vec<u8>> {
    let mut buffer = vec![0u8; size];
    // SAFETY: `size` writable bytes.
    let n = unsafe { read_into(fd, buffer.as_mut_ptr(), size as u64, offset) }?;
    buffer.truncate(n as usize);
    Ok(buffer)
}

fn write(fd: c_int, data: &[u8]) -> Result<u64> {
    // SAFETY: `data` is readable.
    unsafe { write_from(fd, data.as_ptr(), data.len() as u64, None) }
}

fn pwrite(fd: c_int, data: &[u8], offset: i64) -> Result<u64> {
    // SAFETY: `data` is readable.
    unsafe { write_from(fd, data.as_ptr(), data.len() as u64, Some(offset)) }
}

fn read_all(guest: &[u8]) -> Vec<u8> {
    let fd = open(guest, 0, 0).unwrap();
    let data = read(fd, 64).unwrap();
    close(fd).unwrap();
    data
}

/// Every record of a `getdirentries` buffer: (fileno, reclen, type, name).
fn records(buffer: &[u8]) -> Vec<(u32, u16, u8, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < buffer.len() {
        let fileno = u32::from_ne_bytes(buffer[at..at + 4].try_into().unwrap());
        let reclen = u16::from_ne_bytes(buffer[at + 4..at + 6].try_into().unwrap());
        let namlen = buffer[at + 7] as usize;
        assert_eq!(reclen as usize % 8, 0);
        assert_eq!(buffer[at + 8 + namlen], 0, "NUL after the name");
        out.push((
            fileno,
            reclen,
            buffer[at + 6],
            buffer[at + 8..at + 8 + namlen].to_vec(),
        ));
        at += reclen as usize;
    }
    out
}

fn list(fd: c_int) -> Vec<(u32, u16, u8, Vec<u8>)> {
    let mut buffer = vec![0u8; 4096];
    let (n, _) = getdents(fd, &mut buffer).unwrap();
    records(&buffer[..n])
}

#[test]
fn the_c_test_mod_overlay_is_read_only_and_saves_are_writable() {
    let scratch = Scratch::new("c-test");
    let modded = scratch.root.join("mod.dcx");
    fs::write(&modded, "modded").unwrap();
    link(&modded, &scratch.game("asset.dcx"));

    for guest in [&b"/app0/asset.dcx"[..], b"/hostapp/asset.dcx", b"asset.dcx"] {
        assert_eq!(read_all(guest), b"modded", "{}", shown(guest));
    }
    assert_eq!(stat(b"/app0/asset.dcx").unwrap().size, 6);

    assert_eq!(open(b"/app0/asset.dcx", 2, 0), Err(Errno::EROFS));
    assert_eq!(open(b"/app0/asset.dcx", O_TRUNC, 0), Err(Errno::EROFS));
    assert_eq!(truncate(b"/app0/asset.dcx", 0), Err(Errno::EROFS));
    assert_eq!(
        path_op(b"/app0/asset.dcx", PathOp::Unlink),
        Err(Errno::EROFS)
    );
    assert_eq!(
        rename(b"/app0/asset.dcx", b"/data/moved"),
        Err(Errno::EROFS)
    );
    assert_eq!(
        fs::read(&modded).unwrap(),
        b"modded",
        "the mod file is untouched"
    );

    let dir = open(b"/app0", O_DIRECTORY, 0).unwrap();
    let entries = list(dir);
    assert!(entries
        .iter()
        .any(|(_, _, kind, name)| name == b"asset.dcx" && *kind == DT_REG));
    close(dir).unwrap();

    let save = open(b"/data/test-save", 0x202, 0o644).unwrap();
    assert_eq!(write(save, b"save"), Ok(4));
    close(save).unwrap();
    assert_eq!(fs::read(scratch.user("data/test-save")).unwrap(), b"save");

    assert_eq!(translate(b"/data/..\\..\\escape"), Err(Errno::EACCES));
    assert_eq!(translate(b"..\\..\\escape"), Err(Errno::EACCES));
    assert_eq!(stat(b"/data/a..b").map(|_| ()), Err(Errno::ENOENT));
}

#[test]
fn configure_creates_and_mounts_the_user_directories() {
    let scratch = Scratch::new("configure");
    for name in ["temp0", "download0", "data"] {
        assert!(scratch.user(name).is_dir(), "{name}");
        // configure joins the user root and the mount name with '/', as the C did.
        let mut mounted = bytes(&scratch.root.join("user")).to_vec();
        mounted.extend_from_slice(format!("/{name}").as_bytes());
        let mut guest = format!("/{name}/x").into_bytes();
        let mut want = mounted.clone();
        want.extend_from_slice(b"/x");
        assert_eq!(translate(&guest), Ok(want));
        guest.truncate(name.len() + 1);
        assert_eq!(translate(&guest), Ok(mounted));
    }
    assert_eq!(user_dir().to_bytes(), bytes(&scratch.root.join("user")));
    // SAFETY: a NUL-terminated string that lives as long as the configuration.
    assert_eq!(
        unsafe { CStr::from_ptr(ffi::runtime_file_user_dir()) },
        user_dir()
    );
}

#[test]
fn translation_rules() {
    let scratch = Scratch::new("translate");
    let game = bytes(&scratch.root.join("game")).to_vec();
    let under = |rest: &str| [&game[..], rest.as_bytes()].concat();

    assert_eq!(translate(b"eboot.bin"), Ok(under("/eboot.bin")));
    assert_eq!(translate(b"/app0"), Ok(game.clone()));
    assert_eq!(translate(b"/hostapp/a/b"), Ok(under("/a/b")));
    assert_eq!(translate(b""), Err(Errno::ENOENT));
    // A mount matches whole components only.
    assert_eq!(translate(b"/app0x/file"), Err(Errno::ENOENT));
    assert_eq!(translate(b"/nowhere"), Err(Errno::ENOENT));
    // ".." as a component, with either separator, at the start or the end.
    for escape in [
        &b"/app0/../x"[..],
        b"/app0/..",
        b"../x",
        b"..",
        b"/data/a/..\\b",
        b"/data\\..",
    ] {
        assert_eq!(translate(escape), Err(Errno::EACCES), "{}", shown(escape));
    }
    for allowed in [
        &b"/app0/a..b"[..],
        b"/app0/..a",
        b"/app0/a..",
        b"/app0/.../x",
    ] {
        assert!(translate(allowed).is_ok(), "{}", shown(allowed));
    }

    // The longest mount wins.
    let deeper = scratch.root.join("deeper");
    fs::create_dir_all(&deeper).unwrap();
    assert!(mount(b"/app0/sub", bytes(&deeper)));
    assert_eq!(
        translate(b"/app0/sub/f"),
        Ok([bytes(&deeper), b"/f"].concat())
    );
    assert_eq!(translate(b"/app0/subway"), Ok(under("/subway")));
    // Mounting the same name again replaces its host root; unmounting removes it.
    assert!(mount(b"/app0/sub", b"/elsewhere"));
    assert_eq!(translate(b"/app0/sub/f"), Ok(b"/elsewhere/f".to_vec()));
    unmount(b"/app0/sub");
    assert_eq!(translate(b"/app0/sub/f"), Ok(under("/sub/f")));
    unmount(b"/never-mounted");

    // The C caller's buffer: the path and its NUL must fit.
    let path = translate(b"/app0/abc").unwrap();
    assert_eq!(
        translate_within(b"/app0/abc", path.len() + 1),
        Ok(path.clone())
    );
    assert_eq!(
        translate_within(b"/app0/abc", path.len()),
        Err(Errno::ENAMETOOLONG)
    );
    let long = [b"/app0/".to_vec(), vec![b'a'; 2000]].concat();
    assert_eq!(translate(&long), Err(Errno::ENAMETOOLONG));

    let mut out = [0 as c_char; 1024];
    // SAFETY: NUL-terminated input, a 1024-byte buffer.
    let result =
        unsafe { ffi::runtime_file_translate(c"/app0/abc".as_ptr(), out.as_mut_ptr(), out.len()) };
    assert_eq!(result, 0);
    // SAFETY: written NUL-terminated just above.
    assert_eq!(
        unsafe { CStr::from_ptr(out.as_ptr()) }.to_bytes(),
        &path[..]
    );
    // SAFETY: as above, with a buffer too small for the path.
    let result = unsafe { ffi::runtime_file_translate(c"/app0/abc".as_ptr(), out.as_mut_ptr(), 4) };
    assert_eq!(result, Errno::ENAMETOOLONG.0);
    // SAFETY: as above.
    let result = unsafe { ffi::runtime_file_translate(c"/x/y".as_ptr(), out.as_mut_ptr(), 1024) };
    assert_eq!(result, Errno::ENOENT.0);
}

#[test]
fn the_mount_table_has_32_entries_and_name_limits() {
    let _scratch = Scratch::new("mount-limits");
    // configure made 5.
    let names: Vec<String> = (0..27).map(|i| format!("/slot{i}")).collect();
    for name in &names {
        assert!(mount(name.as_bytes(), b"/host"), "{name}");
    }
    assert!(!mount(b"/one-too-many", b"/host"));
    // A full table still replaces an existing mount.
    assert!(mount(b"/slot0", b"/other"));
    assert_eq!(translate(b"/slot0/x"), Ok(b"/other/x".to_vec()));
    for name in &names {
        unmount(name.as_bytes());
    }
    assert!(!mount(&[b'/'; 64], b"/host"));
    assert!(!mount(b"/long-host", &[b'h'; 512]));
    assert!(mount(&[b'/'; 63], &[b'h'; 511]));
    unmount(&[b'/'; 63]);
    // SAFETY: NUL-terminated literals.
    unsafe {
        assert_eq!(
            ffi::runtime_file_mount(c"/c-mount".as_ptr(), c"/host".as_ptr()),
            0
        );
        assert_eq!(
            ffi::runtime_file_mount(std::ptr::null(), c"/host".as_ptr()),
            -1
        );
        ffi::runtime_file_unmount(c"/c-mount".as_ptr());
    }
    assert_eq!(translate(b"/c-mount/x"), Err(Errno::ENOENT));
}

/// Linux only: macOS and Windows file systems ignore case themselves.
#[cfg(target_os = "linux")]
#[test]
fn missing_components_take_the_spelling_of_a_case_insensitive_match() {
    let scratch = Scratch::new("case");
    fs::create_dir_all(scratch.game("cubemap/train-wreck-a")).unwrap();
    fs::write(
        scratch.game("cubemap/train-wreck-a/train_wreck_a_07.dds"),
        "dds",
    )
    .unwrap();
    let real = [
        bytes(&scratch.root.join("game")),
        b"/cubemap/train-wreck-a/train_wreck_a_07.dds",
    ]
    .concat();
    assert_eq!(
        translate(b"/app0/Cubemap/Train-Wreck-A/Train_Wreck_a_07.dds"),
        Ok(real.clone())
    );
    assert_eq!(
        read_all(b"/app0/cubemap/train-wreck-a/Train_Wreck_a_07.dds"),
        b"dds"
    );
    // Only below the mount, only when missing, never a different length.
    assert_eq!(
        translate(b"/app0/CUBEMAP/missing/x.dds"),
        Ok([bytes(&scratch.root.join("game")), b"/cubemap/missing/x.dds"].concat())
    );
    assert_eq!(
        translate(b"/app0/cubemap/train-wreck-a/train_wreck_a_7.dds"),
        Ok([
            bytes(&scratch.root.join("game")),
            b"/cubemap/train-wreck-a/train_wreck_a_7.dds"
        ]
        .concat())
    );
    // An exact match wins over a case-folded one.
    fs::write(scratch.game("cubemap/Exact.txt"), "upper").unwrap();
    fs::write(scratch.game("cubemap/exact.txt"), "lower").unwrap();
    assert_eq!(read_all(b"/app0/cubemap/Exact.txt"), b"upper");
    assert_eq!(read_all(b"/app0/cubemap/exact.txt"), b"lower");
    assert_eq!(open(b"/app0/cubemap/nothing.txt", 0, 0), Err(Errno::ENOENT));
}

#[test]
fn the_mod_overlay_wins_by_its_links_and_by_deeper_mounts() {
    let scratch = Scratch::new("overlay");
    // The dump's file and a mod's replacement: the overlay tree links the mod's file.
    fs::create_dir_all(scratch.game("data")).unwrap();
    fs::write(scratch.game("data/kept.bin"), "dump").unwrap();
    let mods = scratch.root.join("mods/a");
    fs::create_dir_all(&mods).unwrap();
    fs::write(mods.join("swapped.bin"), "mod a").unwrap();
    link(&mods.join("swapped.bin"), &scratch.game("data/swapped.bin"));
    assert_eq!(read_all(b"/app0/data/kept.bin"), b"dump");
    assert_eq!(read_all(b"/app0/data/swapped.bin"), b"mod a");
    assert_eq!(read_all(b"data/swapped.bin"), b"mod a");
    assert_eq!(stat(b"/app0/data/swapped.bin").unwrap().size, 5);
    let fd = open(b"/app0/data/swapped.bin", 0, 0).unwrap();
    assert_eq!(fstat(fd).unwrap().size, 5);
    close(fd).unwrap();
    // A mount below /app0 (another layer) wins over /app0 for its paths only.
    let layer = scratch.root.join("layer");
    fs::create_dir_all(&layer).unwrap();
    fs::write(layer.join("kept.bin"), "layer").unwrap();
    assert!(mount(b"/app0/data", bytes(&layer)));
    assert_eq!(read_all(b"/app0/data/kept.bin"), b"layer");
    assert_eq!(read_all(b"data/kept.bin"), b"layer");
    unmount(b"/app0/data");
    assert_eq!(read_all(b"/app0/data/kept.bin"), b"dump");
    // The game mounts stay read-only through a link.
    assert_eq!(open(b"/app0/data/swapped.bin", 1, 0), Err(Errno::EROFS));
    assert_eq!(fs::read(mods.join("swapped.bin")).unwrap(), b"mod a");
}

#[test]
fn positioned_calls_never_move_the_position_sequential_ones_share_it() {
    let scratch = Scratch::new("positions");
    fs::write(scratch.user("data/file"), "0123456789").unwrap();
    let fd = open(b"/data/file", 2, 0).unwrap();
    assert_eq!(read(fd, 3), Ok(b"012".to_vec()));
    assert_eq!(pread(fd, 2, 7), Ok(b"78".to_vec()));
    assert_eq!(read(fd, 2), Ok(b"34".to_vec()));
    // A negative pread offset reads at the position (and moves it), as the C file did.
    assert_eq!(pread(fd, 1, -1), Ok(b"5".to_vec()));
    assert_eq!(lseek(fd, 0, 1), Ok(6));
    assert_eq!(pwrite(fd, b"AB", 0), Ok(2));
    assert_eq!(lseek(fd, 0, 1), Ok(6));
    assert_eq!(write(fd, b"xy"), Ok(2));
    assert_eq!(lseek(fd, 0, 1), Ok(8));
    assert_eq!(pwrite(fd, b"z", -1), Err(Errno::EINVAL));
    assert_eq!(lseek(fd, -2, 2), Ok(8));
    assert_eq!(read(fd, 10), Ok(b"89".to_vec()));
    assert_eq!(read(fd, 10), Ok(Vec::new()), "end of file");
    assert_eq!(pread(fd, 4, 100), Ok(Vec::new()));
    assert_eq!(lseek(fd, -1, 0), Err(Errno::EINVAL));
    assert_eq!(lseek(fd, 0, 3), Err(Errno::EINVAL));
    assert_eq!(lseek(fd, 3, 0), Ok(3));
    assert_eq!(read(fd, 0), Ok(Vec::new()));
    assert_eq!(fstat(fd).unwrap().size, 10);
    close(fd).unwrap();
    assert_eq!(fs::read(scratch.user("data/file")).unwrap(), b"AB2345xy89");

    // O_APPEND writes at the end whatever the position, positioned ones too (Linux and
    // FreeBSD both do), and a positioned one leaves the position.
    let fd = open(b"/data/file", 1 | O_APPEND, 0).unwrap();
    assert_eq!(write(fd, b"++"), Ok(2));
    assert_eq!(lseek(fd, 0, 1), Ok(12));
    assert_eq!(pwrite(fd, b"@", 0), Ok(1));
    assert_eq!(lseek(fd, 0, 1), Ok(12));
    assert_eq!(lseek(fd, 0, 0), Ok(0));
    assert_eq!(write(fd, b"!"), Ok(1));
    close(fd).unwrap();
    assert_eq!(
        fs::read(scratch.user("data/file")).unwrap(),
        b"AB2345xy89++@!"
    );

    // Read-only descriptors refuse writes, write-only ones reads (Windows: access denied).
    let fd = open(b"/data/file", 0, 0).unwrap();
    let refused = write(fd, b"no").unwrap_err();
    close(fd).unwrap();
    let fd = open(b"/data/file", 1, 0).unwrap();
    let refused_read = read(fd, 1).unwrap_err();
    close(fd).unwrap();
    #[cfg(unix)]
    assert_eq!((refused, refused_read), (Errno::EBADF, Errno::EBADF));
    #[cfg(not(unix))]
    let _ = (refused, refused_read);
    // A null buffer.
    let fd = open(b"/data/file", 0, 0).unwrap();
    // SAFETY: a null buffer is refused before it is used.
    assert_eq!(
        unsafe { read_into(fd, std::ptr::null_mut(), 4, -1) },
        Err(Errno::EFAULT)
    );
    close(fd).unwrap();
}

#[test]
fn open_flags_create_truncate_and_directories() {
    let scratch = Scratch::new("flags");
    // O_CREAT|O_EXCL, then again.
    let fd = open(b"/data/new", 1 | O_CREAT | O_EXCL, 0o600).unwrap();
    close(fd).unwrap();
    assert!(scratch.user("data/new").is_file());
    assert_eq!(
        open(b"/data/new", 1 | O_CREAT | O_EXCL, 0),
        Err(Errno::EEXIST)
    );
    // Read-only with O_CREAT creates; with O_TRUNC truncates.
    let fd = open(b"/data/read-create", O_CREAT, 0).unwrap();
    assert_eq!(read(fd, 4), Ok(Vec::new()));
    close(fd).unwrap();
    assert!(scratch.user("data/read-create").is_file());
    fs::write(scratch.user("data/read-create"), "abc").unwrap();
    let fd = open(b"/data/read-create", O_CREAT, 0).unwrap();
    assert_eq!(
        read(fd, 4),
        Ok(b"abc".to_vec()),
        "O_CREAT keeps an existing file"
    );
    close(fd).unwrap();
    let fd = open(b"/data/read-create", O_TRUNC, 0).unwrap();
    assert_eq!(read(fd, 4), Ok(Vec::new()));
    close(fd).unwrap();
    // O_SYNC writes go through.
    let fd = open(b"/data/sync", 1 | O_CREAT | O_SYNC, 0).unwrap();
    assert_eq!(write(fd, b"s"), Ok(1));
    assert_eq!(fsync(fd), Ok(()));
    close(fd).unwrap();

    // O_CREAT on the game mount opens an existing file read-only, never creates one.
    fs::write(scratch.game("config.ini"), "cfg").unwrap();
    let fd = open(b"/app0/config.ini", O_CREAT, 0).unwrap();
    assert_eq!(read(fd, 8), Ok(b"cfg".to_vec()));
    close(fd).unwrap();
    assert_eq!(open(b"/app0/absent.ini", O_CREAT, 0), Err(Errno::EROFS));
    assert!(!scratch.game("absent.ini").exists());
    for flags in [1, 2, 3, O_APPEND, O_TRUNC, O_EXCL] {
        assert_eq!(
            open(b"/app0/config.ini", flags, 0),
            Err(Errno::EROFS),
            "{flags:#x}"
        );
    }

    // Directories.
    assert_eq!(open(b"/data", 2, 0), Err(Errno::EISDIR));
    assert_eq!(open(b"/data", O_CREAT, 0), Err(Errno::EISDIR));
    assert_eq!(open(b"/data", 1 | O_CREAT | O_EXCL, 0), Err(Errno::EEXIST));
    assert_eq!(open(b"/data/new", O_DIRECTORY, 0), Err(Errno::ENOTDIR));
    assert_eq!(open(b"/data/absent", O_DIRECTORY, 0), Err(Errno::ENOENT));
    let missing = MISSING.load(Ordering::Relaxed);
    assert_eq!(open(b"/data/absent", 0, 0), Err(Errno::ENOENT));
    assert_eq!(MISSING.load(Ordering::Relaxed), missing + 1);
    let dir = open(b"/data", 0, 0).unwrap();
    assert_eq!(read(dir, 4), Err(Errno::EISDIR));
    assert_eq!(write(dir, b"x"), Err(Errno::EBADF));
    assert_eq!(fstat(dir).unwrap().mode & 0o170000, 0o040000);
    assert_eq!(fsync(dir), Ok(()));
    assert_eq!(ftruncate(dir, 0), Err(Errno::EINVAL));
    close(dir).unwrap();
}

#[test]
fn listings_page_through_the_entries() {
    let scratch = Scratch::new("listing");
    fs::create_dir_all(scratch.game("dir/sub")).unwrap();
    let names: Vec<String> = (0..40)
        .map(|i| format!("file-{i:02}-{}", "x".repeat(i)))
        .collect();
    for name in &names {
        fs::write(scratch.game("dir").join(name), "").unwrap();
    }
    let fd = open(b"/app0/dir", O_DIRECTORY, 0).unwrap();
    let mut seen = Vec::new();
    let mut buffer = vec![0u8; 512];
    let mut calls = 0;
    loop {
        let (n, base) = getdents(fd, &mut buffer).unwrap();
        assert_eq!(
            base,
            seen.len() as i64,
            "basep is the position before the call"
        );
        if n == 0 {
            break;
        }
        calls += 1;
        seen.extend(records(&buffer[..n]));
    }
    assert!(calls > 1, "40 long names do not fit in 512 bytes");
    assert_eq!(seen.len(), 2 + 1 + names.len());
    for (i, (fileno, reclen, _, name)) in seen.iter().enumerate() {
        assert_eq!(*fileno, i as u32 + 1);
        assert_eq!(*reclen as usize, (8 + name.len() + 1 + 7) & !7);
    }
    assert_eq!((seen[0].2, &seen[0].3[..]), (DT_DIR, &b"."[..]));
    assert_eq!((seen[1].2, &seen[1].3[..]), (DT_DIR, &b".."[..]));
    assert!(seen.iter().any(|e| e.3 == b"sub" && e.2 == DT_DIR));
    for name in &names {
        assert!(
            seen.iter().any(|e| e.3 == name.as_bytes() && e.2 == DT_REG),
            "{name}"
        );
    }

    // lseek sets the entry index, absolutely only.
    assert_eq!(lseek(fd, 2, 0), Ok(2));
    let again = list(fd);
    assert_eq!(again[0].0, 3);
    assert_eq!(again.len(), seen.len() - 2);
    assert_eq!(lseek(fd, 0, 1), Err(Errno::EINVAL));
    assert_eq!(lseek(fd, -1, 0), Err(Errno::EINVAL));
    assert_eq!(lseek(fd, 1000, 0), Ok(1000));
    assert_eq!(getdents(fd, &mut buffer), Ok((0, 1000)));

    // The buffer minimum, and the raw entry point.
    assert_eq!(getdents(fd, &mut [0u8; 511]), Err(Errno::EINVAL));
    lseek(fd, 0, 0).unwrap();
    let mut base = -1i64;
    // SAFETY: 512 writable bytes and a writable base.
    let n = unsafe { getdents_raw(fd, buffer.as_mut_ptr(), 512, &mut base) }.unwrap();
    assert!(n > 0);
    assert_eq!(base, 0);
    // SAFETY: a null buffer is refused before it is used.
    assert_eq!(
        unsafe { getdents_raw(fd, std::ptr::null_mut(), 512, std::ptr::null_mut()) },
        Err(Errno::EFAULT)
    );
    close(fd).unwrap();

    // getdirentries wants a directory.
    fs::write(scratch.game("plain"), "").unwrap();
    let file = open(b"/app0/plain", 0, 0).unwrap();
    assert_eq!(getdents(file, &mut buffer), Err(Errno::EINVAL));
    close(file).unwrap();
    assert_eq!(getdents(file, &mut buffer), Err(Errno::EBADF));
}

#[test]
fn descriptors_run_out_at_1024_and_are_reused_lowest_first() {
    let _scratch = Scratch::new("emfile");
    let mut fds = Vec::new();
    let error = loop {
        match open(b"/app0", 0, 0) {
            Ok(fd) => fds.push(fd),
            Err(error) => break error,
        }
        assert!(fds.len() <= MAX_FILES, "the table never fills");
    };
    assert_eq!(error, Errno::EMFILE);
    assert_eq!(*fds.last().unwrap(), MAX_FILES as c_int - 1);
    assert!(fds.iter().all(|&fd| fd >= 3));
    close(fds[10]).unwrap();
    close(fds[5]).unwrap();
    assert_eq!(open(b"/app0", 0, 0), Ok(fds[5]));
    for fd in fds {
        let _ = close(fd);
    }
    assert_eq!(open(b"/app0", 0, 0).map(|fd| fd >= 3), Ok(true));
}

#[test]
fn bad_descriptors_and_stdio() {
    let _scratch = Scratch::new("descriptors");
    assert_eq!(close(0), Ok(()));
    assert_eq!(close(2), Ok(()));
    assert_eq!(close(-1), Err(Errno::EBADF));
    assert_eq!(close(1000), Err(Errno::EBADF));
    assert_eq!(close(5000), Err(Errno::EBADF));
    assert_eq!(read(-1, 4), Err(Errno::EBADF));
    assert_eq!(read(999, 4), Err(Errno::EBADF));
    assert_eq!(write(999, b"x"), Err(Errno::EBADF));
    assert_eq!(lseek(1, 0, 0), Err(Errno::EBADF));
    assert_eq!(lseek(999, 0, 7), Err(Errno::EBADF));
    assert_eq!(fstat(999).map(|_| ()), Err(Errno::EBADF));
    assert_eq!(getdents(0, &mut [0u8; 512]), Err(Errno::EBADF));
    assert_eq!(ftruncate(999, 0), Err(Errno::EBADF));
    assert_eq!(fsync(999), Err(Errno::EBADF));
    // A zero-length write to stdout succeeds without output.
    assert_eq!(write(1, b""), Ok(0));
}

#[test]
fn path_operations_off_the_game_mounts() {
    let scratch = Scratch::new("path-ops");
    assert_eq!(path_op(b"/data/dir", PathOp::Mkdir(0)), Ok(()));
    assert!(scratch.user("data/dir").is_dir());
    assert_eq!(path_op(b"/data/dir", PathOp::Mkdir(0)), Err(Errno::EEXIST));
    assert_eq!(path_op(b"/app0/dir", PathOp::Mkdir(0)), Err(Errno::EROFS));
    fs::write(scratch.user("data/dir/f"), "1234").unwrap();
    assert_eq!(path_op(b"/data/dir", PathOp::Rmdir), Err(Errno::ENOTEMPTY));
    assert_eq!(truncate(b"/data/dir/f", 2), Ok(()));
    assert_eq!(fs::read(scratch.user("data/dir/f")).unwrap(), b"12");
    assert_eq!(truncate(b"/data/dir/f", -1), Err(Errno::EINVAL));
    let fd = open(b"/data/dir/f", 2, 0).unwrap();
    assert_eq!(ftruncate(fd, 1), Ok(()));
    assert_eq!(fstat(fd).unwrap().size, 1);
    close(fd).unwrap();
    fs::write(scratch.user("data/other"), "other").unwrap();
    // rename replaces the target, on Windows too.
    assert_eq!(rename(b"/data/dir/f", b"/data/other"), Ok(()));
    assert_eq!(fs::read(scratch.user("data/other")).unwrap(), b"1");
    assert_eq!(rename(b"/data/absent", b"/data/x"), Err(Errno::ENOENT));
    assert_eq!(rename(b"/data/other", b"/app0/x"), Err(Errno::EROFS));
    assert_eq!(access(b"/data/other", 0), Ok(()));
    assert_eq!(access(b"/data/other", 6), Ok(()));
    assert_eq!(access(b"/data/absent", 0), Err(Errno::ENOENT));
    assert_eq!(path_op(b"/data/other", PathOp::Unlink), Ok(()));
    assert_eq!(path_op(b"/data/other", PathOp::Unlink), Err(Errno::ENOENT));
    assert_eq!(path_op(b"/data/dir", PathOp::Rmdir), Ok(()));
    assert!(!scratch.user("data/dir").exists());
    assert_eq!(path_op(b"/app0", PathOp::Rmdir), Err(Errno::EROFS));
    assert_eq!(path_op(b"relative", PathOp::Unlink), Err(Errno::EROFS));
}

#[test]
fn the_guest_stat_is_freebsd_s_layout() {
    assert_eq!(std::mem::size_of::<GuestStat>(), 120);
    assert_eq!(std::mem::offset_of!(GuestStat, mode), 8);
    assert_eq!(std::mem::offset_of!(GuestStat, nlink), 10);
    assert_eq!(std::mem::offset_of!(GuestStat, atime), 24);
    assert_eq!(std::mem::offset_of!(GuestStat, mtime), 40);
    assert_eq!(std::mem::offset_of!(GuestStat, ctime), 56);
    assert_eq!(std::mem::offset_of!(GuestStat, size), 72);
    assert_eq!(std::mem::offset_of!(GuestStat, blocks), 80);
    assert_eq!(std::mem::offset_of!(GuestStat, blksize), 88);
    assert_eq!(std::mem::offset_of!(GuestStat, birthtime), 104);

    let scratch = Scratch::new("stat");
    fs::write(scratch.user("data/f"), vec![7u8; 1000]).unwrap();
    let file = stat(b"/data/f").unwrap();
    assert_eq!(file.size, 1000);
    assert_eq!(file.mode & 0o170000, 0o100000);
    assert!(file.nlink >= 1);
    assert!(file.blocks >= 2);
    assert!(file.blksize > 0);
    assert!(file.mtime.sec > 1_600_000_000);
    assert_eq!(file.birthtime, file.ctime);
    assert_eq!(
        (file.uid, file.gid, file.rdev, file.flags, file.gen),
        (0, 0, 0, 0, 0)
    );
    #[cfg(windows)]
    assert_eq!((file.mode, file.blocks, file.blksize), (0o100644, 2, 4096));
    let dir = stat(b"/data").unwrap();
    assert_eq!(dir.mode & 0o170000, 0o040000);
    assert_eq!(stat(b"/data/absent").map(|_| ()), Err(Errno::ENOENT));

    // The C entry points: translation first, then the buffer, then the host.
    let mut raw = [0xaau8; 120];
    // SAFETY: NUL-terminated paths, a 120-byte buffer.
    unsafe {
        assert_eq!(
            ffi::runtime_file_stat(c"/data/f".as_ptr(), raw.as_mut_ptr().cast()),
            0
        );
        assert_eq!(
            ffi::runtime_file_stat(c"/data/f".as_ptr(), std::ptr::null_mut()),
            -i64::from(Errno::EFAULT.0)
        );
        assert_eq!(
            ffi::runtime_file_stat(c"/none/f".as_ptr(), std::ptr::null_mut()),
            -i64::from(Errno::ENOENT.0)
        );
        assert_eq!(
            ffi::runtime_file_fstat(999, std::ptr::null_mut()),
            -i64::from(Errno::EBADF.0)
        );
    }
    assert_eq!(i64::from_ne_bytes(raw[72..80].try_into().unwrap()), 1000);
    assert_eq!(&raw[12..24], &[0u8; 12], "uid, gid and rdev are zero");
}

#[test]
fn host_errors_map_to_freebsd_errno() {
    use io::ErrorKind as K;
    let kind = |kind: K| Errno::from_io(&io::Error::from(kind));
    assert_eq!(kind(K::NotFound), Errno::ENOENT);
    assert_eq!(kind(K::PermissionDenied), Errno::EACCES);
    assert_eq!(kind(K::AlreadyExists), Errno::EEXIST);
    assert_eq!(kind(K::WouldBlock), Errno::EAGAIN);
    assert_eq!(kind(K::InvalidInput), Errno::EINVAL);
    assert_eq!(kind(K::Other), Errno::EIO);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let raw = |code: i32| Errno::from_io(&io::Error::from_raw_os_error(code));
        assert_eq!(raw(2), Errno::ENOENT);
        assert_eq!(raw(13), Errno::EACCES);
        assert_eq!(raw(30), Errno::EROFS);
        assert_eq!(raw(11), Errno::EAGAIN);
        assert_eq!(raw(35), Errno::EDEADLK);
        assert_eq!(raw(36), Errno::ENAMETOOLONG);
        assert_eq!(raw(38), Errno::ENOSYS);
        assert_eq!(raw(39), Errno::ENOTEMPTY);
        assert_eq!(raw(40), Errno(62));
        assert_eq!(raw(75), Errno(84));
        assert_eq!(raw(110), Errno::ETIMEDOUT);
        assert_eq!(raw(125), Errno(85));
        assert_eq!(raw(15), Errno::EIO);
        assert_eq!(raw(26), Errno::EIO);
        assert_eq!(raw(95), Errno::EIO);
    }
    #[cfg(windows)]
    {
        let raw = |code: i32| Errno::from_io(&io::Error::from_raw_os_error(code));
        assert_eq!(raw(2), Errno::ENOENT);
        assert_eq!(raw(3), Errno::ENOENT);
        assert_eq!(raw(5), Errno::EACCES);
        assert_eq!(raw(32), Errno::EACCES);
        assert_eq!(raw(80), Errno::EEXIST);
        assert_eq!(raw(183), Errno::EEXIST);
        assert_eq!(raw(145), Errno::ENOTEMPTY);
    }
}

#[test]
fn the_report_counts() {
    let scratch = Scratch::new("report");
    fs::write(scratch.user("data/f"), "abcd").unwrap();
    let (opens, reads, bytes, writes) = (
        OPENS.load(Ordering::Relaxed),
        READS.load(Ordering::Relaxed),
        BYTES_READ.load(Ordering::Relaxed),
        WRITES.load(Ordering::Relaxed),
    );
    let fd = open(b"/data/f", 2, 0).unwrap();
    read(fd, 3).unwrap();
    pread(fd, 8, 0).unwrap();
    write(fd, b"z").unwrap();
    pwrite(fd, b"y", 0).unwrap();
    close(fd).unwrap();
    assert_eq!(OPENS.load(Ordering::Relaxed), opens + 1);
    assert_eq!(READS.load(Ordering::Relaxed), reads + 2);
    assert_eq!(BYTES_READ.load(Ordering::Relaxed), bytes + 7);
    assert_eq!(
        WRITES.load(Ordering::Relaxed),
        writes + 1,
        "pwrite is not counted"
    );
    report();
}

/// The guest's entry points, reached through `runtime_file_resolve` with scoped NIDs.
#[cfg(target_arch = "x86_64")]
#[test]
fn guest_calls_resolve_by_nid_and_keep_their_conventions() {
    use std::ffi::c_void;
    use std::mem::transmute;
    type Open = unsafe extern "sysv64" fn(*const c_char, c_int, c_int) -> i64;
    type Read = unsafe extern "sysv64" fn(c_int, *mut c_void, u64) -> i64;
    type Stat = unsafe extern "sysv64" fn(*const c_char, *mut GuestStat) -> i64;
    type Lseek = unsafe extern "sysv64" fn(c_int, i64, c_int) -> i64;
    let get = |nid: &CStr| {
        // SAFETY: a NUL-terminated NID.
        let address = unsafe { ffi::runtime_file_resolve(nid.as_ptr()) };
        assert_ne!(address, 0, "{nid:?}");
        address
    };

    let scratch = Scratch::new("guest");
    fs::write(scratch.game("asset.dcx"), "guest").unwrap();
    // SAFETY: each address is the function of that symbol, with the C signature.
    unsafe {
        let sce_open: Open = transmute(get(c"1G3lF1Gg1k8#p#J"));
        let posix_open: Open = transmute(get(c"wuCroIGjt2g#I#J"));
        let posix_read: Read = transmute(get(c"AqBioC2vF3I#I#J"));
        let posix_stat: Stat = transmute(get(c"E6ao34wPw+U#I#J"));
        let sce_stat: Stat = transmute(get(c"eV9wAD2riIA#p#J"));
        let sce_lseek: Lseek = transmute(get(c"oib76F-12fk#p#J"));
        get(c"taRWhTJFTgE#p#J");
        get(c"+r3rMFwItV4#p#J");

        let fd = sce_open(c"/app0/asset.dcx".as_ptr(), 0, 0);
        assert!(fd >= 3);
        let mut buffer = [0u8; 8];
        assert_eq!(posix_read(fd as c_int, buffer.as_mut_ptr().cast(), 8), 5);
        assert_eq!(&buffer[..5], b"guest");
        assert_eq!(sce_lseek(fd as c_int, 1, 0), 1);
        assert_eq!(
            sce_lseek(fd as c_int, 0, 9),
            i64::from(0x8002_0016u32 as i32)
        );
        close(fd as c_int).unwrap();

        assert_eq!(
            sce_open(c"/app0/absent".as_ptr(), 0, 0),
            i64::from(0x8002_0002u32 as i32)
        );
        assert_eq!(
            sce_open(c"/app0/asset.dcx".as_ptr(), 2, 0),
            i64::from(0x8002_001eu32 as i32)
        );
        assert_eq!(posix_open(c"/app0/asset.dcx".as_ptr(), 1, 0), -1);
        #[cfg(not(uncharted_loader))]
        assert_eq!(guest::errno(), Errno::EROFS.0);
        assert_eq!(
            sce_open(std::ptr::null(), 0, 0),
            i64::from(0x8002_000eu32 as i32)
        );

        let mut out = GuestStat::default();
        assert_eq!(posix_stat(c"asset.dcx".as_ptr(), &mut out), 0);
        assert_eq!(out.size, 5);
        assert_eq!(
            sce_stat(c"/app0/absent".as_ptr(), &mut out),
            i64::from(0x8002_0002u32 as i32)
        );
    }
    // An import this module does not export, and a NID nothing knows.
    // SAFETY: NUL-terminated strings, or null.
    unsafe {
        assert_eq!(ffi::runtime_file_resolve(c"ejekcaNQNq0#p#J".as_ptr()), 0);
        assert_eq!(ffi::runtime_file_resolve(c"unknown".as_ptr()), 0);
        assert_eq!(ffi::runtime_file_resolve(std::ptr::null()), 0);
    }
}
