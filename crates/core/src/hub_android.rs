//! Android APK + prepared-data manifest descriptor and SHA-256 verification helper for Stage
//! 7.04 (ADR 0004, ADR 0005).
//!
//! The Strix Halo build hub serves both the Android launcher/runtime APK and the prepared
//! `Os::Android` bundle (runtime libraries, settings, and GPU-independent shader translations)
//! to a paired Android device. Game dumps, keys, saves, and driver pipeline caches remain
//! forbidden via [`bundle::is_forbidden`].

use crate::bundle::{self, sha256_hex, Bundle, Kind};
use crate::error::{Error, Result};
use crate::platform::Os;
use serde_json::{json, Value};
use std::path::Path;

/// Default Android ABI for Stage 7 builds (`android/components.lock.json`, ADR 0005).
pub const ANDROID_ABI: &str = "arm64-v8a";
/// Minimum Android API level (`androidApi: 33` in `android/components.lock.json`).
pub const ANDROID_MIN_SDK: u32 = 33;
/// Default manifest filename served alongside a published Android hub release.
pub const ANDROID_HUB_MANIFEST_FILE: &str = "android-hub.txt";

const HEADER: &str = "uncharted-android-hub 1";

/// Descriptor for an Android APK artifact served by a build hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApkArtifact {
    /// Relative path inside the hub's release directory (using `/`, ending in `.apk`).
    pub path: String,
    /// Human-readable version string (e.g. `"0.3.15"`).
    pub version: String,
    /// Monotonic Android `versionCode` (> 0).
    pub version_code: u64,
    /// Target Android ABI (e.g. `"arm64-v8a"`).
    pub abi: String,
    /// Minimum Android API level (e.g. `33`).
    pub min_sdk: u32,
    /// Exact byte size of the APK.
    pub size: u64,
    /// Lowercase 64-character hex SHA-256 digest of the APK.
    pub sha256: String,
}

/// Combined Stage 7.04 descriptor pairing an Android APK with its prepared `Os::Android` bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidHubManifest {
    pub apk: ApkArtifact,
    pub install: Bundle,
}

/// Returns `true` when `rel_path` is a safe relative `.apk` path that passes
/// [`bundle::is_forbidden`].
pub fn is_valid_apk_path(rel_path: &str) -> bool {
    !bundle::is_forbidden(rel_path) && rel_path.to_ascii_lowercase().ends_with(".apk")
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_clean_token(value: &str) -> bool {
    !value.is_empty()
        && !value
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
}

fn regular_file_in_tree(root: &Path, rel_path: &str) -> Result<()> {
    let mut current = root.to_path_buf();
    let parts: Vec<&str> = rel_path.split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        let metadata = std::fs::symlink_metadata(&current).map_err(|err| {
            Error::Hub(format!(
                "cannot inspect {} in Android hub directory: {err}",
                rel_path
            ))
        })?;
        let expected_type = if index + 1 == parts.len() {
            metadata.file_type().is_file()
        } else {
            metadata.file_type().is_dir()
        };
        if !expected_type {
            return Err(Error::Hub(format!(
                "refusing non-regular Android hub path {rel_path}"
            )));
        }
    }
    Ok(())
}

/// Computes `(byte_len, lowercase_sha256_hex)` for a regular file on disk, refusing symlinks
/// and non-regular entries.
pub fn sha256_file(path: &Path) -> Result<(u64, String)> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|err| Error::Hub(format!("missing file {}: {err}", path.display())))?;
    if !metadata.file_type().is_file() {
        return Err(Error::Hub(format!(
            "refusing non-regular file {}",
            path.display()
        )));
    }
    let bytes = std::fs::read(path)?;
    Ok((bytes.len() as u64, sha256_hex(&bytes)))
}

