// SPDX-License-Identifier: GPL-2.0-only
// Android C-ABI bridge for iannitram/save-relay (save-relay#49, save-relay#254, issue #250).
// Handles initialization, pull-before-play, push-after-exit, offline/local-only fallback,
// slot-aware manifest and save directory paths, and side-by-side conflict surfacing.

#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Matches save-relay/include/save_relay.h (save-relay#49 / #254).
#ifndef SAVE_RELAY_H
typedef struct SaveRelayHttpSink SaveRelayHttpSink;

typedef struct SaveRelayHttpRequest {
    const char* method;
    const char* url;
    const char* headers;
    const uint8_t* body;
    size_t body_len;
    size_t max_response_bytes;
} SaveRelayHttpRequest;

typedef struct SaveRelayHost {
    void* user_data;
    const char* home;
    const char* data;
    const char* config;
    const char* documents;
    int (*cred_get)(void* user_data, const char* account, uint8_t* buf, size_t cap, size_t* len);
    int (*cred_set)(void* user_data, const char* account, const char* secret);
    int (*cred_delete)(void* user_data, const char* account);
    int (*http)(void* user_data, const SaveRelayHttpRequest* request, SaveRelayHttpSink* sink);
} SaveRelayHost;
#endif // SAVE_RELAY_H

/// Injectable save-relay C-ABI function table for host unit testing and decoupled loading.
/// Pass NULL on Android (when compiled with BB_HAVE_SAVE_RELAY) to invoke the linked
/// save-relay library symbols directly.
typedef struct BbSaveRelayCAbi {
    char* (*init)(const SaveRelayHost* host);
    char* (*pull)(const char* manifest_json, const char* saves_dir, const char* resolve);
    char* (*push)(const char* manifest_json, const char* saves_dir, const char* resolve);
    char* (*status)(const char* manifest_json, const char* saves_dir);
    char* (*setup)(const char* server, const char* user, const char* password, const char* device,
                   const char* root);
    char* (*sign_in_start)(const char* service, const char* redirect_uri, const char* device,
                           const char* root);
    char* (*sign_in_finish)(const char* redirect_url);
    void (*free_str)(char* pointer);
    int (*http_respond)(SaveRelayHttpSink* sink, uint16_t status, const char* headers,
                        const uint8_t* body, size_t body_len);
    char* (*download_game_files)(const char* manifest_json, const char* dest_dir);
    char* (*game_files_status)(const char* manifest_json, const char* dest_dir);
} BbSaveRelayCAbi;

typedef enum BbAndroidSaveGame {
    BB_ANDROID_SAVE_GAME_U1 = 1,
    BB_ANDROID_SAVE_GAME_U2 = 2,
    BB_ANDROID_SAVE_GAME_U3 = 3,
} BbAndroidSaveGame;

#define BB_ANDROID_SAVE_SLOT_MIN 1u
#define BB_ANDROID_SAVE_SLOT_MAX 10u

typedef enum BbSaveRelayResolve {
    BB_SAVE_RELAY_RESOLVE_NONE = 0,
    BB_SAVE_RELAY_RESOLVE_KEEP_SERVER = 1,
    BB_SAVE_RELAY_RESOLVE_KEEP_LOCAL = 2,
} BbSaveRelayResolve;

typedef enum BbSaveRelayOutcome {
    BB_SAVE_RELAY_OUTCOME_UNAVAILABLE = -2,
    BB_SAVE_RELAY_OUTCOME_ERROR = -1,
    BB_SAVE_RELAY_OUTCOME_DONE = 0,
    BB_SAVE_RELAY_OUTCOME_OFFLINE = 1,
    BB_SAVE_RELAY_OUTCOME_LOCAL_ONLY = 2,
    BB_SAVE_RELAY_OUTCOME_CONFLICT = 3,
    BB_SAVE_RELAY_OUTCOME_HELD = 4,
} BbSaveRelayOutcome;

typedef struct BbSaveRelaySideCard {
    int present;
    char device[129];
    char time[64];
    uint64_t revision;
    int has_revision;
    char place[256];
    uint64_t playtime_secs;
    int has_playtime;
} BbSaveRelaySideCard;

typedef struct BbSaveRelayHeldInfo {
    int present;
    char device[129];
    char since[64];
} BbSaveRelayHeldInfo;

