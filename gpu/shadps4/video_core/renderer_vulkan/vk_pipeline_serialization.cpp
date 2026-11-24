// SPDX-FileCopyrightText: Copyright 2025-2026 shadPS4 Emulator Project
// SPDX-License-Identifier: GPL-2.0-or-later

#include "shader_recompiler/ir/passes/ir_passes.h"
#include "shader_recompiler/frontend/control_flow_graph.h"
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <filesystem>
#include <shared_mutex>
#include <thread>
#include <unordered_map>
#include <vector>
#include "bbport_threads.h"
#include "common/serdes.h"
#include "common/thread.h"
#include "core/emulator_settings.h"
#include "shader_recompiler/frontend/fetch_shader.h"
#include "shader_recompiler/info.h"
#include "shader_recompiler/backend/spirv/emit_spirv.h"
#include "video_core/cache_storage.h"
#include "video_core/renderer_vulkan/vk_instance.h"
#include "video_core/renderer_vulkan/vk_pipeline_cache.h"
#include "video_core/renderer_vulkan/vk_shader_util.h"
#include "bbport_focus_fix.h"
#include "bbport_shader_preload.h"
#include "common/elf_info.h"

namespace Serialization {
/* You should increment versions below once corresponding serialization scheme is changed. */
static constexpr u32 ShaderBinaryVersion = 16u; // bbport: motion shaders hold address stand-ins (10); scalar instructions split EXEC scopes (11); both lines merged, GCN NaN semantics (shadPS4 PR #5257) and WorkgroupSplit (12); both on merging upstream 0.3.11 (13); shared memory bounds pass + compute runtime_info shared_memory_size (14); NaN guard on float ImageWrite (15); page table watch bit (16)
static constexpr u32 ShaderMetaVersion = 9u; // bbport: ImageResource::needs_native; RuntimeInfo::Properties::dx10_clamp (Linux line, 8); InfoPersistent::cs_split_eligible (upstream, 8); both (9)
static constexpr u32 PipelineKeyVersion = 6u; // bbport: Info layout (cs_split_eligible)
/// bbport (BilalReloaded): the cache name of a shader's SPIR-V. Shaders that gained multi-wave
/// LDS barriers get a new name, so their binaries cached before the fix are not found and only
/// those shaders recompile.
/// Shaders with a clamped loop bound (BB_EFFECT_LOOP_CLAMP) get their own names too.
static std::string ShaderBinaryName(u64 pgm_hash, size_t perm_idx) {
    const char* clamp = !Shader::Optimization::EffectLoopClamp(pgm_hash) ? ""
                        : pgm_hash == 0x689d1af9                       ? "_lc2"
                                                                       : "_lc1";
    auto name = fmt::format("{:#018x}_{}{}{}", pgm_hash, perm_idx,
                            Shader::Optimization::MultiwaveLdsBarriers(pgm_hash) ? "_lds4" : "",
                            clamp);
    // Keep the suffix specific to the one known affected translation. With the switch off,
    // the legacy cache name is returned byte-for-byte.
    if (pgm_hash == 0xf8fa9165 && Shader::Gcn::ScopeSplitBarrier()) {
        name += "_bs1";
    }
    return name;
}
} // namespace Serialization

