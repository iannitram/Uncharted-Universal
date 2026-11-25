# Signing the Mac 1.0 app

## Decision

The Mac app in the 1.0 release, `uncharted-macos.zip` (Apple silicon, arm64 only), is **signed ad hoc**
(`codesign --force --sign -`, then `codesign --verify --deep --strict`; not `--deep` when signing, so the pinned Save Relay helper keeps the exact bytes its SHA-256 in `components.lock.json` names). It has no Developer ID
signature and is not notarized.

## Why

- Apple silicon will not run an unsigned binary, so the bundle is signed; an ad-hoc signature is
  enough for that and costs nothing.
- A Developer ID certificate and notarization need the paid Apple Developer Program account and the
  signing secrets that are still the owner's to provide (save-relay#287). The 1.0 line does not wait
  for them.
- The same holds for the iOS apps: they are sideloaded ad hoc and re-signed by the sideloading tool.

## What the player sees

- A downloaded zip carries the quarantine attribute, so Gatekeeper refuses the first launch ("cannot
  be opened because the developer cannot be verified"). The player opens it once with right-click, then
  Open, or runs `xattr -dr com.apple.quarantine "Uncharted Universal.app"`. The release notes say so.
- The ad-hoc code directory hash changes with every build. The Save Relay helper stores its
  credentials in the macOS Keychain (its own store, the 9.05 design, #440; the app never writes a
  credential to a file or a log), and an ad-hoc helper may make the Keychain ask again after an
  update. That is a prompt, not a lost sign-in. Nothing here reads the Keychain except the helper.
- `codesign` treats every file in `Contents/MacOS` as code, so the per-game relay manifests live in
  `Contents/Resources/save-relay-manifests` (sealed) and `Contents/MacOS/save-relay-manifests` is a link to
  them, where the launcher and `play` look.
- Everything in `Contents/MacOS` is sealed by the one bundle signature. Do not swap a helper inside a
  built app (the contract test does so only in a temporary copy).

## Upgrade path (needs the owner)

1. Join the paid program, create a Developer ID Application certificate, and store it with an
   app-specific password as repository secrets (never in the repo).
2. In `packaging/macos/bundle.sh`, sign the helper (`save-relay`) first, then the app, with
   `--options runtime` and `--timestamp`, instead of `--deep`.
3. Notarize with `xcrun notarytool submit --wait`, staple, and keep `codesign --verify --strict` and
   `spctl --assess` in the `macos` job.
4. Update this page and the release notes; the Keychain access then survives updates because the
   signing identity is stable.
