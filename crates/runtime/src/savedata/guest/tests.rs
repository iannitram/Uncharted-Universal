//! The guest functions with the guest's structures as the deleted C declared them, on the
//! process's store (under `test_lock`, in a temporary user directory). The store's own rules are
//! tested in `store/tests.rs`; here: the NIDs, the structure reads and writes, null pointers.

use super::super::store::{ERR_NOT_FOUND, ERR_NOT_MOUNTED, MODE_CREATE, PARAM_SIZE};
use super::super::{ffi, set_user_dir, test_lock};
use super::*;
use std::fs;
use std::path::PathBuf;
use std::ptr::{null, null_mut};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("uu-guest-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        set_user_dir(&path.to_string_lossy());
        // SAFETY: a NUL-terminated literal.
        unsafe { ffi::runtime_savedata_configure(c"GUESTTEST".as_ptr()) };
        Self(path)
    }

    fn root(&self) -> PathBuf {
        self.0.join("savedata/1/GUESTTEST")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        set_user_dir("");
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> DirName {
    let mut field = [0u8; DIR_NAME];
    field[..name.len()].copy_from_slice(name.as_bytes());
    field
}

fn mount_request(name: &DirName, mode: u32) -> Mount1 {
    Mount1 {
        user: 1,
        pad: 0,
        title: null(),
        dir: name,
        fingerprint: null(),
        blocks: 0,
        mode,
        reserved: [0; 32],
    }
}

fn address(f: usize, nid: &CStr) {
    assert_eq!(resolve(nid), Some(f), "{nid:?}");
    // SAFETY: a NUL-terminated literal.
    assert_eq!(unsafe { ffi::runtime_savedata_resolve(nid.as_ptr()) }, f);
}

#[test]
fn guest_resolve_finds_each_function_by_its_nid() {
    address(initialize as *const () as usize, c"ZkZhskCPXFw#O#P");
    address(terminate as *const () as usize, c"yKDy8S5yLA0#O#P");
    address(mount as *const () as usize, c"32HQAQdwM2o#O#P");
    address(umount as *const () as usize, c"BMR4F-Uek3E#O#P");
    address(set_param as *const () as usize, c"85zul--eGXs#O#P");
    address(save_icon as *const () as usize, c"c88Yy54Mx0w#O#P");
    address(delete as *const () as usize, c"S1GkePI17zQ#O#P");
    address(search as *const () as usize, c"dyIhnXq-0SM#O#P");
    address(memory_setup as *const () as usize, c"v7AAAMo0Lz4#O#P");
    address(memory_get as *const () as usize, c"7Bt5pBC-Aco#O#P");
    address(memory_set as *const () as usize, c"h3YURzXGSVQ#O#P");
    for nid in [
        c"s9e3+YpRnzw#d#e",
        c"4tPhsP6FpDI#d#e",
        c"KK3Bdg1RWK0#d#e",
        c"YuH2FA7azqQ#d#e",
        c"ERKzksauAJA#d#e",
        c"en7gNVnh878#d#e",
        c"fH46Lag88XY#d#e",
        c"hay1CfTmLyA#d#e",
        c"yEiJ-qqr6Cg#d#e",
    ] {
        assert!(resolve(nid).is_some(), "{nid:?}");
    }
    // UpdateStatus and GetStatus are one function.
    assert_eq!(resolve(c"KK3Bdg1RWK0#d#e"), resolve(c"ERKzksauAJA#d#e"));
    // Another library's NID, an unknown one, a bare symbol name and null.
    assert_eq!(resolve(c"8G2LB+A3rzg#q#q"), None);
    assert_eq!(resolve(c"32HQAQdwM2o#O#X"), None);
    assert_eq!(resolve(c"sceSaveDataMount"), None);
    // SAFETY: null is allowed.
    assert_eq!(unsafe { ffi::runtime_savedata_resolve(null()) }, 0);
}

#[test]
fn guest_c_bytes_stops_at_the_nul_or_the_limit() {
    let text = b"abc\0def";
    // SAFETY: `text` is readable up to its NUL.
    unsafe {
        assert_eq!(c_bytes(text.as_ptr(), 16).as_deref(), Some(&b"abc"[..]));
        assert_eq!(c_bytes(text.as_ptr(), 2).as_deref(), Some(&b"ab"[..]));
        assert_eq!(c_bytes(text.as_ptr(), 0).as_deref(), Some(&b""[..]));
        assert_eq!(c_bytes(null(), 16), None);
    }
}

#[test]
fn guest_a_save_mounts_writes_lists_and_unmounts() {
    let _lock = test_lock();
    let d = TempDir::new("save");
    terminate();
    assert_eq!(terminate(), ERR_NOT_INITIALIZED);
    let mut result = MountResult::ZERO;
    let name = dir("SAVEDATA01P");
    let request = mount_request(&name, MODE_CREATE);
    assert_eq!(mount(&request, &mut result), ERR_NOT_INITIALIZED);
    assert_eq!(search(null(), null_mut()), ERR_NOT_INITIALIZED);
    assert_eq!(initialize(null()), 0);
    assert_eq!(mount(null(), &mut result), ERR_PARAMETER);
    assert_eq!(mount(&request, null_mut()), ERR_PARAMETER);
    assert_eq!(mount(&request, &mut result), 0);
    assert_eq!(
        (c_text(&result.point), result.status),
        (&b"/savedata0"[..], 1)
    );
    let point: MountPoint = result.point;

    // SetParam: 1 to 3 are text up to the NUL or the size, 4 the user param, 0 a whole param.
    let chapter = b"Made-up chapter, cut here";
    let detail = b"Made-up detail\0after";
    let user = 0xBEEF_u32.to_ne_bytes();
    assert_eq!(set_param(&point, 1, chapter.as_ptr().cast(), 15), 0);
    assert_eq!(set_param(&point, 3, detail.as_ptr().cast(), u64::MAX), 0);
    assert_eq!(set_param(&point, 4, user.as_ptr().cast(), 4), 0);
    assert_eq!(set_param(&point, 4, user.as_ptr().cast(), 3), ERR_PARAMETER);
    assert_eq!(set_param(&point, 0, user.as_ptr().cast(), 4), ERR_PARAMETER);
    assert_eq!(set_param(&point, 9, user.as_ptr().cast(), 4), ERR_PARAMETER);
    assert_eq!(set_param(&point, 1, null(), 4), ERR_PARAMETER);
    assert_eq!(
        set_param(null(), 1, chapter.as_ptr().cast(), 4),
        ERR_NOT_MOUNTED
    );
    let meta = d.root().join("SAVEDATA01P.sce_sys");
    let read = || {
        let bytes: [u8; PARAM_SIZE] = fs::read(meta.join("param.bin"))
            .unwrap()
            .try_into()
            .unwrap();
        Param::from_bytes(&bytes)
    };
    let p = read();
    assert_eq!(c_text(&p.title), b"Made-up chapter");
    assert_eq!(c_text(&p.detail), b"Made-up detail");
    assert_eq!(p.user_param, 0xBEEF);
    let mut whole = Param::ZERO;
    whole.subtitle[..7].copy_from_slice(b"Made-up");
    whole.user_param = 5;
    let raw = whole.to_bytes();
    // An unaligned buffer, as a guest may pass.
    let mut shifted = vec![0u8; PARAM_SIZE + 1];
    shifted[1..].copy_from_slice(&raw);
    let size = PARAM_SIZE as u64;
    assert_eq!(set_param(&point, 0, shifted[1..].as_ptr().cast(), size), 0);
    assert_eq!(
        (c_text(&read().subtitle), read().user_param),
        (&b"Made-up"[..], 5)
    );

    // SaveIcon: `data_size` bytes of the buffer.
    let png = b"made-up png, cut";
    let icon = Icon {
        buffer: png.as_ptr().cast(),
        buffer_size: png.len() as u64,
        data_size: 12,
        reserved: [0; 32],
    };
    assert_eq!(save_icon(&point, &icon), 0);
    assert_eq!(fs::read(meta.join("icon0.png")).unwrap(), b"made-up png,");
    assert_eq!(save_icon(&point, null()), ERR_PARAMETER);
    let empty = Icon {
        buffer: null(),
        ..icon
    };
    assert_eq!(save_icon(&point, &empty), ERR_PARAMETER);

    // A second directory through Mount2, then both unmounted: the save is committed.
    let other = dir("SAVEDATA01Q");
    let request2 = Mount2 {
        user: 1,
        pad: 0,
        dir: &other,
        blocks: 0,
        mode: MODE_CREATE,
        reserved: [0; 32],
        pad2: 0,
    };
    let mut result2 = MountResult::ZERO;
    assert_eq!(mount2(&request2, &mut result2), 0);
    assert_eq!(c_text(&result2.point), b"/savedata1");
    assert_eq!(ffi::runtime_savedata_wait_idle(0), -1);
    assert_eq!(umount(&result2.point), 0);
    assert_eq!(umount(&point), 0);
    assert_eq!(umount(&point), ERR_NOT_FOUND);
    assert_eq!(umount(null()), ERR_NOT_FOUND);
    assert_eq!(ffi::runtime_savedata_wait_idle(0), 0);
    let published: serde_json::Value =
        serde_json::from_slice(&fs::read(d.0.join("Uncharted 1/where.json")).unwrap()).unwrap();
    assert_eq!(published["game"], "u1");
    assert_eq!(published["subtitle"], "Made-up");

    // DirNameSearch: all hits counted, as many names, params and infos set as fit.
    let mut names = [[0xAAu8; DIR_NAME]; 1];
    let mut params = [Param::ZERO; 1];
    let mut infos = [SearchInfo {
        blocks: 0,
        free_blocks: 0,
        reserved: [0; 32],
    }];
    let mut found = SearchResult {
        hits: 0,
        pad: 0,
        names: names.as_mut_ptr(),
        names_capacity: 1,
        set_count: 0,
        params: params.as_mut_ptr(),
        infos: infos.as_mut_ptr(),
        reserved: [0; 12],
        pad2: 0,
    };
    let cond = SearchCond {
        user: 1,
        pad: 0,
        title: null(),
        dir: null(),
        key: 0,
        order: 1,
        reserved: [0; 32],
    };
    assert_eq!(search(&cond, &mut found), 0);
    assert_eq!((found.hits, found.set_count), (2, 1));
    assert_eq!(names[0], dir("SAVEDATA01Q"));
    assert_eq!(params[0].user_param, 0);
    assert_eq!((infos[0].blocks, infos[0].free_blocks), (32768, 16384));
    let pattern = dir("%P");
    let by_name = SearchCond {
        dir: &pattern,
        order: 0,
        ..cond
    };
    found.names = null_mut();
    found.infos = null_mut();
    assert_eq!(search(&by_name, &mut found), 0);
    assert_eq!((found.hits, found.set_count), (1, 1));
    assert_eq!(params[0].user_param, 5);
    assert_eq!(search(&cond, null_mut()), ERR_PARAMETER);
    assert_eq!(search(null(), &mut found), ERR_PARAMETER);
    // Another title's folder, through the request's title id.
    let other_title: TitleId = *b"CUSA99999\0\0\0\0\0\0\0";
    let elsewhere = SearchCond {
        title: &other_title,
        ..cond
    };
    assert_eq!(search(&elsewhere, &mut found), 0);
    assert_eq!(found.hits, 0);

    // Delete: the request's title and directory.
    let wrong = Delete {
        user: 1,
        pad: 0,
        title: &other_title,
        dir: &name,
        unused: 0,
        reserved: [0; 32],
        pad2: 0,
    };
    assert_eq!(delete(&wrong), ERR_NOT_FOUND);
    let right = Delete {
        title: null(),
        ..wrong
    };
    assert_eq!(delete(&right), 0);
    assert!(!d.root().join("SAVEDATA01P").exists());
    assert_eq!(delete(null()), ERR_PARAMETER);
    assert_eq!(terminate(), 0);
}

#[test]
fn guest_save_data_memory_reads_and_writes_the_guest_buffer() {
    let _lock = test_lock();
    let d = TempDir::new("memory");
    assert_eq!(initialize(null()), 0);
    let mut blob = Param::ZERO;
    blob.title[..4].copy_from_slice(b"Blob");
    assert_eq!(memory_setup(1, 32, &blob), 0);
    let memory = d.0.join("savedata/1/GUESTTEST.memory");
    assert_eq!(fs::metadata(memory.join("memory.dat")).unwrap().len(), 32);
    assert!(memory.join("param.bin").is_file());
    let data = *b"made-up";
    assert_eq!(memory_set(1, data.as_ptr().cast(), 7, 4), 0);
    let mut back = [0u8; 7];
    assert_eq!(memory_get(1, back.as_mut_ptr().cast(), 7, 4), 0);
    assert_eq!(&back, b"made-up");
    assert_eq!(memory_get(1, null_mut(), 7, 4), ERR_PARAMETER);
    assert_eq!(memory_set(1, null(), 7, 4), ERR_PARAMETER);
    assert_eq!(memory_set(1, data.as_ptr().cast(), 7, 30), ERR_PARAMETER);
    assert_eq!(memory_setup(1, 0, null()), ERR_PARAMETER);
    assert_eq!(terminate(), 0);
}