namespace Vulkan {

void RegisterPipelineData(const ComputePipelineKey& key,
                          ComputePipeline::SerializationSupport& sdata) {
    if (!Storage::DataBase::Instance().IsOpened()) {
        return;
    }

    Serialization::Archive ar{};
    Serialization::Writer pldata{ar};

    pldata.Write(Serialization::PipelineKeyVersion);
    pldata.Write(u32{1}); // compute

    key.Serialize(ar);
    sdata.Serialize(ar);

    Storage::DataBase::Instance().Save(Storage::BlobType::PipelineKey,
                                       fmt::format("c_{:#018x}", key.value), ar.TakeOff());
}

void RegisterPipelineData(const GraphicsPipelineKey& key, u64 hash,
                          GraphicsPipeline::SerializationSupport& sdata) {
    if (!Storage::DataBase::Instance().IsOpened()) {
        return;
    }

    Serialization::Archive ar{};
    Serialization::Writer pldata{ar};

    pldata.Write(Serialization::PipelineKeyVersion);
    pldata.Write(u32{0}); // graphics

    key.Serialize(ar);
    sdata.Serialize(ar);

    Storage::DataBase::Instance().Save(Storage::BlobType::PipelineKey,
                                       fmt::format("g_{:#018x}", hash), ar.TakeOff());
}

void RegisterShaderMeta(const Shader::Info& info,
                        const std::optional<Shader::Gcn::FetchShaderData>& fetch_shader_data,
                        const Shader::StageSpecialization& spec, size_t perm_hash,
                        size_t perm_idx) {
    if (!Storage::DataBase::Instance().IsOpened()) {
        return;
    }

    Serialization::Archive ar;
    Serialization::Writer meta{ar};

    meta.Write(Serialization::ShaderMetaVersion);
    meta.Write(Serialization::ShaderBinaryVersion);

    meta.Write(perm_hash);
    meta.Write(perm_idx);

    spec.Serialize(ar);
    info.Serialize(ar);

    Storage::DataBase::Instance().Save(Storage::BlobType::ShaderMeta,
                                       fmt::format("{:#018x}", perm_hash), ar.TakeOff());
}

void RegisterShaderBinary(std::vector<u32>&& spv, u64 pgm_hash, size_t perm_idx) {
    if (!Storage::DataBase::Instance().IsOpened()) {
        return;
    }

    Storage::DataBase::Instance().Save(Storage::BlobType::ShaderBinary,
                                       Serialization::ShaderBinaryName(pgm_hash, perm_idx),
                                       std::move(spv));
}

bool LoadShaderMeta(Serialization::Archive& ar, Shader::Info& info,
                    std::optional<Shader::Gcn::FetchShaderData>& fetch_shader_data,
                    Shader::StageSpecialization& spec, size_t& perm_idx) {
    Serialization::Reader meta{ar};

    u32 meta_version{};
    if (!meta.TryRead(meta_version) || meta_version != Serialization::ShaderMetaVersion) {
        return false;
    }

    u32 binary_version{};
    if (!meta.TryRead(binary_version) || binary_version != Serialization::ShaderBinaryVersion) {
        return false;
    }

    u64 perm_hash_ar{};
    if (!meta.TryRead(perm_hash_ar) || !meta.TryRead(perm_idx)) {
        return false;
    }

    if (!meta.CanRead(sizeof(Shader::StageSpecialization::bitset) +
                      sizeof(Shader::StageSpecialization::runtime_info))) {
        return false;
    }
    spec.Deserialize(ar);
    info.Deserialize(ar);

    fetch_shader_data = spec.fetch_shader_data;
    return true;
}

void ComputePipelineKey::Serialize(Serialization::Archive& ar) const {
    Serialization::Writer key{ar};
    key.Write(value);
}

bool ComputePipelineKey::Deserialize(Serialization::Archive& ar) {
    Serialization::Reader key{ar};
    return key.TryRead(value);
}

void ComputePipeline::SerializationSupport::Serialize(Serialization::Archive& ar) const {
    // Nothing here yet
    return;
}

bool ComputePipeline::SerializationSupport::Deserialize(Serialization::Archive& ar) {
    // Nothing here yet
    return true;
}

void GraphicsPipelineKey::Serialize(Serialization::Archive& ar) const {
    Serialization::Writer key{ar};

    key.Write(this, sizeof(*this));
}

bool GraphicsPipelineKey::Deserialize(Serialization::Archive& ar) {
    Serialization::Reader key{ar};

    return key.TryRead(this, sizeof(*this));
}

void GraphicsPipeline::SerializationSupport::Serialize(Serialization::Archive& ar) const {
    Serialization::Writer sdata{ar};

    sdata.Write(&vertex_attributes, sizeof(vertex_attributes));
    sdata.Write(&vertex_bindings, sizeof(vertex_bindings));
    sdata.Write(&divisors, sizeof(divisors));
    sdata.Write(multisampling);
    sdata.Write(tcs);
    sdata.Write(tes);
}

bool GraphicsPipeline::SerializationSupport::Deserialize(Serialization::Archive& ar) {
    Serialization::Reader sdata{ar};

    return sdata.TryRead(&vertex_attributes, sizeof(vertex_attributes)) &&
           sdata.TryRead(&vertex_bindings, sizeof(vertex_bindings)) &&
           sdata.TryRead(&divisors, sizeof(divisors)) && sdata.TryRead(multisampling) &&
           sdata.TryRead(tcs) && sdata.TryRead(tes);
}

namespace {

/// Runs fn(i) for i in [0, count) on up to `threads` threads (the caller is one of them).
template <typename Fn>
void ParallelFor(size_t count, u32 threads, Fn&& fn) {
    if (count == 0) {
        return;
    }
    threads = static_cast<u32>(std::max<size_t>(1, std::min<size_t>(threads, count)));
    std::atomic<size_t> next{0};
    const auto run = [&] {
        for (size_t i = next.fetch_add(1); i < count; i = next.fetch_add(1)) {
            fn(i);
        }
    };
    std::vector<std::thread> pool;
    for (u32 t = 1; t < threads; ++t) {
        pool.emplace_back([&] {
            BbThreads::MakeBelowNormal();
            run();
        });
    }
    run();
    for (auto& thread : pool) {
        thread.join();
    }
}

/// One stored shader permutation, read from the cache.
struct LoadedStage {
    std::unique_ptr<Program> program; ///< holds the stored Info until it is inserted
    Shader::StageSpecialization spec;
    size_t perm_idx{};
    vk::ShaderModule module{};
    bool parsed = false; ///< metadata read: its permutation slot is known
    // Filled by the serial insertion:
    Program* owner{};
    std::optional<Shader::Gcn::FetchShaderData> fetch_shader{};
    bool ready = false; ///< owner and module are usable
};

/// One stored pipeline key.
struct StoredPipeline {
    bool parsed = false;
    bool compute = false;
    GraphicsPipelineKey gkey{};
    GraphicsPipeline::SerializationSupport gsdata{};
    ComputePipelineKey ckey{};
    ComputePipeline::SerializationSupport csdata{};
    std::unique_ptr<GraphicsPipeline> gpipeline;
    std::unique_ptr<ComputePipeline> cpipeline;
};

} // namespace

void PipelineCache::StartWarmUp() {
    if (!EmulatorSettings.IsPipelineCacheEnabled()) {
        return;
    }
    // BB_WARM_MODE=0: no preload (the pipelines compile as the game needs them).
    if (const char* mode = std::getenv("BB_WARM_MODE"); mode && mode[0] == '0') {
        Storage::DataBase::Instance().FinishPreload();
        warm_done.store(true, std::memory_order_release);
        return;
    }
    Storage::DataBase::Instance().Open();
    warm_done.store(false, std::memory_order_release);
    warm_thread = std::thread([this] { WarmUpThread(); });
}

void PipelineCache::WaitWarm() {
    warm_done.wait(false, std::memory_order_acquire);
}

void PipelineCache::WarmUpThread() {
    Common::SetCurrentThreadName("bb:PipelineWarm");
    BbThreads::MakeBelowNormal();
    auto& db = Storage::DataBase::Instance();
    const auto t0 = std::chrono::steady_clock::now();
    const auto elapsed_ms = [&] {
        return std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0)
            .count();
    };

