//! The trophy reader, ported from `tests/test_trophies.py`. The packs are synthetic: they are
//! made here with a made-up trophy key, so no console key or game data is needed (SPEC.md M9).
//! The one dump test runs only with `--ignored`, the collection dump and the player's key.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use uncharted_core::games::Game;
use uncharted_core::trophies::{
    self, Aes128, Grade, GradeCounts, Trophy, TrophyKey, TrophySet, TRP_MAGIC,
};

/// A made-up trophy key for the fixtures: not the console's.
const TEST_KEY: [u8; 16] = *b"not the real key";

fn key() -> TrophyKey {
    TrophyKey::from_bytes(TEST_KEY)
}

fn cbc_encrypt(aes: &Aes128, data: &[u8], iv: &[u8; 16]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut previous = *iv;
    for chunk in data.chunks(16) {
        let mut block = [0u8; 16];
        for (i, byte) in block.iter_mut().enumerate() {
            *byte = chunk.get(i).copied().unwrap_or(0) ^ previous[i];
        }
        previous = aes.encrypt_block(&block);
        out.extend_from_slice(&previous);
    }
    out
}

/// An encrypted trophy text entry as a trophy pack holds it: CBC of a zero block and the xml;
/// the first cipher block doubles as the IV of the rest.
fn esfm(npcommid: &str, xml: &str) -> Vec<u8> {
    let aes = key().title_key(npcommid).unwrap();
    let mut body = vec![0u8; 16];
    body.extend_from_slice(xml.as_bytes());
    body.resize(body.len().div_ceil(16) * 16, 0);
    cbc_encrypt(&aes, &body, &[0; 16])
}

/// `entries`: (name, flag, bytes) -> the bytes of a trophy pack.
fn make_trp(entries: &[(&str, u32, Vec<u8>)]) -> Vec<u8> {
    let mut table = Vec::new();
    let mut data = Vec::new();
    let offset = 0x60 + 64 * entries.len() as u64;
    for (name, flag, blob) in entries {
        let mut row = name.as_bytes().to_vec();
        row.resize(32, 0);
        row.extend_from_slice(&(offset + data.len() as u64).to_be_bytes());
        row.extend_from_slice(&(blob.len() as u64).to_be_bytes());
        row.extend_from_slice(&flag.to_be_bytes());
        row.resize(64, 0);
        table.extend(row);
        data.extend_from_slice(blob);
    }
    let mut header = Vec::new();
    header.extend_from_slice(&TRP_MAGIC.to_be_bytes());
    header.extend_from_slice(&3u32.to_be_bytes());
    header.extend_from_slice(&((0x60 + table.len() + data.len()) as u64).to_be_bytes());
    header.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    header.extend_from_slice(&64u32.to_be_bytes());
    header.extend_from_slice(&0u32.to_be_bytes());
    header.resize(0x60, 0);
    header.extend(table);
    header.extend(data);
    header
}

const CONF: &str = "<trophyconf version=\"1.1\"><npcommid>{id}</npcommid><group id=\"001\"/>\
    <trophy id=\"000\" hidden=\"no\" ttype=\"P\" pid=\"-1\"/><trophy id=\"001\" hidden=\"no\" ttype=\"B\" pid=\"000\"/>\
    <trophy id=\"002\" hidden=\"yes\" ttype=\"G\" pid=\"000\"/></trophyconf>";
const TEXTS: &str = "<trophyconf><npcommid>{id}</npcommid><title-name>Uncharted\u{2122} Test</title-name>\
    <title-detail>A game</title-detail><group id=\"001\"><name>G</name></group>\
    <trophy id=\"000\" ttype=\"P\"><name>Platinum</name><detail>All of them</detail></trophy>\
    <trophy id=\"001\" ttype=\"B\"><name>First</name><detail>Do the first thing</detail></trophy>\
    <trophy id=\"002\" hidden=\"yes\" ttype=\"G\"><name>Secret</name><detail>Do\ta\nthing</detail></trophy></trophyconf>";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really a picture";