typedef struct BbSaveRelaySyncRequest {
    uint32_t game;
    uint32_t slot;
    const char* user_dir;
    const char* saves_dir_override;
    const char* manifest_path;
    const char* manifest_json_override;
    uint32_t resolve;
} BbSaveRelaySyncRequest;

typedef struct BbSaveRelaySyncResult {
    int32_t outcome;
    char action[32];
    char game_id[80];
    uint64_t revision;
    int has_revision;
    int server_is_newer;
    BbSaveRelaySideCard server;
    BbSaveRelaySideCard local;
    BbSaveRelayHeldInfo held;
    char error_code[64];
    char message[512];
} BbSaveRelaySyncResult;

typedef struct BbSaveRelaySignInChallenge {
    char service[32];
    char verification_url[1024];
    char user_code[64];
    char message[512];
} BbSaveRelaySignInChallenge;

typedef struct BbSaveRelayGameFilesRequest {
    uint32_t game;
    const char* dest_dir;
    const char* manifest_path;
    const char* manifest_json_override;
} BbSaveRelayGameFilesRequest;

typedef struct BbSaveRelayGameFilesResult {
    int32_t outcome;
    char game_id[80];
    uint64_t total_files;
    uint64_t completed_files;
    uint64_t downloaded_files;
    uint64_t skipped_files;
    uint64_t partial_files;
    uint64_t bytes_on_disk;
    uint64_t bytes_downloaded;
    uint64_t total_bytes;
    int complete;
    int reauth_needed;
    char error_code[64];
    char message[512];
} BbSaveRelayGameFilesResult;

/// Writes the canonical game id ("uncharted-1", "uncharted-2", "uncharted-3") for `game`.
/// Returns 0 on success, < 0 on invalid game or small buffer.
int bb_android_save_relay_game_id(uint32_t game, char* out_id, size_t out_cap);

/// Computes the slot-aware saves directory matching crates/core/src/saves.rs:
/// - slot 1: "<user_dir>/Uncharted N"
/// - slot 2..=10: "<user_dir>/Save slots/Uncharted N/Slot <slot>"
/// Returns 0 on success, < 0 on invalid arguments.
int bb_android_save_relay_slot_saves_dir(const char* user_dir, uint32_t game, uint32_t slot,
                                         char* out_dir, size_t out_cap);

/// Computes the slot-aware manifest file path under `manifests_dir`:
/// - slot 0: "<manifests_dir>/uncharted-<n>.json" (base manifest)
/// - slot 1..=10: "<manifests_dir>/uncharted-<n>-slot-<slot>.json"
/// Returns 0 on success, < 0 on invalid arguments.
int bb_android_save_relay_manifest_path(const char* manifests_dir, uint32_t game, uint32_t slot,
                                        char* out_path, size_t out_cap);

/// Generates the format-1 save-relay manifest JSON for `game` and `slot` (1..=10), with
/// `id` set to "uncharted-<n>-slot-<slot>" and `saves_from: "launcher"`.
/// Returns 0 on success, < 0 on invalid arguments.
int bb_android_save_relay_manifest_json(uint32_t game, uint32_t slot, char* out_json,
                                        size_t out_cap);

/// Rewrites an existing format-1 manifest JSON so its `id` targets `slot` ("<base>-slot-<slot>"),
/// stripping any existing `-slot-<k>` suffix before appending the requested slot.
/// Returns 0 on success, < 0 on invalid JSON or arguments.
int bb_android_save_relay_rewrite_manifest_for_slot(const char* base_manifest_json, uint32_t slot,
                                                    char* out_json, size_t out_cap);

/// Initializes save-relay with `host` callbacks and folders. Returns 0 on success, < 0 on failure.
int bb_android_save_relay_init(const SaveRelayHost* host, const BbSaveRelayCAbi* abi, char* err_buf,
                               size_t err_cap);

/// Pulls saves before starting a game.
/// Returns >= 0 for handled outcomes (DONE=0, OFFLINE=1, LOCAL_ONLY=2, CONFLICT=3, HELD=4),
/// or < 0 on error/unavailability; details are always written to `out_result`.
int bb_android_save_relay_pull_before_play(const BbSaveRelaySyncRequest* req,
                                           const BbSaveRelayCAbi* abi,
                                           BbSaveRelaySyncResult* out_result);

