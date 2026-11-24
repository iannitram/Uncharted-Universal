use super::*;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "uncharted-shader-preload-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sample_key() -> CacheKey {
    CacheKey {
        game: Game::U2,
        game_revision: "01.02".to_owned(),
        shader_version: 16,
        gpu_vendor_id: 0x1002,
        gpu_device_id: 0x1586,
        driver_id: 3,
        driver_version: 24_02_00,
        driver_uuid: "00112233445566778899aabbccddeeff".to_owned(),
        feature_flags: feature_flags::NAN_GUARD | feature_flags::SAFE_SHADERS,
    }
}

fn sample_entry(hash: u64, perm: u32, stage: ShaderStage) -> ShaderEntry {
    ShaderEntry {
        pgm_hash: hash,
        perm_idx: perm,
        stage,
        pipeline_key_hash: Some(hash ^ 0xfeed_beef),
        route_tag: Some("prologue".to_owned()),
    }
}

struct FakeCompiler {
    cached: HashSet<(u64, u32, ShaderStage)>,
    fail_on: HashSet<u64>,
    compile_calls: usize,
    cancel_after: Option<(usize, Arc<AtomicBool>)>,
}

impl FakeCompiler {
    fn new() -> Self {
        Self {
            cached: HashSet::new(),
            fail_on: HashSet::new(),
            compile_calls: 0,
            cancel_after: None,
        }
    }
}

impl ShaderCompiler for FakeCompiler {
    fn is_cached(&self, entry: &ShaderEntry) -> bool {
        self.cached
            .contains(&(entry.pgm_hash, entry.perm_idx, entry.stage))
    }

    fn warm_entry(&mut self, entry: &ShaderEntry) -> std::result::Result<WarmOutcome, String> {
        self.compile_calls += 1;
        if let Some((limit, flag)) = &self.cancel_after {
            if self.compile_calls >= *limit {
                flag.store(true, Ordering::Release);
            }
        }
        if self.fail_on.contains(&entry.pgm_hash) {
            return Err("driver refused pipeline".to_owned());
        }
        let inserted = self
            .cached
            .insert((entry.pgm_hash, entry.perm_idx, entry.stage));
        if inserted {
            Ok(WarmOutcome::Warmed)
        } else {
            Ok(WarmOutcome::AlreadyCached)
        }
    }
}

#[test]
fn cache_key_validates_all_dimensions() {
    let base = sample_key();
    assert_eq!(base.check_compatible(&base), Ok(()));

    let mut wrong_game = base.clone();
    wrong_game.game = Game::U3;
    assert!(matches!(
        wrong_game.check_compatible(&base),
        Err(MismatchReason::GameMismatch { .. })
    ));

    let mut wrong_rev = base.clone();
    wrong_rev.game_revision = "01.00".to_owned();
    assert!(matches!(
        wrong_rev.check_compatible(&base),
        Err(MismatchReason::GameRevisionMismatch { .. })
    ));

    let mut wrong_ver = base.clone();
    wrong_ver.shader_version = 15;
    assert!(matches!(
        wrong_ver.check_compatible(&base),
        Err(MismatchReason::ShaderVersionMismatch { .. })
    ));

    let mut wrong_gpu = base.clone();
    wrong_gpu.gpu_device_id = 0x744c;
    assert!(matches!(
        wrong_gpu.check_compatible(&base),
        Err(MismatchReason::GpuMismatch { .. })
    ));

    let mut wrong_driver = base.clone();
    wrong_driver.driver_version += 1;
    assert!(matches!(
        wrong_driver.check_compatible(&base),
        Err(MismatchReason::DriverMismatch { .. })
    ));

    let mut wrong_flags = base.clone();
    wrong_flags.feature_flags |= feature_flags::EFFECT_LOOP_CLAMP;
    assert!(matches!(
        wrong_flags.check_compatible(&base),
        Err(MismatchReason::FeatureFlagsMismatch { .. })
    ));
}

#[test]
fn manifest_recording_deduplicates_and_round_trips() {
    let dir = TempDir::new("roundtrip");
    let path = dir.0.join("preload_manifest.json");

    let mut manifest = PreloadManifest::new(sample_key());
    assert!(manifest.record_entry(sample_entry(0x2000, 0, ShaderStage::Fragment)));
    assert!(manifest.record_entry(sample_entry(0x1000, 1, ShaderStage::Vertex)));
    // Duplicate (same hash, perm, stage, pipeline_key_hash) is ignored.
    assert!(!manifest.record_entry(sample_entry(0x1000, 1, ShaderStage::Vertex)));
    assert_eq!(manifest.entries.len(), 2);

    write_manifest(&path, &manifest).unwrap();
    let (loaded, skipped) = read_manifest(&path, false).unwrap();
    assert_eq!(skipped, 0);
    assert_eq!(loaded.cache_key, manifest.cache_key);
    assert_eq!(loaded.canonical_entries(), manifest.canonical_entries());
}