impl ApkArtifact {
    /// Validates every field of the APK descriptor without touching disk.
    pub fn validate(&self) -> Result<()> {
        if bundle::is_forbidden(&self.path) {
            return Err(Error::Hub(format!(
                "refusing to bundle {}: game files, keys and saves never leave a hub",
                self.path
            )));
        }
        if !is_valid_apk_path(&self.path) {
            return Err(Error::Hub(format!(
                "invalid Android APK path {}: must be a safe relative .apk path",
                self.path
            )));
        }
        if !is_clean_token(&self.version) {
            return Err(Error::Hub(
                "Android APK version must be a non-empty token without whitespace".into(),
            ));
        }
        if self.version_code == 0 {
            return Err(Error::Hub(
                "Android APK version_code must be greater than zero".into(),
            ));
        }
        if !is_clean_token(&self.abi) {
            return Err(Error::Hub(
                "Android APK ABI must be a non-empty token without whitespace".into(),
            ));
        }
        if self.min_sdk == 0 {
            return Err(Error::Hub(
                "Android APK min_sdk must be greater than zero".into(),
            ));
        }
        if !is_sha256_hex(&self.sha256) {
            return Err(Error::Hub(format!(
                "Android APK {} has an invalid SHA-256 digest",
                self.path
            )));
        }
        Ok(())
    }
}

/// Reads and hashes an APK file inside `root` at `rel_path`, returning a validated
/// [`ApkArtifact`].
pub fn describe_apk(
    root: &Path,
    rel_path: &str,
    version: &str,
    version_code: u64,
    abi: &str,
    min_sdk: u32,
) -> Result<ApkArtifact> {
    if bundle::is_forbidden(rel_path) {
        return Err(Error::Hub(format!(
            "refusing to bundle {rel_path}: game files, keys and saves never leave a hub"
        )));
    }
    if !is_valid_apk_path(rel_path) {
        return Err(Error::Hub(format!(
            "invalid Android APK path {rel_path}: must be a safe relative .apk path"
        )));
    }
    regular_file_in_tree(root, rel_path)?;
    let (size, sha256) = sha256_file(&root.join(rel_path))?;
    let artifact = ApkArtifact {
        path: rel_path.to_owned(),
        version: version.to_owned(),
        version_code,
        abi: abi.to_owned(),
        min_sdk,
        size,
        sha256,
    };
    artifact.validate()?;
    Ok(artifact)
}

/// Verifies that the APK at `root.join(&apk.path)` exists as a regular file and matches `apk.size`
/// and `apk.sha256`.
pub fn verify_apk(apk: &ApkArtifact, root: &Path) -> Result<()> {
    apk.validate()?;
    regular_file_in_tree(root, &apk.path).map_err(|_| {
        Error::Hub(format!(
            "{} does not match the Android hub manifest",
            apk.path
        ))
    })?;
    let (size, digest) = sha256_file(&root.join(&apk.path)).map_err(|_| {
        Error::Hub(format!(
            "{} does not match the Android hub manifest",
            apk.path
        ))
    })?;
    if size != apk.size || digest != apk.sha256 {
        return Err(Error::Hub(format!(
            "{} does not match the Android hub manifest",
            apk.path
        )));
    }
    Ok(())
}

/// Returns `true` when `local_root` does not already hold an intact copy of `apk`.
pub fn apk_needs_transfer(apk: &ApkArtifact, local_root: &Path) -> bool {
    verify_apk(apk, local_root).is_err()
}

impl AndroidHubManifest {
    /// Constructs and validates an [`AndroidHubManifest`], enforcing `install.target == Os::Android`
    /// and rejecting any forbidden paths.
    pub fn new(apk: ApkArtifact, install: Bundle) -> Result<Self> {
        apk.validate()?;
        if install.target != Os::Android {
            return Err(Error::Hub(format!(
                "Android hub manifest requires target android, found {}",
                install.target.key()
            )));
        }
        for file in &install.files {
            if bundle::is_forbidden(&file.path) {
                return Err(Error::Hub(format!(
                    "refusing to bundle {}: game files, keys and saves never leave a hub",
                    file.path
                )));
            }
            if !is_sha256_hex(&file.sha256) {
                return Err(Error::Hub(format!(
                    "bundle file {} has an invalid SHA-256 digest",
                    file.path
                )));
            }
        }
        Ok(Self { apk, install })
    }

    /// Builds a Stage 7.04 manifest from an APK under `apk_root/apk_rel_path` and a prepared
    /// `Os::Android` build directory `prepared_dir`.
    pub fn build(
        apk_root: &Path,
        apk_rel_path: &str,
        version: &str,
        version_code: u64,
        prepared_dir: &Path,
        patch_level: u32,
    ) -> Result<Self> {
        Self::build_with_spec(
            apk_root,
            apk_rel_path,
            version,
            version_code,
            ANDROID_ABI,
            ANDROID_MIN_SDK,
            prepared_dir,
            patch_level,
        )
    }

