# 0004: Three home build hubs

**Decided:** 2026-11-01, in the spec approval ("Approve (Recommended)"), mirroring totk-native ADR 0003.

## Decision

Only the hubs hold my dump: the Strix Halo tablet (prepares for both SteamOS handhelds and Android), the RTX 3090 Ti desktop (Windows), the Mac (Mac, iPhone, iPad). Other devices find a hub by mDNS on the home network, pair once with a six-digit code (SPAKE2), and pull a sealed bundle: the prepared install, the patch level and the GPU-independent shader translations. A driver pipeline cache travels only between devices with the same GPU and driver. Nothing goes through a server or save-relay's storage.

## Discovery record

Added 2026-11-12 with roadmap 4.01 (`crates/core/src/hub/discovery.rs`). A hub announces the mDNS service `_uncharted-universal._tcp.local.` with its TCP port and a TXT record of two keys: `hub=<recipe::Hub::key()>` (`strix-halo`, `desktop`, `mac`) and `targets=<Os::key()>,...` (comma-separated, no spaces, for example `linux,android`). Keys match without case and only the first copy counts (RFC 6763 6.4); other keys are ignored so a later version can add some. A device drops a record with an unknown hub, an empty, unknown or repeated target, or port 0, and drops every address outside the home network (loopback, RFC 1918, link-local, IPv6 ULA). Lifted from totk-native's schema without its Switch-specific parts.

## Why

Preparing needs the dump and tens of GB; handhelds and phones should hold neither the dump nor the work.
