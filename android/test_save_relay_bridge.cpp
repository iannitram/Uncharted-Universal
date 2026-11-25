// SPDX-License-Identifier: GPL-2.0-only
#include "save_relay_bridge.h"

#include <cassert>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>

namespace {

struct MockRelayState {
    int alloc_count = 0;
    int free_count = 0;
    int init_calls = 0;
    int pull_calls = 0;
    int push_calls = 0;
    int status_calls = 0;
    int setup_calls = 0;
    int sign_in_start_calls = 0;
    int sign_in_finish_calls = 0;
    int download_game_files_calls = 0;
    int game_files_status_calls = 0;

    std::string last_manifest;
    std::string last_saves_dir;
    std::string last_dest_dir;
    std::string last_resolve;
    bool had_resolve = false;
    std::string last_setup_server;
    std::string last_setup_user;
    std::string last_setup_password;

    std::string next_init_json = R"({"ok":true,"command":"init","data":{}})";
    std::string next_pull_json;
    std::string next_push_json;
    std::string next_status_json;
    std::string next_setup_json = R"({"ok":true,"command":"setup","data":{}})";
    std::string next_sign_in_start_json;
    std::string next_sign_in_finish_json = R"({"ok":true,"command":"setup","data":{}})";
    std::string next_download_game_files_json;
    std::string next_game_files_status_json;
};

MockRelayState g_mock;

char* DupResponse(const std::string& s) {
    ++g_mock.alloc_count;
    char* p = static_cast<char*>(std::malloc(s.size() + 1));
    assert(p != nullptr);
    std::memcpy(p, s.data(), s.size());
    p[s.size()] = '\0';
    return p;
}

void MockFree(char* p) {
    if (p) {
        ++g_mock.free_count;
        std::free(p);
    }
}

char* MockInit(const SaveRelayHost* host) {
    assert(host != nullptr);
    ++g_mock.init_calls;
    return DupResponse(g_mock.next_init_json);
}

char* MockPull(const char* manifest_json, const char* saves_dir, const char* resolve) {
    ++g_mock.pull_calls;
    g_mock.last_manifest = manifest_json ? manifest_json : "";
    g_mock.last_saves_dir = saves_dir ? saves_dir : "";
    g_mock.had_resolve = (resolve != nullptr);
    g_mock.last_resolve = resolve ? resolve : "";
    return DupResponse(g_mock.next_pull_json);
}

char* MockPush(const char* manifest_json, const char* saves_dir, const char* resolve) {
    ++g_mock.push_calls;
    g_mock.last_manifest = manifest_json ? manifest_json : "";
    g_mock.last_saves_dir = saves_dir ? saves_dir : "";
    g_mock.had_resolve = (resolve != nullptr);
    g_mock.last_resolve = resolve ? resolve : "";
    return DupResponse(g_mock.next_push_json);
}

char* MockStatus(const char* manifest_json, const char* saves_dir) {
    ++g_mock.status_calls;
    g_mock.last_manifest = manifest_json ? manifest_json : "";
    g_mock.last_saves_dir = saves_dir ? saves_dir : "";
    return DupResponse(g_mock.next_status_json);
}

char* MockSetup(const char* server, const char* user, const char* password, const char*,
                const char*) {
    ++g_mock.setup_calls;
    g_mock.last_setup_server = server ? server : "";
    g_mock.last_setup_user = user ? user : "";
    g_mock.last_setup_password = password ? password : "";
    return DupResponse(g_mock.next_setup_json);
}

char* MockSignInStart(const char*, const char*, const char*, const char*) {
    ++g_mock.sign_in_start_calls;
    return DupResponse(g_mock.next_sign_in_start_json);
}

char* MockSignInFinish(const char*) {
    ++g_mock.sign_in_finish_calls;
    return DupResponse(g_mock.next_sign_in_finish_json);
}

char* MockDownloadGameFiles(const char* manifest_json, const char* dest_dir) {
    ++g_mock.download_game_files_calls;
    g_mock.last_manifest = manifest_json ? manifest_json : "";
    g_mock.last_dest_dir = dest_dir ? dest_dir : "";
    return DupResponse(g_mock.next_download_game_files_json);
}

char* MockGameFilesStatus(const char* manifest_json, const char* dest_dir) {
    ++g_mock.game_files_status_calls;
    g_mock.last_manifest = manifest_json ? manifest_json : "";
    g_mock.last_dest_dir = dest_dir ? dest_dir : "";
    return DupResponse(g_mock.next_game_files_status_json);
}

const BbSaveRelayCAbi kMockAbi = {
    MockInit,
    MockPull,
    MockPush,
    MockStatus,
    MockSetup,
    MockSignInStart,
    MockSignInFinish,
    MockFree,
    nullptr,
    MockDownloadGameFiles,
    MockGameFilesStatus,
};

int DummyCredGet(void*, const char*, uint8_t*, size_t, size_t*) {
    return 1;
}
int DummyCredSet(void*, const char*, const char*) {
    return 0;
}
int DummyCredDelete(void*, const char*) {
    return 0;
}
int DummyHttp(void*, const SaveRelayHttpRequest*, SaveRelayHttpSink*) {
    return 1;
}

SaveRelayHost ValidHost() {
    SaveRelayHost h{};
    h.home = "/data/user/0/org.uncharted/files/home";
    h.data = "/data/user/0/org.uncharted/files/data";
    h.config = "/data/user/0/org.uncharted/files/config";
    h.documents = nullptr;
    h.cred_get = DummyCredGet;
    h.cred_set = DummyCredSet;
    h.cred_delete = DummyCredDelete;
    h.http = DummyHttp;
    return h;
}

void TestSlotPathsAndManifests() {
    char buf[1024] = {};

    assert(bb_android_save_relay_game_id(BB_ANDROID_SAVE_GAME_U1, buf, sizeof(buf)) == 0);
    assert(std::strcmp(buf, "uncharted-1") == 0);
    assert(bb_android_save_relay_game_id(BB_ANDROID_SAVE_GAME_U2, buf, sizeof(buf)) == 0);
    assert(std::strcmp(buf, "uncharted-2") == 0);
    assert(bb_android_save_relay_game_id(BB_ANDROID_SAVE_GAME_U3, buf, sizeof(buf)) == 0);
    assert(std::strcmp(buf, "uncharted-3") == 0);
    assert(bb_android_save_relay_game_id(99, buf, sizeof(buf)) < 0);

    // Slot 1 uses "<user_dir>/Uncharted N"; slots 2..=10 use "<user_dir>/Save slots/Uncharted N/Slot <n>".
    assert(bb_android_save_relay_slot_saves_dir(
               "/data/user/0/org.uncharted/files/user/", BB_ANDROID_SAVE_GAME_U1, 1, buf,
               sizeof(buf)) == 0);
    assert(std::strcmp(buf, "/data/user/0/org.uncharted/files/user/Uncharted 1") == 0);

    assert(bb_android_save_relay_slot_saves_dir(
               "/data/user/0/org.uncharted/files/user", BB_ANDROID_SAVE_GAME_U3, 7, buf,
               sizeof(buf)) == 0);
    assert(std::strcmp(buf,
                       "/data/user/0/org.uncharted/files/user/Save slots/Uncharted 3/Slot 7") == 0);

    // Reject invalid slots and traversal.
    assert(bb_android_save_relay_slot_saves_dir("/user", BB_ANDROID_SAVE_GAME_U1, 0, buf,
                                                sizeof(buf)) < 0);
    assert(bb_android_save_relay_slot_saves_dir("/user", BB_ANDROID_SAVE_GAME_U1, 11, buf,
                                                sizeof(buf)) < 0);
    assert(bb_android_save_relay_slot_saves_dir("/user/../escape", BB_ANDROID_SAVE_GAME_U1, 1, buf,
                                                sizeof(buf)) < 0);

    // Slot-aware manifest paths.
    assert(bb_android_save_relay_manifest_path("/app/manifests/", BB_ANDROID_SAVE_GAME_U2, 0, buf,
                                               sizeof(buf)) == 0);
    assert(std::strcmp(buf, "/app/manifests/uncharted-2.json") == 0);

    assert(bb_android_save_relay_manifest_path("/app/manifests", BB_ANDROID_SAVE_GAME_U2, 4, buf,
                                               sizeof(buf)) == 0);
    assert(std::strcmp(buf, "/app/manifests/uncharted-2-slot-4.json") == 0);

    assert(bb_android_save_relay_manifest_path("/app/manifests", BB_ANDROID_SAVE_GAME_U2, 11, buf,
                                               sizeof(buf)) < 0);

    // Canonical manifest JSON and slot rewrite.
    assert(bb_android_save_relay_manifest_json(BB_ANDROID_SAVE_GAME_U2, 3, buf, sizeof(buf)) == 0);
    assert(std::strstr(buf, "\"id\":\"uncharted-2-slot-3\"") != nullptr);
    assert(std::strstr(buf, "\"saves_from\":\"launcher\"") != nullptr);
    assert(std::strstr(buf, "\"SAVEDATA02*/**\"") != nullptr);

    const char* base =
        R"({"format":1,"id":"uncharted-1","name":"Uncharted: Drake's Fortune","saves":{},"saves_from":"launcher","include":["SAVEDATA01*/**"],"exclude":["**/*.tmp"]})";
    assert(bb_android_save_relay_rewrite_manifest_for_slot(base, 5, buf, sizeof(buf)) == 0);
    assert(std::strstr(buf, "\"id\":\"uncharted-1-slot-5\"") != nullptr);

    // Re-targeting an already slot-tagged manifest replaces the old slot cleanly.
    char buf2[1024] = {};
    assert(bb_android_save_relay_rewrite_manifest_for_slot(buf, 9, buf2, sizeof(buf2)) == 0);
    assert(std::strstr(buf2, "\"id\":\"uncharted-1-slot-9\"") != nullptr);
    assert(std::strstr(buf2, "-slot-5") == nullptr);
}

void TestInitAndUnavailable() {
    g_mock = MockRelayState{};
    char err[256] = {};
    SaveRelayHost host = ValidHost();

    // Missing abi on non-linked host fails closed with UNAVAILABLE.
    assert(bb_android_save_relay_init(&host, nullptr, err, sizeof(err)) ==
           BB_SAVE_RELAY_OUTCOME_UNAVAILABLE);

    // Missing folders or callbacks fail before invoking C ABI.
    SaveRelayHost bad_host = host;
    bad_host.http = nullptr;
    assert(bb_android_save_relay_init(&bad_host, &kMockAbi, err, sizeof(err)) < 0);
    assert(g_mock.init_calls == 0);

    bad_host = host;
    bad_host.config = "";
    assert(bb_android_save_relay_init(&bad_host, &kMockAbi, err, sizeof(err)) < 0);
    assert(g_mock.init_calls == 0);

    // Valid init succeeds and frees returned JSON string.
    std::string cpp_err;
    assert(bb::android::saverelay::Init(host, &kMockAbi, cpp_err) == 0);
    assert(cpp_err.empty());
    assert(g_mock.init_calls == 1);
    assert(g_mock.alloc_count == g_mock.free_count);
}

void TestPullBeforePlayAndPushAfterExit() {
    g_mock = MockRelayState{};
    const auto tmp_dir = std::filesystem::temp_directory_path() / "uu_test_save_relay_bridge";
    std::filesystem::create_directories(tmp_dir);
    const auto manifest_file = tmp_dir / "uncharted-2.json";
    {
        std::ofstream out(manifest_file);
        out << R"({"format":1,"id":"uncharted-2","name":"Uncharted 2","saves":{},"saves_from":"launcher","include":["SAVEDATA02*/**"],"exclude":["**/*.tmp"]})";
    }
    const std::string manifest_path_str = manifest_file.string();

    BbSaveRelaySyncRequest req{};
    req.game = BB_ANDROID_SAVE_GAME_U2;
    req.slot = 3;
    req.user_dir = "/data/user/0/org.uncharted/files/user";
    req.manifest_path = manifest_path_str.c_str();
    req.resolve = BB_SAVE_RELAY_RESOLVE_NONE;

    // 1. Successful pull before play (reads manifest from disk and rewrites id for slot 3).
    g_mock.next_pull_json =
        R"({"ok":true,"command":"pull","data":{"game":"uncharted-2-slot-3","action":"pulled","revision":12,"message":"Pulled revision 12."}})";
    BbSaveRelaySyncResult res{};
    assert(bb::android::saverelay::PullBeforePlay(req, &kMockAbi, res) ==
           BB_SAVE_RELAY_OUTCOME_DONE);
    assert(std::strcmp(res.action, "pulled") == 0);
    assert(std::strcmp(res.game_id, "uncharted-2-slot-3") == 0);
    assert(res.has_revision == 1 && res.revision == 12);
    assert(!g_mock.had_resolve);
    assert(g_mock.last_saves_dir ==
           "/data/user/0/org.uncharted/files/user/Save slots/Uncharted 2/Slot 3");
    assert(g_mock.last_manifest.find("\"id\":\"uncharted-2-slot-3\"") != std::string::npos);

    // 2. Successful push after exit.
    g_mock.next_push_json =
        R"({"ok":true,"command":"push","data":{"game":"uncharted-2-slot-3","action":"pushed","revision":13,"message":"Pushed revision 13."}})";
    assert(bb::android::saverelay::PushAfterExit(req, &kMockAbi, res) ==
           BB_SAVE_RELAY_OUTCOME_DONE);
    assert(std::strcmp(res.action, "pushed") == 0);
    assert(res.has_revision == 1 && res.revision == 13);

    // 3. Offline fallback on pull (both action=="offline" and error.code=="server_unreachable").
    g_mock.next_pull_json =
        R"({"ok":true,"command":"pull","data":{"game":"uncharted-2-slot-3","action":"offline","revision":null,"message":"Offline; using local saves."}})";
    assert(bb_android_save_relay_pull_before_play(&req, &kMockAbi, &res) ==
           BB_SAVE_RELAY_OUTCOME_OFFLINE);
    assert(std::strcmp(res.action, "offline") == 0);

    g_mock.next_pull_json =
        R"({"ok":false,"command":"pull","error":{"code":"server_unreachable","message":"dial tcp failed"}})";
    assert(bb_android_save_relay_pull_before_play(&req, &kMockAbi, &res) ==
           BB_SAVE_RELAY_OUTCOME_OFFLINE);

    // 4. Deferred push when offline after exit.
    g_mock.next_push_json =
        R"({"ok":true,"command":"push","data":{"game":"uncharted-2-slot-3","action":"deferred","revision":null,"message":"Queued for next sync."}})";
    assert(bb_android_save_relay_push_after_exit(&req, &kMockAbi, &res) ==
           BB_SAVE_RELAY_OUTCOME_OFFLINE);
    assert(std::strcmp(res.action, "deferred") == 0);

    // 5. Not set up -> LOCAL_ONLY fallback so player can still play locally.
    g_mock.next_pull_json =
        R"({"ok":false,"command":"pull","error":{"code":"not_set_up","message":"no account"}})";
    assert(bb_android_save_relay_pull_before_play(&req, &kMockAbi, &res) ==
           BB_SAVE_RELAY_OUTCOME_LOCAL_ONLY);

    // 6. Auth failure sanitizes provider path/message.
    g_mock.next_pull_json =
        R"({"ok":false,"command":"pull","error":{"code":"auth_failed","message":"/secret/token/path failed"}})";
    assert(bb_android_save_relay_pull_before_play(&req, &kMockAbi, &res) ==
           BB_SAVE_RELAY_OUTCOME_ERROR);
    assert(std::strstr(res.message, "/secret/token/path") == nullptr);

    // 7. Held by another device on pull.
    g_mock.next_pull_json =
        R"({"ok":true,"command":"pull","data":{"game":"uncharted-2-slot-3","action":"held","held":{"device":"strix-halo","since":"2026-11-22T20:00:00Z"},"message":"Held"}})";
    assert(bb_android_save_relay_pull_before_play(&req, &kMockAbi, &res) ==
           BB_SAVE_RELAY_OUTCOME_HELD);
    assert(res.held.present == 1);
    assert(std::strcmp(res.held.device, "strix-halo") == 0);
    assert(std::strcmp(res.held.since, "2026-11-22T20:00:00Z") == 0);

    // 8. Phase/action mismatches and game/slot mismatches fail closed.
    g_mock.next_pull_json =
        R"({"ok":true,"command":"pull","data":{"game":"uncharted-2-slot-3","action":"deferred","message":"bad"}})";
    assert(bb_android_save_relay_pull_before_play(&req, &kMockAbi, &res) ==
           BB_SAVE_RELAY_OUTCOME_ERROR);

    g_mock.next_pull_json =
        R"({"ok":true,"command":"pull","data":{"game":"uncharted-2-slot-1","action":"pulled","message":"wrong slot"}})";
    assert(bb_android_save_relay_pull_before_play(&req, &kMockAbi, &res) ==
           BB_SAVE_RELAY_OUTCOME_ERROR);

    std::filesystem::remove_all(tmp_dir);
    assert(g_mock.alloc_count == g_mock.free_count);
}

void TestConflictSurfacingAndResolution() {
    g_mock = MockRelayState{};
    BbSaveRelaySyncRequest req{};
    req.game = BB_ANDROID_SAVE_GAME_U1;
    req.slot = 1;
    req.user_dir = "/data/user/0/org.uncharted/files/user";
    req.resolve = BB_SAVE_RELAY_RESOLVE_NONE;

    g_mock.next_pull_json =
        "{\"ok\":true,\"command\":\"pull\",\"data\":{"
        "\"game\":\"uncharted-1-slot-1\","
        "\"action\":\"conflict\","
        "\"server\":{\"device\":\"strix-halo\\n\",\"time\":\"2026-11-22T21:30:00Z\",\"revision\":9},"
        "\"local\":{\"device\":\"pixel-fold\",\"time\":\"2026-11-22T20:15:00Z\",\"revision\":8},"
        "\"server_card\":{\"place\":\"Chapter 12 - Heading Upriver\",\"playtime_secs\":7200},"
        "\"local_card\":{\"place\":\"Chapter 10 - The Customs House\",\"playtime_secs\":5400},"
        "\"message\":\"Both sides changed.\""
        "}}";

    BbSaveRelaySyncResult res{};
    assert(bb_android_save_relay_pull_before_play(&req, &kMockAbi, &res) ==
           BB_SAVE_RELAY_OUTCOME_CONFLICT);
    assert(res.server.present == 1);
    assert(res.local.present == 1);
    assert(res.server_is_newer == 1);
    // Control character '\n' stripped from device name.
    assert(std::strcmp(res.server.device, "strix-halo") == 0);
    assert(std::strcmp(res.local.device, "pixel-fold") == 0);
    assert(res.server.has_revision == 1 && res.server.revision == 9);
    assert(res.local.has_revision == 1 && res.local.revision == 8);
    assert(std::strcmp(res.server.place, "Chapter 12 - Heading Upriver") == 0);
    assert(res.server.has_playtime == 1 && res.server.playtime_secs == 7200);
    assert(res.local.has_playtime == 1 && res.local.playtime_secs == 5400);

    // Retry with keep-server.
    req.resolve = BB_SAVE_RELAY_RESOLVE_KEEP_SERVER;
    g_mock.next_pull_json =
        R"({"ok":true,"command":"pull","data":{"game":"uncharted-1-slot-1","action":"pulled","revision":9,"message":"Kept server save."}})";
    assert(bb_android_save_relay_pull_before_play(&req, &kMockAbi, &res) ==
           BB_SAVE_RELAY_OUTCOME_DONE);
    assert(g_mock.had_resolve && g_mock.last_resolve == "keep-server");

    // Push conflict with keep-local resolution.
    req.resolve = BB_SAVE_RELAY_RESOLVE_KEEP_LOCAL;
    g_mock.next_push_json =
        R"({"ok":true,"command":"push","data":{"game":"uncharted-1-slot-1","action":"pushed","revision":10,"message":"Kept local save."}})";
    assert(bb_android_save_relay_push_after_exit(&req, &kMockAbi, &res) ==
           BB_SAVE_RELAY_OUTCOME_DONE);
    assert(g_mock.had_resolve && g_mock.last_resolve == "keep-local");

    assert(g_mock.alloc_count == g_mock.free_count);
}

void TestStatusAndAuthFlows() {
    g_mock = MockRelayState{};
    BbSaveRelaySyncRequest req{};
    req.game = BB_ANDROID_SAVE_GAME_U3;
    req.slot = 2;
    req.user_dir = "/data/user/0/org.uncharted/files/user";

    g_mock.next_status_json =
        R"({"ok":true,"command":"status","data":{"game":"uncharted-3-slot-2","reachable":false,"would":"offline","message":"Server offline"}})";
    BbSaveRelaySyncResult res{};
    assert(bb::android::saverelay::Status(req, &kMockAbi, res) == BB_SAVE_RELAY_OUTCOME_OFFLINE);

    g_mock.next_status_json =
        R"({"ok":true,"command":"status","data":{"game":"uncharted-3-slot-2","reachable":true,"would":"noop","held":{"device":"deck","since":"2026-11-22T18:00:00Z"},"message":"Held"}})";
    assert(bb::android::saverelay::Status(req, &kMockAbi, res) == BB_SAVE_RELAY_OUTCOME_HELD);
    assert(res.held.present == 1 && std::strcmp(res.held.device, "deck") == 0);

    // Nextcloud setup: rejects multiline passwords; succeeds with valid inputs.
    char err[256] = {};
    assert(bb_android_save_relay_setup_nextcloud("https://cloud.example", "nate", "bad\npass",
                                               "pixel-fold", nullptr, &kMockAbi, err,
                                               sizeof(err)) < 0);
    assert(g_mock.setup_calls == 0);

    assert(bb_android_save_relay_setup_nextcloud("https://cloud.example", "nate", "app-pass-123",
                                               "pixel-fold", "save-relay", &kMockAbi, err,
                                               sizeof(err)) == 0);
    assert(g_mock.setup_calls == 1);
    assert(g_mock.last_setup_server == "https://cloud.example");
    assert(g_mock.last_setup_user == "nate");
    assert(g_mock.last_setup_password == "app-pass-123");

    // OAuth PKCE browser sign-in (save-relay#254).
    BbSaveRelaySignInChallenge ch{};
    assert(bb_android_save_relay_sign_in_start("nextcloud", "uncharted://oauth", "pixel-fold",
                                              nullptr, &kMockAbi, &ch, err, sizeof(err)) < 0);

    g_mock.next_sign_in_start_json =
        R"({"ok":true,"command":"setup","data":{"url":"https://accounts.google.com/o/oauth2/v2/auth?state=xyz","user_code":null,"message":"Open browser"}})";
    assert(bb_android_save_relay_sign_in_start("gdrive", "uncharted://oauth", "pixel-fold",
                                              nullptr, &kMockAbi, &ch, err, sizeof(err)) == 0);
    assert(std::strcmp(ch.service, "gdrive") == 0);
    assert(std::strcmp(ch.verification_url,
                       "https://accounts.google.com/o/oauth2/v2/auth?state=xyz") == 0);

    assert(bb_android_save_relay_sign_in_finish("uncharted://oauth?code=abc&state=xyz", &kMockAbi,
                                               err, sizeof(err)) == 0);
    assert(g_mock.sign_in_finish_calls == 1);
    assert(g_mock.alloc_count == g_mock.free_count);
}

void TestGameFilesBridgeAndFallbacks() {
    g_mock = MockRelayState{};
    const char* manifest_u2 =
        R"({"version":1,"game_id":"uncharted-2","files":[{"path":"u2data/a.psarc","size":4096,"sha256":"0000000000000000000000000000000000000000000000000000000000000000"}]})";

    BbSaveRelayGameFilesRequest req{};
    req.game = BB_ANDROID_SAVE_GAME_U2;
    req.dest_dir = "/data/user/0/org.uncharted.universal.u2/files/games/uncharted-2/";
    req.manifest_json_override = manifest_u2;

    // NULL-safe fallback when game-files function pointers are missing.
    BbSaveRelayCAbi partial_abi = kMockAbi;
    partial_abi.download_game_files = nullptr;
    partial_abi.game_files_status = nullptr;
    BbSaveRelayGameFilesResult res{};
    assert(bb::android::saverelay::DownloadGameFiles(req, &partial_abi, res) ==
           BB_SAVE_RELAY_OUTCOME_UNAVAILABLE);
    assert(std::strcmp(res.error_code, "unavailable") == 0);
    assert(bb::android::saverelay::GameFilesStatus(req, &partial_abi, res) ==
           BB_SAVE_RELAY_OUTCOME_UNAVAILABLE);

    // Path traversal rejection.
    BbSaveRelayGameFilesRequest bad_req = req;
    bad_req.dest_dir = "/data/user/0/../escape";
    assert(bb::android::saverelay::DownloadGameFiles(bad_req, &kMockAbi, res) ==
           BB_SAVE_RELAY_OUTCOME_ERROR);
    assert(std::strcmp(res.error_code, "invalid_path") == 0);

    // Partial `.part` status decoding.
    g_mock.next_game_files_status_json =
        R"({"ok":true,"command":"game-files-status","data":{"game_id":"uncharted-2","total_files":2,"completed_files":1,"partial_files":1,"bytes_on_disk":3072,"total_bytes":4096,"complete":false}})";
    assert(bb::android::saverelay::GameFilesStatus(req, &kMockAbi, res) ==
           BB_SAVE_RELAY_OUTCOME_DONE);
    assert(res.complete == 0);
    assert(res.completed_files == 1 && res.partial_files == 1);
    assert(res.bytes_on_disk == 3072 && res.total_bytes == 4096);
    assert(g_mock.last_dest_dir == "/data/user/0/org.uncharted.universal.u2/files/games/uncharted-2");

    // Completed download with skipped file.
    g_mock.next_download_game_files_json =
        R"({"ok":true,"command":"game-files","data":{"game_id":"uncharted-2","total_files":2,"downloaded_files":1,"skipped_files":1,"bytes_downloaded":1024,"total_bytes":4096,"complete":true}})";
    assert(bb::android::saverelay::DownloadGameFiles(req, &kMockAbi, res) ==
           BB_SAVE_RELAY_OUTCOME_DONE);
    assert(res.complete == 1);
    assert(res.downloaded_files == 1 && res.skipped_files == 1);
    assert(res.bytes_downloaded == 1024 && res.total_bytes == 4096);

    // reauth_needed and offline error mapping.
    g_mock.next_download_game_files_json =
        R"({"ok":false,"command":"game-files","error":{"code":"reauth_needed","message":"Dropbox token expired"}})";
    assert(bb::android::saverelay::DownloadGameFiles(req, &kMockAbi, res) ==
           BB_SAVE_RELAY_OUTCOME_ERROR);
    assert(res.reauth_needed == 1);
    assert(std::strcmp(res.error_code, "reauth_needed") == 0);

    g_mock.next_download_game_files_json =
        R"({"ok":false,"command":"game-files","error":{"code":"server_unreachable","message":"offline"}})";
    assert(bb::android::saverelay::DownloadGameFiles(req, &kMockAbi, res) ==
           BB_SAVE_RELAY_OUTCOME_OFFLINE);
    assert(res.reauth_needed == 0);
    assert(std::strcmp(res.error_code, "server_unreachable") == 0);
    assert(g_mock.alloc_count == g_mock.free_count);
}

} // namespace

int main() {
    TestSlotPathsAndManifests();
    TestInitAndUnavailable();
    TestPullBeforePlayAndPushAfterExit();
    TestConflictSurfacingAndResolution();
    TestStatusAndAuthFlows();
    TestGameFilesBridgeAndFallbacks();
    std::printf("test_save_relay_bridge: all checks passed\n");
    return 0;
}