    /// Like [`AndroidHubManifest::build`], with explicit `abi` and `min_sdk`.
    #[allow(clippy::too_many_arguments)]
    pub fn build_with_spec(
        apk_root: &Path,
        apk_rel_path: &str,
        version: &str,
        version_code: u64,
        abi: &str,
        min_sdk: u32,
        prepared_dir: &Path,
        patch_level: u32,
    ) -> Result<Self> {
        let apk = describe_apk(apk_root, apk_rel_path, version, version_code, abi, min_sdk)?;
        let install = bundle::manifest(prepared_dir, Os::Android, patch_level)?;
        Self::new(apk, install)
    }

    /// Verifies both the APK under `apk_root` and every file of `self.install` under `install_dir`.
    pub fn verify_all(&self, apk_root: &Path, install_dir: &Path) -> Result<()> {
        if self.install.target != Os::Android {
            return Err(Error::Hub(format!(
                "Android hub manifest requires target android, found {}",
                self.install.target.key()
            )));
        }
        verify_apk(&self.apk, apk_root)?;
        bundle::verify(&self.install, install_dir)?;
        Ok(())
    }

    /// Serializes the manifest in deterministic line format:
    /// `uncharted-android-hub 1`, `apk ...`, then [`bundle::to_text`].
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(HEADER);
        out.push('\n');
        out.push_str(&format!(
            "apk {} {} {} {} {} {} {}\n",
            self.apk.version,
            self.apk.version_code,
            self.apk.abi,
            self.apk.min_sdk,
            self.apk.size,
            self.apk.sha256,
            self.apk.path
        ));
        out.push_str(&bundle::to_text(&self.install));
        out
    }

    /// Parses [`AndroidHubManifest::to_text`] and validates every field.
    pub fn from_text(text: &str) -> Result<Self> {
        let mut lines = text.lines();
        let header = lines.next().unwrap_or("");
        if header != HEADER {
            return Err(Error::Hub(format!(
                "bad android hub manifest line 1: {header}"
            )));
        }
        let apk_line = lines.next().unwrap_or("");
        let mut fields = apk_line.splitn(8, ' ');
        let tag = fields.next().unwrap_or("");
        let version = fields.next().unwrap_or("");
        let version_code = fields.next().unwrap_or("");
        let abi = fields.next().unwrap_or("");
        let min_sdk = fields.next().unwrap_or("");
        let size = fields.next().unwrap_or("");
        let sha256 = fields.next().unwrap_or("");
        let path = fields.next().unwrap_or("");

        let bad_apk = || Error::Hub(format!("bad android hub manifest line 2: {apk_line}"));
        if tag != "apk" || path.is_empty() {
            return Err(bad_apk());
        }
        let version_code = version_code.parse::<u64>().map_err(|_| bad_apk())?;
        let min_sdk = min_sdk.parse::<u32>().map_err(|_| bad_apk())?;
        let size = size.parse::<u64>().map_err(|_| bad_apk())?;

        let apk = ApkArtifact {
            path: path.to_owned(),
            version: version.to_owned(),
            version_code,
            abi: abi.to_owned(),
            min_sdk,
            size,
            sha256: sha256.to_owned(),
        };
        apk.validate()?;

        let rest: String = lines.map(|l| format!("{l}\n")).collect();
        let install = bundle::from_text(&rest)?;
        Self::new(apk, install)
    }

    /// Serializes the manifest as JSON for hub HTTP/RPC control frames.
    pub fn to_json(&self) -> Result<String> {
        let files: Vec<Value> = self
            .install
            .files
            .iter()
            .map(|f| {
                json!({
                    "path": f.path,
                    "size": f.size,
                    "sha256": f.sha256,
                    "kind": match f.kind {
                        Kind::Install => "install",
                        Kind::ShaderTranslation => "shader",
                    }
                })
            })
            .collect();
        let doc = json!({
            "format": 1,
            "apk": {
                "path": self.apk.path,
                "version": self.apk.version,
                "version_code": self.apk.version_code,
                "abi": self.apk.abi,
                "min_sdk": self.apk.min_sdk,
                "size": self.apk.size,
                "sha256": self.apk.sha256,
            },
            "install": {
                "target": self.install.target.key(),
                "patch_level": self.install.patch_level,
                "build_id": self.install.build_id,
                "files": files,
            }
        });
        serde_json::to_string(&doc)
            .map_err(|err| Error::Hub(format!("failed to serialize Android hub manifest: {err}")))
    }

    /// Parses [`AndroidHubManifest::to_json`] and validates all fields and forbidden paths.
    pub fn from_json(text: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(text)
            .map_err(|err| Error::Hub(format!("bad android hub json: {err}")))?;
        if value["format"].as_u64() != Some(1) {
            return Err(Error::Hub("unsupported android hub manifest format".into()));
        }
        let apk_obj = &value["apk"];
        let apk = ApkArtifact {
            path: apk_obj["path"]
                .as_str()
                .ok_or_else(|| Error::Hub("missing apk.path".into()))?
                .to_owned(),
            version: apk_obj["version"]
                .as_str()
                .ok_or_else(|| Error::Hub("missing apk.version".into()))?
                .to_owned(),
            version_code: apk_obj["version_code"]
                .as_u64()
                .ok_or_else(|| Error::Hub("missing apk.version_code".into()))?,
            abi: apk_obj["abi"]
                .as_str()
                .ok_or_else(|| Error::Hub("missing apk.abi".into()))?
                .to_owned(),
            min_sdk: apk_obj["min_sdk"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| Error::Hub("missing apk.min_sdk".into()))?,
            size: apk_obj["size"]
                .as_u64()
                .ok_or_else(|| Error::Hub("missing apk.size".into()))?,
            sha256: apk_obj["sha256"]
                .as_str()
                .ok_or_else(|| Error::Hub("missing apk.sha256".into()))?
                .to_owned(),
        };

        let inst_obj = &value["install"];
        let target_str = inst_obj["target"]
            .as_str()
            .ok_or_else(|| Error::Hub("missing install.target".into()))?;
        let target = Os::from_key(target_str)
            .ok_or_else(|| Error::Hub(format!("unknown install.target: {target_str}")))?;
        let patch_level = inst_obj["patch_level"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| Error::Hub("missing install.patch_level".into()))?;
        let build_id = inst_obj["build_id"]
            .as_str()
            .ok_or_else(|| Error::Hub("missing install.build_id".into()))?
            .to_owned();
        let file_arr = inst_obj["files"]
            .as_array()
            .ok_or_else(|| Error::Hub("missing install.files".into()))?;
        let mut files = Vec::with_capacity(file_arr.len());
        for entry in file_arr {
            let path = entry["path"]
                .as_str()
                .ok_or_else(|| Error::Hub("missing install file path".into()))?
                .to_owned();
            let size = entry["size"]
                .as_u64()
                .ok_or_else(|| Error::Hub("missing install file size".into()))?;
            let sha256 = entry["sha256"]
                .as_str()
                .ok_or_else(|| Error::Hub("missing install file sha256".into()))?
                .to_owned();
            let kind = match entry["kind"].as_str() {
                Some("install") => Kind::Install,
                Some("shader") => Kind::ShaderTranslation,
                _ => return Err(Error::Hub("invalid install file kind".into())),
            };
            files.push(bundle::BundleFile {
                path,
                size,
                sha256,
                kind,
            });
        }
        let install = Bundle {
            target,
            patch_level,
            build_id,
            files,
        };
        Self::new(apk, install)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "uncharted-hub-android-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn build_verify_and_round_trip_android_hub_manifest() {
        let root = temp_dir("round-trip");
        let apk_root = root.join("hub-apks");
        let prep_dir = root.join("prepared-android");
        let apk_bytes = b"synthetic-uncharted-android-apk-v1";
        write_file(&apk_root, "apk/uncharted-android-0.3.15.apk", apk_bytes);
        write_file(
            &prep_dir,
            "lib/arm64-v8a/libuncharted_runtime.so",
            b"synthetic arm64 runtime",
        );
        write_file(&prep_dir, "shaders/0123abcd.spv", b"shader spirv");
        write_file(&prep_dir, "saves/ignored.sav", b"device-owned");

        let manifest = AndroidHubManifest::build(
            &apk_root,
            "apk/uncharted-android-0.3.15.apk",
            "0.3.15",
            42,
            &prep_dir,
            7,
        )
        .unwrap();

        assert_eq!(manifest.apk.size, apk_bytes.len() as u64);
        assert_eq!(manifest.apk.sha256, sha256_hex(apk_bytes));
        assert_eq!(manifest.apk.abi, ANDROID_ABI);
        assert_eq!(manifest.apk.min_sdk, ANDROID_MIN_SDK);
        assert_eq!(manifest.install.target, Os::Android);
        assert_eq!(manifest.install.patch_level, 7);
        assert_eq!(manifest.install.files.len(), 2);

        manifest.verify_all(&apk_root, &prep_dir).unwrap();
        assert!(!apk_needs_transfer(&manifest.apk, &apk_root));

        let text = manifest.to_text();
        assert_eq!(AndroidHubManifest::from_text(&text), Ok(manifest.clone()));

        let json = manifest.to_json().unwrap();
        assert_eq!(AndroidHubManifest::from_json(&json), Ok(manifest));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verify_detects_tampered_apk_and_tampered_prepared_data() {
        let root = temp_dir("tamper");
        let apk_root = root.join("apks");
        let prep_dir = root.join("prep");
        write_file(&apk_root, "apk/app.apk", b"valid-apk-payload");
        write_file(&prep_dir, "lib/libruntime.so", b"runtime-payload");

        let manifest =
            AndroidHubManifest::build(&apk_root, "apk/app.apk", "1.0.0", 1, &prep_dir, 0).unwrap();

        let client_apks = root.join("client-apks");
        assert!(apk_needs_transfer(&manifest.apk, &client_apks));
        write_file(&client_apks, "apk/app.apk", b"valid-apk-payload");
        assert!(!apk_needs_transfer(&manifest.apk, &client_apks));

        // Flip 1 byte in the APK (same size, different SHA-256).
        write_file(&apk_root, "apk/app.apk", b"valid-apk-payloae");
        assert!(apk_needs_transfer(&manifest.apk, &apk_root));
        assert_eq!(
            manifest.verify_all(&apk_root, &prep_dir),
            Err(Error::Hub(
                "apk/app.apk does not match the Android hub manifest".into()
            ))
        );

        // Restore APK, tamper prepared file.
        write_file(&apk_root, "apk/app.apk", b"valid-apk-payload");
        write_file(&prep_dir, "lib/libruntime.so", b"corrupted");
        assert_eq!(
            manifest.verify_all(&apk_root, &prep_dir),
            Err(Error::Hub(
                "lib/libruntime.so does not match the bundle".into()
            ))
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_non_apk_non_android_and_forbidden_paths() {
        let root = temp_dir("rejections");
        let apk_root = root.join("apks");
        let prep_dir = root.join("prep");
        write_file(&apk_root, "apk/not-an-apk.bin", b"123");
        write_file(&apk_root, "apk/valid.apk", b"apk");
        write_file(&prep_dir, "lib/libruntime.so", b"runtime");

        assert!(matches!(
            describe_apk(&apk_root, "apk/not-an-apk.bin", "1.0.0", 1, ANDROID_ABI, 33),
            Err(Error::Hub(msg)) if msg.contains(".apk")
        ));
        assert!(matches!(
            describe_apk(&apk_root, "../escape.apk", "1.0.0", 1, ANDROID_ABI, 33),
            Err(Error::Hub(msg)) if msg.contains("refusing to bundle")
        ));
        assert!(matches!(
            describe_apk(&apk_root, "saves/app.apk", "1.0.0", 1, ANDROID_ABI, 33),
            Err(Error::Hub(msg)) if msg.contains("refusing to bundle")
        ));

        let valid_apk =
            describe_apk(&apk_root, "apk/valid.apk", "1.0.0", 1, ANDROID_ABI, 33).unwrap();
        let linux_bundle = bundle::manifest(&prep_dir, Os::Linux, 0).unwrap();
        assert!(matches!(
            AndroidHubManifest::new(valid_apk, linux_bundle),
            Err(Error::Hub(msg)) if msg.contains("target android")
        ));

        let bad_text =
            "uncharted-android-hub 1\napk 1.0.0 1 arm64-v8a 33 3 deadbeef apk/valid.apk\n";
        assert!(matches!(
            AndroidHubManifest::from_text(bad_text),
            Err(Error::Hub(msg)) if msg.contains("SHA-256")
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_apk() {
        let root = temp_dir("symlink");
        let real = root.join("real.apk");
        std::fs::write(&real, b"apk-bytes").unwrap();
        let link = root.join("link.apk");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert!(matches!(
            describe_apk(&root, "link.apk", "1.0.0", 1, ANDROID_ABI, 33),
            Err(Error::Hub(msg)) if msg.contains("non-regular")
        ));

        std::fs::remove_dir_all(root).unwrap();
    }
}