    // Check if cache is compatible: the shader profile (what the translator was told about the
    // GPU), the GPU/driver identity, game revision, shader versions, and feature flags must match.
    std::vector<u8> profile_data{};
    db.Load(Storage::BlobType::ShaderProfile, "profile", profile_data);
    std::vector<u8> device_data{};
    db.Load(Storage::BlobType::ShaderProfile, "device", device_data);
    std::vector<u8> identity_data{};
    db.Load(Storage::BlobType::ShaderProfile, "identity", identity_data);
    const auto& device_tag = db.DeviceTag();
    const auto& elf_info = Common::ElfInfo::Instance();
    BbShaderPreload::CacheIdentity current_identity{
        .meta_version = Serialization::ShaderMetaVersion,
        .binary_version = Serialization::ShaderBinaryVersion,
        .pipeline_key_version = Serialization::PipelineKeyVersion,
        .vendor_id = instance.GetVendorID(),
        .device_id = instance.GetDeviceID(),
        .driver_id = static_cast<u32>(instance.GetDriverID()),
        .driver_version = instance.GetDriverVersion(),
        .pipeline_cache_uuid = instance.GetPipelineCacheUUID(),
        .feature_flags = BbShaderPreload::CurrentFeatureFlags(),
        .game_serial = std::string{elf_info.GameSerial()},
        .app_ver = std::string{elf_info.AppVer()},
    };
    const auto write_profile = [&] {
        profile_data.resize(sizeof(profile));
        std::memcpy(profile_data.data(), &profile, sizeof(profile));
        db.Save(Storage::BlobType::ShaderProfile, "profile", std::move(profile_data));
        db.Save(Storage::BlobType::ShaderProfile, "device",
                std::vector<u8>(device_tag.begin(), device_tag.end()));
        db.Save(Storage::BlobType::ShaderProfile, "identity", current_identity.Serialize());
    };
    bool compatible = true;
    if (profile_data.empty()) {
        db.FinishPreload();
        write_profile();
        compatible = false;
    } else {
        if (profile_data.size() != sizeof(Shader::Profile)) {
            LOG_WARNING(Render, "Pipeline cache profile has unexpected size ({} != {})",
                        profile_data.size(), sizeof(Shader::Profile));
        }
        Shader::Profile cached_profile{};
        if (profile_data.size() == sizeof(Shader::Profile)) {
            std::memcpy(&cached_profile, profile_data.data(), sizeof(cached_profile));
        }
        // bbport: a cache folder also names its GPU; blobs another GPU wrote can pass the
        // profile comparison and then crash the driver's compiler (Intel igc64 on NVIDIA's).
        const bool same_device = device_data.size() == device_tag.size() &&
                                 std::equal(device_data.begin(), device_data.end(),
                                            device_tag.begin());
        BbShaderPreload::CacheIdentity cached_identity{};
        std::string mismatch_reason;
        const bool identity_ok =
            !identity_data.empty() && cached_identity.Deserialize(std::move(identity_data)) &&
            cached_identity.Matches(current_identity, mismatch_reason);
        if (profile_data.size() != sizeof(Shader::Profile) || cached_profile != profile ||
            !same_device || !identity_ok) {
            // bbport: upstream closed the cache for the session here, so it was never rewritten
            // and every later session compiled every shader again (stutters on each new area).
            // Start a fresh cache for this build and GPU instead.
            LOG_WARNING(Render,
                        "Pipeline cache isn't compatible with current system ({}): rebuilding it",
                        mismatch_reason.empty() ? "profile or device mismatch" : mismatch_reason);
            db.Clear();
            db.FinishPreload();
            write_profile();
            compatible = false;
        }
    }
    if (!compatible) {
        warm_progress.Reset(0);
        warm_progress.done.store(true, std::memory_order_release);
        warm_done.store(true, std::memory_order_release);
        warm_done.notify_all();
        return;
    }