fn fixture(folder: &Path, npcommid: &str, npbind: bool) {
    let sys_dir = folder.join("sce_sys");
    std::fs::create_dir_all(sys_dir.join("trophy")).unwrap();
    let texts = TEXTS.replace("{id}", npcommid);
    let pack = make_trp(&[
        (
            "TROPCONF.ESFM",
            3,
            esfm(npcommid, &CONF.replace("{id}", npcommid)),
        ),
        ("TROP.ESFM", 3, esfm(npcommid, &texts)),
        (
            "TROP_01.ESFM",
            3,
            esfm(npcommid, &texts.replace("First", "Erster")),
        ),
        ("TROP001.PNG", 0, PNG.to_vec()),
        ("ICON0.PNG", 0, PNG.to_vec()),
        ("GR001.PNG", 0, PNG.to_vec()),
    ]);
    std::fs::write(sys_dir.join("trophy").join("trophy01.trp"), pack).unwrap();
    if npbind {
        let mut bytes = vec![0u8; 7];
        bytes.extend_from_slice(npcommid.as_bytes());
        bytes.extend_from_slice(&[0; 9]);
        std::fs::write(sys_dir.join("npbind.dat"), bytes).unwrap();
    }
}

/// A fresh folder under the system temp dir, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!(
            "uncharted-trophies-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn pack(game: &Path) -> PathBuf {
    game.join("sce_sys").join("trophy").join("trophy01.trp")
}

fn trophy(set: &TrophySet, id: u32) -> &Trophy {
    set.trophies.iter().find(|t| t.id == id).unwrap()
}

// ---- AES ----

#[test]
fn fips_197_vector() {
    let key: [u8; 16] = std::array::from_fn(|i| i as u8);
    let aes = Aes128::new(&key);
    let plain: [u8; 16] = *b"\x00\x11\x22\x33\x44\x55\x66\x77\x88\x99\xaa\xbb\xcc\xdd\xee\xff";
    let cipher: [u8; 16] = *b"\x69\xc4\xe0\xd8\x6a\x7b\x04\x30\xd8\xcd\xb7\x80\x70\xb4\xc5\x5a";
    assert_eq!(aes.encrypt_block(&plain), cipher);
    assert_eq!(aes.decrypt_block(&cipher), plain);
}

#[test]
fn cbc_round_trip() {
    let aes = Aes128::new(b"0123456789abcdef");
    let data: Vec<u8> = (0..64).collect();
    let iv = [b'i'; 16];
    assert_eq!(aes.cbc_decrypt(&cbc_encrypt(&aes, &data, &iv), &iv), data);
}

// ---- The pack ----

#[test]
fn load_with_known_id() {
    let temp = TempDir::new("known");
    let game = temp.0.join("game");
    fixture(&game, "NPWR09798_00", false);
    let set = trophies::load_trp(&pack(&game), &game, 8, "", &key()).unwrap();
    assert_eq!(set.npcommid, "NPWR09798_00");
    assert_eq!(set.title, "Uncharted\u{2122} Test");
    let ids: Vec<u32> = set.trophies.iter().map(|t| t.id).collect();
    assert_eq!(ids, [0, 1, 2]);
    assert_eq!(trophy(&set, 0).grade, Grade::Platinum);
    assert!(trophy(&set, 2).hidden);
    assert_eq!(trophy(&set, 1).name, "First");
    assert_eq!(set.groups, 1);
    let icons: Vec<&str> = set.icons.keys().map(String::as_str).collect();
    assert_eq!(icons, ["ICON0.PNG", "TROP001.PNG"]);
    // `load` finds the same pack from the game.
    assert_eq!(trophies::load(&game, Game::U1, 8, &key()).unwrap(), set);
}

#[test]
fn language_text() {
    let temp = TempDir::new("language");
    let game = temp.0.join("game");
    fixture(&game, "NPWR09798_00", false);
    let set = trophies::load_trp(&pack(&game), &game, 1, "", &key()).unwrap();
    assert_eq!(trophy(&set, 1).name, "Erster");
}

#[test]
fn id_from_npbind_and_by_search() {
    let temp = TempDir::new("npbind");
    let game = temp.0.join("game");
    fixture(&game, "NPWR00123_00", true);
    let found = trophies::load_trp(&pack(&game), &game, 1, "", &key()).unwrap();
    assert_eq!(found.npcommid, "NPWR00123_00");
    std::fs::remove_file(game.join("sce_sys").join("npbind.dat")).unwrap();
    let searched = trophies::load_trp(&pack(&game), &game, 1, "", &key()).unwrap();
    assert_eq!(searched.npcommid, "NPWR00123_00");
}

