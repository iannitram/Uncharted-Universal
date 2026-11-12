# Documentation

Start with the [Universal README](../README.md), [approved spec](../SPEC.md),
[implementation roadmap](../ROADMAP.md) and [code map](code-map.md).
The [platform peer survey](../PEERS.md) records head starts for Linux, Windows,
macOS, iOS/iPadOS and Android; the
[2026-11-06 readiness audit](audits/2026-11-06-universal-platforms.md) records
what is actually implemented. Earlier research is in [`../Research/`](../Research/README.md).

Most of the design notes below come from the Bloodborne port (bbport) that this project grew
out of. The engine code they describe (loader, runtime, renderer, upscalers) is the same one
that runs Uncharted. They were written in Russian and translated on 2026-10-16. The originals
are in [`ru/`](ru/), at the same paths.

## Building

- [BUILDING_LINUX.md](BUILDING_LINUX.md): building and running on Linux from source, with every dependency's minimum and tested version.

## Plans

- [../ROADMAP.md](../ROADMAP.md): Universal implementation stages (1.0: Windows and Linux/SteamOS; Mac, iOS and Android are 1.x).
- [ROADMAP.md](ROADMAP.md): historical Linux-fork goals and phases, retained from upstream.
- [history/BLOODBORNE_ROADMAP_2026-09-30.md](history/BLOODBORNE_ROADMAP_2026-09-30.md): the
  Bloodborne-era plan, with the state of the renderer and upscaler at the end of September.

## Design notes

| Document | Topic | Related research |
|---|---|---|
| [parallel_gpu.md](parallel_gpu.md) | The GPU command thread, the two-stage draw pipeline, hitches when data loads | [Architecture: GPU library](../Research/codebase-architecture.md#gpu-library) |
| [upscaler.md](upscaler.md) | Frame analysis, scene constants, FSR 3.1/4 and FSR 4.1.1 integration, frame generation | [Graphics: FSR on Vulkan](../Research/graphics-upscaling-and-performance.md#amd-fsr-31-fsr-4-and-redstone-on-vulkan) |
| [motion_vectors.md](motion_vectors.md) | Camera and per-object motion vectors | [Graphics: key takeaways](../Research/graphics-upscaling-and-performance.md#key-takeaways-for-uncharted_pc) |
| [DEPTH_ADAPTIVE_TAA.md](DEPTH_ADAPTIVE_TAA.md) | The port's own TAA | — |
| [TEMPORAL_REVIEW_2026-10-01.md](TEMPORAL_REVIEW_2026-10-01.md) | Review of TAA and upscaler inputs | — |
| [MODS.md](MODS.md) | File mods and third-party patches (Bloodborne era) | [Architecture: Bloodborne leftovers](../Research/codebase-architecture.md#uncharted-specific-vs-bloodborne-leftovers) |

## Logs and change notes

- [DEVELOPMENT_LOG.md](DEVELOPMENT_LOG.md): from the first native run of Bloodborne onwards
  (Russian: [DEVELOPMENT_LOG.ru.md](DEVELOPMENT_LOG.ru.md)).
- [CHANGES_2026-10-02.md](CHANGES_2026-10-02.md), [CHANGES_2026-10-03.md](CHANGES_2026-10-03.md).
- [history/](history/): status reports, fix logs and session notes from late September 2026.
- [original-readme/](original-readme/README.md): the bbport README (English and Russian).