    u32 threads = std::clamp<u32>(BbThreads::Available() > 2 ? BbThreads::Available() - 2 : 1, 1, 16);
    if (const char* env = std::getenv("BB_WARM_THREADS")) { // 1 = the old serial load, for timing
        threads = std::clamp<u32>(static_cast<u32>(std::atoi(env)), 1, 64);
    }
    if (EmulatorSettings.IsPipelineCacheArchived()) {
        threads = 1; // the zip reader is not thread-safe
    }
    const auto device = instance.GetDevice();

    // 1. The stored pipeline keys.
    const auto names = db.ListBlobNames(Storage::BlobType::PipelineKey);
    std::vector<StoredPipeline> stored(names.size());
    ParallelFor(stored.size(), threads, [&](size_t i) {
        std::vector<u8> data;
        db.Load(Storage::BlobType::PipelineKey, names[i], data);
        if (data.empty()) {
            return;
        }
        Serialization::Archive ar{std::move(data)};
        Serialization::Reader pldata{ar};
        u32 version{};
        pldata.Read(version);
        if (version != Serialization::PipelineKeyVersion) {
            // A key of an older cache layout can never load again: drop it.
            std::error_code ec;
            std::filesystem::remove(db.Directory() / (names[i] + ".key"), ec);
            return;
        }
        u32 is_compute{};
        pldata.Read(is_compute);
        auto& entry = stored[i];
        entry.compute = is_compute != 0;
        if (entry.compute) {
            entry.ckey.Deserialize(ar);
            entry.csdata.Deserialize(ar);
        } else {
            entry.gkey.Deserialize(ar);
            entry.gsdata.Deserialize(ar);
        }
        entry.parsed = true;
    });
    const double t_keys = elapsed_ms();