/// Pushes saves after a game exits.
/// Returns >= 0 for handled outcomes (DONE=0, OFFLINE=1, LOCAL_ONLY=2, CONFLICT=3),
/// or < 0 on error/unavailability; details are always written to `out_result`.
int bb_android_save_relay_push_after_exit(const BbSaveRelaySyncRequest* req,
                                          const BbSaveRelayCAbi* abi,
                                          BbSaveRelaySyncResult* out_result);

/// Queries save-relay status without modifying local or remote saves.
int bb_android_save_relay_status(const BbSaveRelaySyncRequest* req, const BbSaveRelayCAbi* abi,
                                 BbSaveRelaySyncResult* out_result);

/// Configures Nextcloud credentials via save_relay_setup (passwords are never logged or echoed).
int bb_android_save_relay_setup_nextcloud(const char* server, const char* user,
                                          const char* password, const char* device,
                                          const char* root, const BbSaveRelayCAbi* abi,
                                          char* err_buf, size_t err_cap);

/// Starts browser PKCE OAuth sign-in ("gdrive" or "dropbox", save-relay#254).
int bb_android_save_relay_sign_in_start(const char* service, const char* redirect_uri,
                                        const char* device, const char* root,
                                        const BbSaveRelayCAbi* abi,
                                        BbSaveRelaySignInChallenge* out_challenge, char* err_buf,
                                        size_t err_cap);

/// Finishes browser PKCE OAuth sign-in using the redirect URL returned by the browser session.
int bb_android_save_relay_sign_in_finish(const char* redirect_url, const BbSaveRelayCAbi* abi,
                                         char* err_buf, size_t err_cap);

/// Downloads per-game asset files resumably through save-relay (Dropbox App folder, save-relay#305).
/// Returns >= 0 for handled outcomes (DONE=0, OFFLINE=1, LOCAL_ONLY=2), or < 0 on error/unavailable.
int bb_android_save_relay_download_game_files(const BbSaveRelayGameFilesRequest* req,
                                              const BbSaveRelayCAbi* abi,
                                              BbSaveRelayGameFilesResult* out_result);

/// Inspects local completion and `.part` resume progress for per-game assets without network I/O.
/// Returns >= 0 on success (DONE=0) or < 0 on error/unavailable.
int bb_android_save_relay_game_files_status(const BbSaveRelayGameFilesRequest* req,
                                            const BbSaveRelayCAbi* abi,
                                            BbSaveRelayGameFilesResult* out_result);

#ifdef __cplusplus
} // extern "C"

#include <string>

namespace bb::android::saverelay {

inline int Init(const SaveRelayHost& host, const BbSaveRelayCAbi* abi, std::string& err) {
    char buf[512] = {};
    const int rc = bb_android_save_relay_init(&host, abi, buf, sizeof(buf));
    err = buf;
    return rc;
}

inline int PullBeforePlay(const BbSaveRelaySyncRequest& req, const BbSaveRelayCAbi* abi,
                          BbSaveRelaySyncResult& out) {
    return bb_android_save_relay_pull_before_play(&req, abi, &out);
}

inline int PushAfterExit(const BbSaveRelaySyncRequest& req, const BbSaveRelayCAbi* abi,
                         BbSaveRelaySyncResult& out) {
    return bb_android_save_relay_push_after_exit(&req, abi, &out);
}

inline int Status(const BbSaveRelaySyncRequest& req, const BbSaveRelayCAbi* abi,
                  BbSaveRelaySyncResult& out) {
    return bb_android_save_relay_status(&req, abi, &out);
}

inline int DownloadGameFiles(const BbSaveRelayGameFilesRequest& req, const BbSaveRelayCAbi* abi,
                             BbSaveRelayGameFilesResult& out) {
    return bb_android_save_relay_download_game_files(&req, abi, &out);
}

inline int GameFilesStatus(const BbSaveRelayGameFilesRequest& req, const BbSaveRelayCAbi* abi,
                           BbSaveRelayGameFilesResult& out) {
    return bb_android_save_relay_game_files_status(&req, abi, &out);
}

} // namespace bb::android::saverelay
#endif