#[test]
fn warm_manifest_is_idempotent_and_reports_progress() {
    let key = sample_key();
    let mut manifest = PreloadManifest::new(key.clone());
    manifest.record_entry(sample_entry(0x1111, 0, ShaderStage::Vertex));
    manifest.record_entry(sample_entry(0x2222, 0, ShaderStage::Fragment));
    manifest.record_entry(sample_entry(0x3333, 2, ShaderStage::Compute));

    let mut compiler = FakeCompiler::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut snapshots = Vec::new();

    let first = warm_manifest(&manifest, &key, &mut compiler, true, &cancel, |p| {
        snapshots.push(p.clone());
    });
    assert_eq!(
        first,
        PreloadReport {
            skipped_disabled: false,
            invalidated: None,
            total: 3,
            warmed: 3,
            already_cached: 0,
            failed: 0,
            cancelled: false,
        }
    );
    assert_eq!(compiler.compile_calls, 3);
    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots.last().unwrap().completed, 3);

    // Second launch: idempotent — all entries are already cached, zero compiles.
    let second = warm_manifest(&manifest, &key, &mut compiler, true, &cancel, |_| {});
    assert_eq!(
        second,
        PreloadReport {
            skipped_disabled: false,
            invalidated: None,
            total: 3,
            warmed: 0,
            already_cached: 3,
            failed: 0,
            cancelled: false,
        }
    );
    assert_eq!(compiler.compile_calls, 3);
}

#[test]
fn warm_manifest_respects_cancellation_and_env_toggle() {
    assert!(is_warm_enabled(|_| None));
    assert!(is_warm_enabled(|_| Some("1".to_owned())));
    assert!(!is_warm_enabled(
        |k| (k == ENV_WARM_MODE).then(|| "0".to_owned())
    ));

    let key = sample_key();
    let mut manifest = PreloadManifest::new(key.clone());
    for i in 1..=5 {
        manifest.record_entry(sample_entry(i * 0x100, 0, ShaderStage::Fragment));
    }

    let mut compiler = FakeCompiler::new();
    let cancel = Arc::new(AtomicBool::new(false));

    // Disabled by env toggle.
    let disabled = warm_manifest(&manifest, &key, &mut compiler, false, &cancel, |_| {});
    assert!(disabled.skipped_disabled);
    assert_eq!(compiler.compile_calls, 0);

    // Cancel after 2 entries.
    compiler.cancel_after = Some((2, Arc::clone(&cancel)));
    let cancelled = warm_manifest(&manifest, &key, &mut compiler, true, &cancel, |_| {});
    assert!(cancelled.cancelled);
    assert_eq!(cancelled.warmed, 2);
    assert_eq!(cancelled.total, 5);
    assert_eq!(compiler.compile_calls, 2);
}

#[test]
fn warm_manifest_invalidates_incompatible_key_and_handles_corrupt_entries() {
    let key = sample_key();
    let mut stale_key = key.clone();
    stale_key.driver_version += 10;

    let mut manifest = PreloadManifest::new(stale_key);
    manifest.record_entry(sample_entry(0xabcd, 0, ShaderStage::Vertex));

    let mut compiler = FakeCompiler::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let report = warm_manifest(&manifest, &key, &mut compiler, true, &cancel, |_| {});
    assert!(matches!(
        report.invalidated,
        Some(MismatchReason::DriverMismatch { .. })
    ));
    assert_eq!(compiler.compile_calls, 0);

    // Corrupt manifest JSON / version / entry handling.
    assert!(PreloadManifest::from_json_str("{not valid json", true).is_err());

    let bad_magic = r#"{"magic":"wrong","version":1,"cache_key":{},"entries":[]}"#;
    assert!(PreloadManifest::from_json_str(bad_magic, true).is_err());

    let with_corrupt_entry = r#"{
        "magic": "uncharted-shader-preload",
        "version": 1,
        "cache_key": {
            "game": "u2",
            "game_revision": "01.02",
            "shader_version": 16,
            "gpu_vendor_id": "0x1002",
            "gpu_device_id": "0x1586",
            "driver_id": 3,
            "driver_version": 240200,
            "driver_uuid": "00112233445566778899aabbccddeeff",
            "feature_flags": "0x0000000000000003"
        },
        "entries": [
            {"pgm_hash": "not_hex", "perm_idx": 0, "stage": "vs"},
            {"pgm_hash": "0x0000000000001234", "perm_idx": 0, "stage": "fs"}
        ]
    }"#;
    assert!(PreloadManifest::from_json_str(with_corrupt_entry, false).is_err());
    let (parsed, skipped) = PreloadManifest::from_json_str(with_corrupt_entry, true).unwrap();
    assert_eq!(skipped, 1);
    assert_eq!(parsed.entries.len(), 1);

    // Compiler failure on one entry does not abort remaining entries.
    let mut compiler2 = FakeCompiler::new();
    compiler2.fail_on.insert(0x1111);
    let mut mixed = PreloadManifest::new(key.clone());
    mixed.record_entry(sample_entry(0x1111, 0, ShaderStage::Vertex));
    mixed.record_entry(sample_entry(0x2222, 0, ShaderStage::Fragment));
    let res = warm_manifest(&mixed, &key, &mut compiler2, true, &cancel, |_| {});
    assert_eq!(res.failed, 1);
    assert_eq!(res.warmed, 1);
}