    // 2. The shader permutations those keys name, loaded once each.
    std::vector<u64> stage_hashes;
    for (const auto& entry : stored) {
        if (!entry.parsed) {
            continue;
        }
        if (entry.compute) {
            stage_hashes.push_back(entry.ckey.value);
        } else {
            for (const auto hash : entry.gkey.stage_hashes) {
                if (hash) {
                    stage_hashes.push_back(hash);
                }
            }
        }
    }
    std::ranges::sort(stage_hashes);
    stage_hashes.erase(std::unique(stage_hashes.begin(), stage_hashes.end()), stage_hashes.end());
    std::vector<LoadedStage> stages(stage_hashes.size());
    ParallelFor(stages.size(), threads, [&](size_t i) {
        auto& stage = stages[i];
        std::vector<u8> meta_blob;
        db.Load(Storage::BlobType::ShaderMeta, fmt::format("{:#018x}", stage_hashes[i]),
                meta_blob);
        if (meta_blob.empty()) {
            return;
        }
        Serialization::Archive meta_ar{std::move(meta_blob)};
        stage.program = std::make_unique<Program>();
        stage.spec.info = &stage.program->info;
        std::optional<Shader::Gcn::FetchShaderData> unused;
        if (!LoadShaderMeta(meta_ar, stage.program->info, unused, stage.spec, stage.perm_idx) ||
            Shader::Backend::SPIRV::IsForcedShader(stage.program->info.pgm_hash) ||
            BbFocusFix::FocusCentreShader(stage.program->info.pgm_hash)) {
            stage.program.reset();
            return;
        }
        stage.parsed = true;
        // Motion shaders need the session's buffer addresses: without them the slot is only
        // reserved, so a later permutation cannot take its index.
        const auto& info = stage.program->info;
        if (info.hw_stage == Shader::HwStage::Vertex &&
            stage.spec.runtime_info.hw.vs.motion_vectors &&
            Shader::MotionVectors::positions_address == 0) {
            return;
        }
        std::vector<u32> spv{};
        db.Load(Storage::BlobType::ShaderBinary,
                Serialization::ShaderBinaryName(info.pgm_hash, stage.perm_idx), spv);
        if (spv.empty()) {
            return;
        }
        stage.module = CompileSPV(spv, device);
    });
    const double t_shaders = elapsed_ms();
    // Everything is read: an archived cache turns writable now (the game saves what it compiles).
    db.FinishPreload();

