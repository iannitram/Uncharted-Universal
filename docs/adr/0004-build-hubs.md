# 0004: Three home build hubs

**Decided:** 2026-11-01, in the spec approval ("Approve (Recommended)"), mirroring totk-native ADR 0003.

## Decision

Only the hubs hold my dump: the Strix Halo tablet (prepares for both SteamOS handhelds and Android), the RTX 3090 Ti desktop (Windows), the Mac (Mac, iPhone, iPad). Other devices find a hub by mDNS on the home network, pair once with a six-digit code (SPAKE2), and pull a sealed bundle: the prepared install, the patch level and the GPU-independent shader translations. A driver pipeline cache travels only between devices with the same GPU and driver. Nothing goes through a server or save-relay's storage.

## Why

Preparing needs the dump and tens of GB; handhelds and phones should hold neither the dump nor the work.