#[test]
fn not_a_pack() {
    let temp = TempDir::new("broken");
    let broken = temp.0.join("broken.trp");
    std::fs::write(&broken, b"nope".repeat(100)).unwrap();
    let err = trophies::read_trp(&broken).unwrap_err();
    assert_eq!(err.code(), "trophies");
    assert_eq!(err.to_string(), "Trophies: broken.trp: not a trophy pack");
    // `load` says the same for a damaged pack of a game, and for a pack in another key.
    let game = temp.0.join("game");
    fixture(&game, "NPWR09798_00", false);
    let wrong = TrophyKey::from_hex("000102030405060708090a0b0c0d0e0f").unwrap();
    let err = trophies::load(&game, Game::U1, 1, &wrong).unwrap_err();
    assert_eq!(err.code(), "trophies");
    assert!(err.to_string().contains("no trophy key fits"));
    std::fs::write(pack(&game), b"nope".repeat(100)).unwrap();
    assert_eq!(
        trophies::load(&game, Game::U1, 1, &key())
            .unwrap_err()
            .code(),
        "trophies"
    );
}

#[test]
fn cache_round_trip_and_reuse() {
    let temp = TempDir::new("cache");
    let game = temp.0.join("game");
    fixture(&game, "NPWR09798_00", false);
    let out = temp.0.join("out");
    assert_eq!(
        trophies::prepare(&game, Game::U1, &out, 8, false, &key()).unwrap(),
        Some(out.clone())
    );
    let set = trophies::read_cache(&out).unwrap();
    assert_eq!(set.npcommid, "NPWR09798_00");
    let secret = trophy(&set, 2);
    assert_eq!(
        (
            secret.id,
            secret.grade,
            secret.hidden,
            secret.name.as_str(),
            secret.detail.as_str()
        ),
        (2, Grade::Gold, true, "Secret", "Do\ta\nthing")
    );
    assert_eq!(std::fs::read(out.join("TROP001.PNG")).unwrap(), PNG);
    assert!(!out.join("GR001.PNG").exists());
    let cache = out.join(trophies::CACHE_FILE);
    let written = std::fs::read(&cache).unwrap();
    let time = std::fs::metadata(&cache).unwrap().modified().unwrap();
    // Same pack, same language: kept as it is, even with a key that would not open it.
    let wrong = TrophyKey::from_bytes([0; 16]);
    trophies::prepare(&game, Game::U1, &out, 8, false, &wrong).unwrap();
    assert_eq!(std::fs::metadata(&cache).unwrap().modified().unwrap(), time);
    assert_eq!(std::fs::read(&cache).unwrap(), written);
    trophies::prepare(&game, Game::U1, &out, 1, false, &key()).unwrap();
    assert_eq!(
        trophy(&trophies::read_cache(&out).unwrap(), 1).name,
        "Erster"
    );
    // The file is the runtime's format 2, line for line.
    let text = std::fs::read_to_string(&cache).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "format\t2");
    assert!(lines[1].starts_with("stamp\t") && lines[1].ends_with("|1"));
    assert_eq!(
        &lines[2..],
        [
            "npcommid\tNPWR09798_00",
            "title\tUncharted\u{2122} Test",
            "detail\tA game",
            "groups\t1",
            "trophy\t0\tP\t0\tPlatinum\tAll of them",
            "trophy\t1\tB\t0\tErster\tDo the first thing",
            "trophy\t2\tG\t1\tSecret\tDo\\ta\\nthing",
        ]
    );
}

#[test]
fn game_without_a_pack_has_no_cache() {
    let temp = TempDir::new("nopack");
    let game = temp.0.join("game");
    let out = temp.0.join("out");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("trophies.txt"), "stale").unwrap();
    assert_eq!(
        trophies::prepare(&game, Game::U2, &out, 1, false, &key()).unwrap(),
        None
    );
    assert!(!out.join("trophies.txt").exists());
}