    // 3. Into the program cache, in order. A permutation lives at its original index.
    std::unordered_map<u64, LoadedStage*> by_hash;
    std::vector<Program*> owners;
    for (size_t i = 0; i < stages.size(); ++i) {
        auto& stage = stages[i];
        if (!stage.parsed) {
            continue;
        }
        by_hash.emplace(stage_hashes[i], &stage);
        const u64 pgm_hash = stage.program->info.pgm_hash;
        const auto [it_pgm, new_program] = program_cache.try_emplace(pgm_hash);
        if (new_program) {
            it_pgm.value() = std::move(stage.program);
            owners.push_back(it_pgm.value().get());
        }
        Program* program = it_pgm.value().get();
        stage.owner = program;
        stage.spec.info = &program->info;
        stage.fetch_shader = stage.spec.fetch_shader_data;
        program->modules.resize(std::max(program->modules.size(), stage.perm_idx + 1));
        if (!stage.module) {
            continue; // slot reserved
        }
        if (!new_program) {
            const auto found =
                std::ranges::find(program->modules, stage.spec, &Program::Module::spec);
            if (found != program->modules.end() && found->module) {
                // The same permutation under another index: a store holding more than one
                // cache generation. Leave this pipeline to compile at runtime.
                device.destroyShaderModule(stage.module);
                stage.module = vk::ShaderModule{};
                LOG_WARNING(Render_Vulkan,
                            "Cached permutation {} of {:x} conflicts with index {}, skipping "
                            "preload",
                            stage.perm_idx, pgm_hash,
                            std::distance(program->modules.begin(), found));
                continue;
            }
        }
        program->modules[stage.perm_idx] = {stage.module, stage.spec};
        stage.ready = true;
    }
    const double t_programs = elapsed_ms();

    // 4. The programs are final: selection may start. (A permutation the game needs that is
    // not stored is appended behind the stored ones, as the reserved slots promise.)
    std::array<Shader::RuntimeInfo, MaxShaderStages> no_runtime_infos{};
    ParallelFor(owners.size(), threads, [&](size_t i) {
        owners[i]->info_template = std::make_unique<Shader::Info>(owners[i]->info);
    });
    warm_done.store(true, std::memory_order_release);
    warm_done.notify_all();
    const double t_ready = elapsed_ms();

