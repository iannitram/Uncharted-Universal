# 0004: Three home build hubs

**Decided:** 2026-11-01, in the spec approval ("Approve (Recommended)"), mirroring totk-native ADR 0003.

## Decision

Only the hubs hold my dump: the Strix Halo tablet (prepares for both SteamOS handhelds and Android), the RTX 3090 Ti desktop (Windows), the Mac (Mac, iPhone, iPad). Other devices find a hub by mDNS on the home network, pair once with a six-digit code (SPAKE2), and pull a sealed bundle: the prepared install, the patch level and the GPU-independent shader translations. A driver pipeline cache travels only between devices with the same GPU and driver. Nothing goes through a server or save-relay's storage.

## Discovery record

Added 2026-11-12 with roadmap 4.01 (`crates/core/src/hub/discovery.rs`). A hub announces the mDNS service `_uncharted-universal._tcp.local.` with its TCP port and a TXT record of two keys: `hub=<recipe::Hub::key()>` (`strix-halo`, `desktop`, `mac`) and `targets=<Os::key()>,...` (comma-separated, no spaces, for example `linux,android`). Keys match without case and only the first copy counts (RFC 6763 6.4); other keys are ignored so a later version can add some. A device drops a record with an unknown hub, an empty, unknown or repeated target, or port 0, and drops every address outside the home network (loopback, RFC 1918, link-local, IPv6 ULA). Lifted from totk-native's schema without its Switch-specific parts.

## Bundle stream

Added 2026-11-12 with roadmap 4.03 (`hub::serve`, `hub::pull`). A hub publishes each build as `builds/<build name>/bundle.txt` (`bundle::to_text`) with its files beside it; shader translations sit under `shaders/` and are listed as `file shader`. For a target, the newest `bundle.txt` by modification time wins, then the folder name. The device sends `PULLBUNDLE1 <target>` sealed with the key from pairing; the hub refuses anything else before it reads the builds folder, then sends the bundle text and, file by file, only what the device does not already hold with the right size and hash. The device stages into its own folder under the incomplete marker and runs `bundle::verify` before handing the bundle to `bundle::install`. `bundle::is_forbidden` keeps game files, game archives, keys, saves and driver pipeline caches (`.vkc`, `vulkan_driver*`, `pipeline_cache*`) out of every bundle, so the same-GPU driver cache exchange above stays a separate, later piece.

## Why

Preparing needs the dump and tens of GB; handhelds and phones should hold neither the dump nor the work.