#[test]
fn a_damaged_pack_removes_the_cache() {
    let temp = TempDir::new("damaged");
    let game = temp.0.join("game");
    fixture(&game, "NPWR09798_00", false);
    let out = temp.0.join("out");
    trophies::prepare(&game, Game::U1, &out, 1, false, &key()).unwrap();
    std::fs::write(pack(&game), b"nope".repeat(100)).unwrap();
    let err = trophies::prepare(&game, Game::U1, &out, 1, false, &key()).unwrap_err();
    assert_eq!(err.code(), "trophies");
    assert!(!out.join("trophies.txt").exists());
}

#[test]
fn text_escapes() {
    assert_eq!(
        trophies::unescape(&trophies::escape("a\tb\nc\\d")),
        "a\tb\nc\\d"
    );
    assert_eq!(trophies::escape("a\r\nb"), "a\\nb");
    assert_eq!(trophies::unescape("x\\"), "x\\");
}

#[test]
fn the_key_comes_from_the_environment_at_run_time() {
    let hex = "4e6f74207468652072656164206b6579";
    let vars = |name: &str| (name == trophies::KEY_VAR).then(|| format!(" {hex}\n"));
    let from_env = trophies::TrophyKey::from_env(vars).unwrap();
    let expected = TrophyKey::from_hex(hex).unwrap();
    let probe = |k: &TrophyKey| k.title_key("NPWR00001_00").unwrap().encrypt_block(&[0; 16]);
    assert_eq!(probe(&from_env), probe(&expected));
    let err = trophies::TrophyKey::from_env(|_| None).unwrap_err();
    assert_eq!(err.code(), "trophies");
    assert!(err.to_string().contains("BB_TROPHY_KEY"));
    assert!(TrophyKey::from_hex("12").is_err());
}

// ---- Unlocks ----

#[test]
fn log_keeps_earliest_and_skips_garbage() {
    let temp = TempDir::new("log");
    let log = trophies::unlock_log(&temp.0, "NPWR09798_00");
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, "5 200\n5 100\nbroken\n7 300 extra\n\n").unwrap();
    assert_eq!(
        trophies::read_unlocks(&temp.0, "NPWR09798_00"),
        BTreeMap::from([(5, 100), (7, 300)])
    );
    assert!(trophies::read_unlocks(&temp.0, "NPWR09799_00").is_empty());
}

#[test]
fn progress() {
    let t = |id, grade| Trophy {
        id,
        grade,
        hidden: false,
        group: String::new(),
        name: String::new(),
        detail: String::new(),
    };
    let set = TrophySet {
        trophies: vec![
            t(0, Grade::Platinum),
            t(1, Grade::Bronze),
            t(2, Grade::Gold),
            t(3, Grade::Bronze),
        ],
        ..TrophySet::default()
    };
    let progress = trophies::progress(&set, &BTreeMap::from([(1, 5), (2, 6)]));
    assert_eq!((progress.unlocked, progress.percent), (2, 50));
    assert_eq!(progress.total, 4);
    assert_eq!(
        progress.earned,
        GradeCounts {
            platinum: 0,
            gold: 1,
            silver: 0,
            bronze: 1
        }
    );
    assert_eq!(
        progress.by_grade,
        GradeCounts {
            platinum: 1,
            gold: 1,
            silver: 0,
            bronze: 2
        }
    );
    assert_eq!(
        trophies::progress(&TrophySet::default(), &BTreeMap::new()).percent,
        0
    );
}

// ---- The dump ----

/// Needs the collection dump in `UNCHARTED_DUMP` (the `CUSA02320` folder) and the console's
/// trophy key in `BB_TROPHY_KEY`: `cargo test -p uncharted_core --test trophies -- --ignored`.
#[test]
#[ignore = "needs the collection dump (UNCHARTED_DUMP) and the console's trophy key (BB_TROPHY_KEY)"]
fn the_three_sets() {
    let dump = PathBuf::from(std::env::var("UNCHARTED_DUMP").expect("UNCHARTED_DUMP is set"));
    let key = TrophyKey::from_env(|k| std::env::var(k).ok()).unwrap();
    for game in Game::ALL {
        let set = trophies::load(&dump, game, 1, &key).unwrap();
        assert_eq!(set.npcommid, trophies::trp_file(game).1);
        assert!(set.trophies.len() > 40);
        assert_eq!(set.trophies[0].grade, Grade::Platinum);
    }
}