    // 5. The pipelines themselves, on all threads, while the game already runs: each goes into
    // the map as it is done, unless the game built the same one first. They are built from
    // the programs' template Infos (the game refreshes the programs' own per draw) and then
    // pointed at the programs' Infos.
    std::atomic<u32> num_pipelines{0};
    ParallelFor(stored.size(), threads, [&](size_t i) {
        if (warm_progress.IsCancelled()) {
            return;
        }
        auto& entry = stored[i];
        if (!entry.parsed) {
            warm_progress.completed_entries.fetch_add(1, std::memory_order_relaxed);
            return;
        }
        if (entry.compute) {
            const auto it = by_hash.find(entry.ckey.value);
            if (it == by_hash.end() || !it->second->ready) {
                warm_progress.skipped_entries.fetch_add(1, std::memory_order_relaxed);
                warm_progress.completed_entries.fetch_add(1, std::memory_order_relaxed);
                return;
            }
            {
                std::shared_lock lock{pipelines_mutex};
                if (compute_pipelines.contains(entry.ckey)) {
                    warm_progress.completed_entries.fetch_add(1, std::memory_order_relaxed);
                    return;
                }
            }
            auto pipeline = std::make_unique<ComputePipeline>(
                instance, scheduler, desc_heap, profile, *pipeline_cache, entry.ckey,
                *it->second->owner->info_template, it->second->module, entry.csdata, true);
            // bbport: a cached pipeline the driver cannot create (a new driver, or one saved by
            // a build that stopped on it) is compiled again when the game uses it.
            if (!pipeline->IsValid()) {
                warm_progress.failed_entries.fetch_add(1, std::memory_order_relaxed);
                warm_progress.completed_entries.fetch_add(1, std::memory_order_relaxed);
                return;
            }
            pipeline->RebindStage(&it->second->owner->info);
            std::unique_lock lock{pipelines_mutex};
            if (compute_pipelines.try_emplace(entry.ckey, std::move(pipeline)).second) {
                num_pipelines.fetch_add(1, std::memory_order_relaxed);
                warm_progress.warmed_pipelines.fetch_add(1, std::memory_order_relaxed);
            }
            warm_progress.completed_entries.fetch_add(1, std::memory_order_relaxed);
            return;
        }
        std::array<const Shader::Info*, MaxShaderStages> infos{};
        std::array<const Shader::Info*, MaxShaderStages> program_infos{};
        std::array<vk::ShaderModule, MaxShaderStages> modules{};
        std::optional<Shader::Gcn::FetchShaderData> fetch_shader;
        for (u32 stage = 0; stage < MaxShaderStages; ++stage) {
            const auto hash = entry.gkey.stage_hashes[stage];
            if (!hash) {
                continue;
            }
            const auto it = by_hash.find(hash);
            if (it == by_hash.end() || !it->second->ready) {
                return;
            }
            // bbport: a motion-vector key needs its stages stored with motion output. The shader
            // metadata is stored once per program, not per permutation, so a motion key can
            // match plain shaders; those are left to be built at first use.
            if (entry.gkey.motion_vectors) {
                const auto hw_stage = it->second->owner->info.hw_stage;
                const auto& stage_rt = it->second->spec.runtime_info;
                if (hw_stage == Shader::HwStage::Vertex && !stage_rt.hw.vs.motion_vectors) {
                    return;
                }
                if (hw_stage == Shader::HwStage::Fragment && !stage_rt.hw.fs.motion_vectors) {
                    return;
                }
            }
            infos[stage] = it->second->owner->info_template.get();
            program_infos[stage] = &it->second->owner->info;
            modules[stage] = it->second->module;
            if (it->second->fetch_shader) {
                fetch_shader = it->second->fetch_shader;
            }
        }
        // The game may need (and build) the same pipeline meanwhile: build it only once.
        const u64 claim_id = std::hash<GraphicsPipelineKey>{}(entry.gkey) ^ PipelineClaimTag;
        AcquireClaim(claim_id);
        {
            std::shared_lock lock{pipelines_mutex};
            if (graphics_pipelines.contains(entry.gkey)) {
                ReleaseClaim(claim_id);
                return;
            }
        }
        auto pipeline = std::make_unique<GraphicsPipeline>(
            instance, scheduler, desc_heap, profile, entry.gkey, *pipeline_cache, infos,
            no_runtime_infos, fetch_shader, modules, entry.gsdata, true);
        if (!pipeline->IsValid()) { // bbport: as for compute pipelines
            ReleaseClaim(claim_id);
            warm_progress.failed_entries.fetch_add(1, std::memory_order_relaxed);
            warm_progress.completed_entries.fetch_add(1, std::memory_order_relaxed);
            return;
        }
        pipeline->RebindStages(program_infos);
        {
            std::unique_lock lock{pipelines_mutex};
            if (graphics_pipelines.try_emplace(entry.gkey, std::move(pipeline)).second) {
                num_pipelines.fetch_add(1, std::memory_order_relaxed);
                warm_progress.warmed_pipelines.fetch_add(1, std::memory_order_relaxed);
            }
        }
        ReleaseClaim(claim_id);
        warm_progress.completed_entries.fetch_add(1, std::memory_order_relaxed);
    });
    warm_progress.done.store(true, std::memory_order_release);
    const u32 num_total_pipelines = static_cast<u32>(stored.size());
    LOG_INFO(Render, "Preloaded {} pipelines", num_pipelines.load());
    if (num_total_pipelines > num_pipelines.load()) {
        LOG_WARNING(Render, "{} stale pipelines were found. Consider re-generating the cache",
                    num_total_pipelines - num_pipelines.load());
    }
    std::printf("Pipeline cache: preloaded %u of %u pipelines (%u shaders, %u threads) in %.0f ms "
                "(keys %.0f, shaders %.0f, programs %.0f, pipelines %.0f); the game could select from %.0f ms\n",
                num_pipelines.load(), num_total_pipelines, u32(stages.size()), threads, elapsed_ms(),
                t_keys, t_shaders - t_keys, t_programs - t_shaders, elapsed_ms() - t_programs,
                t_ready);
    // The driver's cache gets the entries this preload had to compile.
    if (driver_cache_loaded_size != DriverCacheSize()) {
        driver_cache_dirty.fetch_add(1);
    }
}

void PipelineCache::Sync() {
    Storage::DataBase::Instance().Close();
}
} // namespace Vulkan

namespace Shader {

void Info::Serialize(Serialization::Archive& ar) const {
    Serialization::Writer info{ar};

    info.Write(this, sizeof(InfoPersistent));
    info.Write(flattened_ud_buf);
    srt_info.Serialize(ar);
}

bool Info::Deserialize(Serialization::Archive& ar) {
    Serialization::Reader info{ar};

    info.Read(this, sizeof(Shader::InfoPersistent));
    info.Read(flattened_ud_buf);

    return srt_info.Deserialize(ar);
}

void Gcn::FetchShaderData::Serialize(Serialization::Archive& ar) const {
    Serialization::Writer fetch{ar};
    ar.Grow(6 + attributes.size() * sizeof(VertexAttribute));

    fetch.Write(size);
    fetch.Write(vertex_offset_sgpr);
    fetch.Write(instance_offset_sgpr);
    fetch.Write(attributes);
}

bool Gcn::FetchShaderData::Deserialize(Serialization::Archive& ar) {
    Serialization::Reader fetch{ar};

    fetch.Read(size);
    fetch.Read(vertex_offset_sgpr);
    fetch.Read(instance_offset_sgpr);
    fetch.Read(attributes);

    return true;
}

void PersistentSrtInfo::Serialize(Serialization::Archive& ar) const {
    Serialization::Writer srt{ar};

    srt.Write(this, sizeof(*this));
    if (walker_func_size) {
        srt.Write(reinterpret_cast<void*>(walker_func), walker_func_size);
    }
}

bool PersistentSrtInfo::Deserialize(Serialization::Archive& ar) {
    Serialization::Reader srt{ar};

    srt.Read(this, sizeof(*this));

    if (walker_func_size) {
        walker_func = RegisterWalkerCode(ar.CurrPtr(), walker_func_size);
        ar.Advance(walker_func_size);
    }

    return true;
}

void StageSpecialization::Serialize(Serialization::Archive& ar) const {
    Serialization::Writer spec{ar};

    spec.Write(start);
    spec.Write(runtime_info);

    spec.Write(bitset.to_string());

    if (fetch_shader_data) {
        spec.Write(sizeof(*fetch_shader_data));
        fetch_shader_data->Serialize(ar);
    } else {
        spec.Write(size_t{0});
    }

    spec.Write(vs_attribs);
    spec.Write(buffers);
    spec.Write(images);
    spec.Write(fmasks);
    spec.Write(samplers);
}

bool StageSpecialization::Deserialize(Serialization::Archive& ar) {
    Serialization::Reader spec{ar};

    spec.Read(start);
    spec.Read(runtime_info);

    std::string bits{};
    spec.Read(bits);
    bitset = std::bitset<MaxStageResources>(bits);

    u64 fetch_data_size{};
    spec.Read(fetch_data_size);

    if (fetch_data_size) {
        Gcn::FetchShaderData fetch_data;
        fetch_data.Deserialize(ar);
        fetch_shader_data = fetch_data;
    }

    spec.Read(vs_attribs);
    spec.Read(buffers);
    spec.Read(images);
    spec.Read(fmasks);
    spec.Read(samplers);

    return true;
}

} // namespace Shader
