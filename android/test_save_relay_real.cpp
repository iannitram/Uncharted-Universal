// SPDX-License-Identifier: GPL-2.0-only
// Real-library integration test linking android/save_relay_bridge.cpp directly
// against iannitram/save-relay (libsave_relay.a, -DBB_HAVE_SAVE_RELAY).

#include "save_relay.h"
#include "save_relay_bridge.h"

#include <cassert>
#include <cctype>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <map>
#include <string>
#include <vector>

namespace fs = std::filesystem;

namespace {

struct InMemoryHost {
    std::map<std::string, std::string> creds;
    std::map<std::string, std::vector<uint8_t>> webdav_files;
    std::map<std::string, std::vector<uint8_t>> dropbox_files;
    bool offline = false;
    bool dropbox_reauth_needed = false;
    int dropbox_fail_after_downloads = -1;
    int dropbox_download_calls = 0;
    std::vector<std::string> dropbox_ranges;
    int http_calls = 0;
};

int CredGet(void* user_data, const char* account, uint8_t* buf, size_t cap, size_t* len) {
    auto* host = static_cast<InMemoryHost*>(user_data);
    if (!host || !account || !buf || !len) {
        return 2;
    }
    auto it = host->creds.find(account);
    if (it == host->creds.end()) {
        return 1;
    }
    if (it->second.size() > cap) {
        return 2;
    }
    std::memcpy(buf, it->second.data(), it->second.size());
    *len = it->second.size();
    return 0;
}

int CredSet(void* user_data, const char* account, const char* secret) {
    auto* host = static_cast<InMemoryHost*>(user_data);
    if (!host || !account || !secret) {
        return 2;
    }
    host->creds[account] = secret;
    return 0;
}

int CredDelete(void* user_data, const char* account) {
    auto* host = static_cast<InMemoryHost*>(user_data);
    if (!host || !account) {
        return 2;
    }
    host->creds.erase(account);
    return 0;
}

std::string ExtractUrlPath(const char* url) {
    if (!url) {
        return "/";
    }
    std::string s(url);
    const size_t scheme = s.find("://");
    if (scheme == std::string::npos) {
        return s;
    }
    const size_t slash = s.find('/', scheme + 3);
    if (slash == std::string::npos) {
        return "/";
    }
    std::string path = s.substr(slash);
    const size_t q = path.find('?');
    if (q != std::string::npos) {
        path.resize(q);
    }
    return path;
}

std::string XmlEscape(const std::string& s) {
    std::string out;
    for (char c : s) {
        switch (c) {
            case '&': out += "&amp;"; break;
            case '<': out += "&lt;"; break;
            case '>': out += "&gt;"; break;
            default: out.push_back(c); break;
        }
    }
    return out;
}

int HttpCallback(void* user_data, const SaveRelayHttpRequest* req, SaveRelayHttpSink* sink) {
    auto* host = static_cast<InMemoryHost*>(user_data);
    if (!host || !req || !sink || !req->method || !req->url) {
        return 1;
    }
    ++host->http_calls;
    if (host->offline) {
        return 1;
    }

    const std::string method(req->method);
    const std::string path = ExtractUrlPath(req->url);

    auto find_header = [&](const char* name) -> std::string {
        if (!req->headers) {
            return {};
        }
        std::string hdrs(req->headers);
        std::string needle = std::string(name) + ":";
        for (auto& ch : needle) {
            ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
        }
        size_t pos = 0;
        while (pos < hdrs.size()) {
            size_t nl = hdrs.find('\n', pos);
            if (nl == std::string::npos) {
                nl = hdrs.size();
            }
            std::string line = hdrs.substr(pos, nl - pos);
            std::string lower = line;
            for (auto& ch : lower) {
                ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
            }
            if (lower.rfind(needle, 0) == 0) {
                std::string val = line.substr(needle.size());
                while (!val.empty() && (val.front() == ' ' || val.front() == '\t')) {
                    val.erase(val.begin());
                }
                while (!val.empty() && val.back() == '\r') {
                    val.pop_back();
                }
                return val;
            }
            pos = nl + 1;
        }
        return {};
    };

    if (method == "POST" && path == "/oauth2/token") {
        if (host->dropbox_reauth_needed) {
            const std::string body = R"({"error":"invalid_grant","error_description":"expired"})";
            return save_relay_http_respond(sink, 400, "Content-Type: application/json\n",
                                           reinterpret_cast<const uint8_t*>(body.data()), body.size());
        }
        const std::string body =
            R"({"access_token":"dbx-access-1","token_type":"bearer","expires_in":14400,"refresh_token":"dbx-refresh-1"})";
        return save_relay_http_respond(sink, 200, "Content-Type: application/json\n",
                                       reinterpret_cast<const uint8_t*>(body.data()), body.size());
    }

    if (method == "POST" && path == "/2/users/get_current_account") {
        const std::string body = R"({"account_id":"dbid:1","email":"nate@example.com"})";
        return save_relay_http_respond(sink, 200, "Content-Type: application/json\n",
                                       reinterpret_cast<const uint8_t*>(body.data()), body.size());
    }

    if (method == "POST" && path == "/2/files/list_folder") {
        const std::string body = R"({"entries":[],"cursor":"c1","has_more":false})";
        return save_relay_http_respond(sink, 200, "Content-Type: application/json\n",
                                       reinterpret_cast<const uint8_t*>(body.data()), body.size());
    }

    if (method == "POST" && path == "/2/files/create_folder_v2") {
        const std::string body = R"({"metadata":{"name":"save-relay","id":"id:1"}})";
        return save_relay_http_respond(sink, 200, "Content-Type: application/json\n",
                                       reinterpret_cast<const uint8_t*>(body.data()), body.size());
    }

    if (method == "POST" && path == "/2/files/download") {
        if (host->dropbox_reauth_needed) {
            const std::string body = R"({"error_summary":"expired_access_token/"})";
            return save_relay_http_respond(sink, 401, "Content-Type: application/json\n",
                                           reinterpret_cast<const uint8_t*>(body.data()), body.size());
        }
        if (host->dropbox_fail_after_downloads == 0) {
            return 1; // Simulate battery death / mid-stream network drop.
        }
        if (host->dropbox_fail_after_downloads > 0) {
            --host->dropbox_fail_after_downloads;
        }
        ++host->dropbox_download_calls;
        const std::string api_arg = find_header("Dropbox-API-Arg");
        const size_t p_key = api_arg.find("\"path\"");
        std::string dbx_path;
        if (p_key != std::string::npos) {
            const size_t q1 = api_arg.find('"', p_key + 6);
            const size_t q2 = (q1 != std::string::npos) ? api_arg.find('"', q1 + 1) : std::string::npos;
            if (q1 != std::string::npos && q2 != std::string::npos) {
                dbx_path = api_arg.substr(q1 + 1, q2 - q1 - 1);
            }
        }
        auto it = host->dropbox_files.find(dbx_path);
        if (it == host->dropbox_files.end()) {
            const std::string body = R"({"error_summary":"path/not_found/"})";
            return save_relay_http_respond(sink, 409, "Content-Type: application/json\n",
                                           reinterpret_cast<const uint8_t*>(body.data()), body.size());
        }
        const auto& full = it->second;
        const std::string range = find_header("Range");
        host->dropbox_ranges.push_back(range);
        size_t start = 0;
        size_t end = full.empty() ? 0 : (full.size() - 1);
        if (range.rfind("bytes=", 0) == 0) {
            const size_t dash = range.find('-', 6);
            if (dash != std::string::npos) {
                start = static_cast<size_t>(std::strtoull(range.substr(6, dash - 6).c_str(), nullptr, 10));
                if (dash + 1 < range.size()) {
                    end = static_cast<size_t>(std::strtoull(range.substr(dash + 1).c_str(), nullptr, 10));
                }
            }
        }
        if (full.empty() || start >= full.size()) {
            return save_relay_http_respond(sink, 416, nullptr, nullptr, 0);
        }
        if (end >= full.size()) {
            end = full.size() - 1;
        }
        const size_t len = end - start + 1;
        return save_relay_http_respond(sink, 206, "Content-Type: application/octet-stream\n",
                                       full.data() + start, len);
    }

    if (method == "PUT") {
        std::vector<uint8_t> body;
        if (req->body && req->body_len > 0) {
            body.assign(req->body, req->body + req->body_len);
        }
        host->webdav_files[path] = std::move(body);
        return save_relay_http_respond(sink, 201, "Content-Length: 0\nETag: \"etag-1\"\n",
                                       nullptr, 0);
    }

    if (method == "GET") {
        auto it = host->webdav_files.find(path);
        if (it == host->webdav_files.end()) {
            return save_relay_http_respond(sink, 404, nullptr, nullptr, 0);
        }
        return save_relay_http_respond(
            sink, 200, "Content-Type: application/octet-stream\nETag: \"etag-1\"\n",
            it->second.empty() ? nullptr : it->second.data(), it->second.size());
    }

    if (method == "HEAD") {
        auto it = host->webdav_files.find(path);
        if (it == host->webdav_files.end()) {
            return save_relay_http_respond(sink, 404, nullptr, nullptr, 0);
        }
        return save_relay_http_respond(sink, 200, nullptr, nullptr, 0);
    }

    if (method == "DELETE") {
        const size_t erased = host->webdav_files.erase(path);
        const std::string prefix = path.back() == '/' ? path : (path + "/");
        for (auto it = host->webdav_files.begin(); it != host->webdav_files.end();) {
            if (it->first.rfind(prefix, 0) == 0) {
                it = host->webdav_files.erase(it);
            } else {
                ++it;
            }
        }
        return save_relay_http_respond(sink, erased ? 204 : 404, nullptr, nullptr, 0);
    }

    if (method == "MKCOL") {
        return save_relay_http_respond(sink, 201, nullptr, nullptr, 0);
    }

    if (method == "MOVE") {
        std::string dest_url;
        if (req->headers) {
            const std::string hdrs(req->headers);
            size_t pos = 0;
            while (pos < hdrs.size()) {
                size_t nl = hdrs.find('\n', pos);
                if (nl == std::string::npos) nl = hdrs.size();
                std::string line = hdrs.substr(pos, nl - pos);
                if (line.size() >= 12) {
                    std::string prefix = line.substr(0, 12);
                    for (char& c : prefix) c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
                    if (prefix == "destination:") {
                        dest_url = line.substr(12);
                        while (!dest_url.empty() && (dest_url.front() == ' ' || dest_url.front() == '\t')) {
                            dest_url.erase(dest_url.begin());
                        }
                        while (!dest_url.empty() && dest_url.back() == '\r') {
                            dest_url.pop_back();
                        }
                        break;
                    }
                }
                pos = nl + 1;
            }
        }
        if (dest_url.empty()) {
            return save_relay_http_respond(sink, 400, nullptr, nullptr, 0);
        }
        const std::string src_prefix = path.back() == '/' ? path : (path + "/");
        const std::string dst_path = ExtractUrlPath(dest_url.c_str());
        const std::string dst_prefix = dst_path.back() == '/' ? dst_path : (dst_path + "/");
        std::vector<std::pair<std::string, std::vector<uint8_t>>> moved;
        for (auto it = host->webdav_files.begin(); it != host->webdav_files.end();) {
            if (it->first == path) {
                moved.emplace_back(dst_path, std::move(it->second));
                it = host->webdav_files.erase(it);
            } else if (it->first.rfind(src_prefix, 0) == 0) {
                moved.emplace_back(dst_prefix + it->first.substr(src_prefix.size()), std::move(it->second));
                it = host->webdav_files.erase(it);
            } else {
                ++it;
            }
        }
        for (auto& kv : moved) {
            host->webdav_files[kv.first] = std::move(kv.second);
        }
        return save_relay_http_respond(sink, 201, nullptr, nullptr, 0);
    }

    if (method == "PROPFIND") {
        // Return a minimal WebDAV multistatus listing for `path` and its direct children.
        std::string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?>"
                          "<d:multistatus xmlns:d=\"DAV:\">";
        const std::string norm = path.back() == '/' ? path : (path + "/");
        bool found_self = (path == "/" || path.find("/remote.php/dav/") != std::string::npos);
        std::map<std::string, bool> children;
        for (const auto& kv : host->webdav_files) {
            if (kv.first == path) {
                found_self = true;
            } else if (kv.first.rfind(norm, 0) == 0) {
                found_self = true;
                const std::string rest = kv.first.substr(norm.size());
                const size_t slash = rest.find('/');
                if (slash == std::string::npos) {
                    children[norm + rest] = false;
                } else {
                    children[norm + rest.substr(0, slash + 1)] = true;
                }
            }
        }
        if (!found_self) {
            return save_relay_http_respond(sink, 404, nullptr, nullptr, 0);
        }
        xml += "<d:response><d:href>" + XmlEscape(path) +
               "</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype>"
               "</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>";
        for (const auto& child : children) {
            xml += "<d:response><d:href>" + XmlEscape(child.first) + "</d:href><d:propstat><d:prop>";
            if (child.second) {
                xml += "<d:resourcetype><d:collection/></d:resourcetype>";
            } else {
                xml += "<d:resourcetype/><d:getetag>\"etag-1\"</d:getetag>";
            }
            xml += "</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>";
        }
        if (!children.empty() || host->webdav_files.count(path)) {
            // Also include getetag when PROPFIND targets a file directly.
            const std::string needle = "<d:href>" + XmlEscape(path) + "</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype>";
            const size_t pos = xml.find(needle);
            if (pos != std::string::npos && host->webdav_files.count(path)) {
                xml.replace(pos, needle.size(),
                            "<d:href>" + XmlEscape(path) +
                                "</d:href><d:propstat><d:prop><d:resourcetype/><d:getetag>\"etag-1\"</d:getetag>");
            }
        }
        xml += "</d:multistatus>";
        return save_relay_http_respond(
            sink, 207, "Content-Type: application/xml; charset=utf-8\n",
            reinterpret_cast<const uint8_t*>(xml.data()), xml.size());
    }

    return save_relay_http_respond(sink, 405, nullptr, nullptr, 0);
}

} // namespace

int main(int argc, char** argv) {
    bool game_files_only = false;
    fs::path base = fs::temp_directory_path() / "uu_test_save_relay_real";
    for (int i = 1; i < argc; ++i) {
        if (argv[i] && std::strcmp(argv[i], "--game-files-only") == 0) {
            game_files_only = true;
        } else if (argv[i] && argv[i][0] != '\0') {
            base = fs::path(argv[i]);
        }
    }
    std::error_code ec;
    fs::remove_all(base, ec);

    const fs::path home_dir = base / "home";
    const fs::path data_dir = base / "data";
    const fs::path config_dir = base / "config";
    const fs::path user_dir = base / "user";
    fs::create_directories(home_dir);
    fs::create_directories(data_dir);
    fs::create_directories(config_dir);
    fs::create_directories(user_dir);

    InMemoryHost mem;
    const std::string home_s = home_dir.string();
    const std::string data_s = data_dir.string();
    const std::string config_s = config_dir.string();
    const std::string user_s = user_dir.string();

    SaveRelayHost host{};
    host.user_data = &mem;
    host.home = home_s.c_str();
    host.data = data_s.c_str();
    host.config = config_s.c_str();
    host.documents = nullptr;
    host.cred_get = CredGet;
    host.cred_set = CredSet;
    host.cred_delete = CredDelete;
    host.http = HttpCallback;

    std::string err;
    assert(bb::android::saverelay::Init(host, nullptr, err) == 0);
    assert(err.empty());

    // 1. Before setup, pull falls back to LOCAL_ONLY.
    BbSaveRelaySyncRequest req{};
    req.game = BB_ANDROID_SAVE_GAME_U1;
    req.slot = 2;
    req.user_dir = user_s.c_str();
    req.resolve = BB_SAVE_RELAY_RESOLVE_NONE;

    char err_buf[512] = {};
    BbSaveRelaySignInChallenge challenge{};

    if (!game_files_only) {
        BbSaveRelaySyncResult res{};
        assert(bb::android::saverelay::PullBeforePlay(req, nullptr, res) ==
               BB_SAVE_RELAY_OUTCOME_LOCAL_ONLY);

        // 2. Setup Nextcloud against our in-memory WebDAV HTTP callback.
        assert(bb_android_save_relay_setup_nextcloud(
                   "https://cloud.example", "nate", "app-pass-123", "pixel-fold", "save-relay",
                   nullptr, err_buf, sizeof(err_buf)) == 0);
        assert(!mem.creds.empty());
        assert(mem.http_calls > 0);

        // 3. Create synthetic save in slot 2 and push after exit.
        char saves_dir_buf[1024] = {};
        assert(bb_android_save_relay_slot_saves_dir(user_s.c_str(), BB_ANDROID_SAVE_GAME_U1, 2,
                                                    saves_dir_buf, sizeof(saves_dir_buf)) == 0);
        const fs::path save_file = fs::path(saves_dir_buf) / "SAVEDATA01_00" / "PARAM.SFO";
        fs::create_directories(save_file.parent_path());
        {
            std::ofstream out(save_file, std::ios::binary);
            out << "SYNTHETIC_SAVE_V1";
        }

        const int push_rc = bb::android::saverelay::PushAfterExit(req, nullptr, res);
        if (push_rc != BB_SAVE_RELAY_OUTCOME_DONE) {
            char mj[4096] = {};
            bb_android_save_relay_manifest_json(req.game, req.slot, mj, sizeof(mj));
            char* raw = save_relay_push(mj, saves_dir_buf, nullptr);
            std::fprintf(stderr, "PushAfterExit failed: outcome=%d action=%s code=%s msg=%s raw=%s\n",
                         push_rc, res.action, res.error_code, res.message, raw ? raw : "(null)");
            save_relay_free(raw);
        }
        assert(push_rc == BB_SAVE_RELAY_OUTCOME_DONE);
        assert(std::strcmp(res.action, "pushed") == 0);
        assert(res.has_revision == 1 && res.revision == 1);

        // 4. Delete local save and pull before play -> restores file from mock WebDAV.
        fs::remove_all(saves_dir_buf);
        assert(!fs::exists(save_file));
        const int pull_rc = bb::android::saverelay::PullBeforePlay(req, nullptr, res);
        if (pull_rc != BB_SAVE_RELAY_OUTCOME_DONE) {
            char mj[4096] = {};
            bb_android_save_relay_manifest_json(req.game, req.slot, mj, sizeof(mj));
            char* raw = save_relay_pull(mj, saves_dir_buf, nullptr);
            std::fprintf(stderr, "PullBeforePlay failed: outcome=%d action=%s code=%s msg=%s raw=%s\n",
                         pull_rc, res.action, res.error_code, res.message, raw ? raw : "(null)");
            save_relay_free(raw);
        }
        assert(pull_rc == BB_SAVE_RELAY_OUTCOME_DONE);
        assert(std::strcmp(res.action, "pulled") == 0);
        assert(res.has_revision == 1 && res.revision == 1);
        assert(fs::exists(save_file));
        {
            std::ifstream in(save_file, std::ios::binary);
            std::string content((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
            assert(content == "SYNTHETIC_SAVE_V1");
        }

        // 5. Offline fallback on pull and deferred push.
        mem.offline = true;
        assert(bb::android::saverelay::PullBeforePlay(req, nullptr, res) ==
               BB_SAVE_RELAY_OUTCOME_OFFLINE);
        assert(bb::android::saverelay::PushAfterExit(req, nullptr, res) ==
               BB_SAVE_RELAY_OUTCOME_OFFLINE);
        mem.offline = false;
    }

    // 6. OAuth PKCE browser sign-in start (Dropbox, registered in ClientSet::shipped()).
    assert(bb_android_save_relay_sign_in_start("dropbox", "uncharted://oauth", "pixel-fold",
                                              nullptr, nullptr, &challenge, err_buf,
                                              sizeof(err_buf)) == 0);
    assert(std::strcmp(challenge.service, "dropbox") == 0);
    assert(std::strncmp(challenge.verification_url, "https://www.dropbox.com/", 24) == 0);

    // 7. Complete Dropbox OAuth sign-in and test Stage 7.04 resumable game-file downloads (#525 / save-relay#305).
    const std::string vurl(challenge.verification_url);
    const size_t st_pos = vurl.find("state=");
    assert(st_pos != std::string::npos);
    size_t st_end = vurl.find('&', st_pos);
    if (st_end == std::string::npos) {
        st_end = vurl.size();
    }
    const std::string state_val = vurl.substr(st_pos + 6, st_end - (st_pos + 6));
    const std::string redirect_uri = "uncharted://oauth?code=mock-auth-code&state=" + state_val;
    assert(bb_android_save_relay_sign_in_finish(redirect_uri.c_str(), nullptr, err_buf,
                                                sizeof(err_buf)) == 0);

    // Prepare 5 MiB synthetic game asset so chunk 1 = 4 MiB and chunk 2 = 1 MiB.
    constexpr size_t kAssetSize = 5 * 1024 * 1024;
    std::vector<uint8_t> asset(kAssetSize);
    for (size_t i = 0; i < kAssetSize; ++i) {
        asset[i] = static_cast<uint8_t>((i * 31 + 7) & 0xff);
    }
    mem.dropbox_files["/game-files/uncharted-1/u1data/pak01.psarc"] = asset;

    const fs::path game_dest = base / "games" / "uncharted-1";
    const std::string game_dest_s = game_dest.string();
    const char* gf_manifest =
        R"({"version":1,"game_id":"uncharted-1","files":[{"path":"u1data/pak01.psarc","size":5242880,"sha256":"f2793bdcacb21753483ac7bd7e6b6e89acecc817c81aa84a722ef1562c50824c"}]})";

    BbSaveRelayGameFilesRequest gf_req{};
    gf_req.game = BB_ANDROID_SAVE_GAME_U1;
    gf_req.dest_dir = game_dest_s.c_str();
    gf_req.manifest_json_override = gf_manifest;

    // 7a. Initial status before download -> incomplete, 0 bytes on disk.
    BbSaveRelayGameFilesResult gf_res{};
    assert(bb::android::saverelay::GameFilesStatus(gf_req, nullptr, gf_res) ==
           BB_SAVE_RELAY_OUTCOME_DONE);
    assert(gf_res.complete == 0);
    assert(gf_res.bytes_on_disk == 0);
    assert(gf_res.total_bytes == kAssetSize);

    // 7b. Simulate battery death / force-close after chunk 1 (4 MiB).
    mem.dropbox_fail_after_downloads = 1;
    assert(bb::android::saverelay::DownloadGameFiles(gf_req, nullptr, gf_res) ==
           BB_SAVE_RELAY_OUTCOME_OFFLINE);
    assert(std::strcmp(gf_res.error_code, "server_unreachable") == 0);
    mem.dropbox_fail_after_downloads = -1;

    // Verify `.part` file exists at 4 MiB and status reflects partial progress.
    const fs::path part_file = game_dest / "u1data" / "pak01.psarc.part";
    assert(fs::exists(part_file));
    assert(fs::file_size(part_file) == 4 * 1024 * 1024);
    assert(bb::android::saverelay::GameFilesStatus(gf_req, nullptr, gf_res) ==
           BB_SAVE_RELAY_OUTCOME_DONE);
    assert(gf_res.complete == 0);
    assert(gf_res.partial_files == 1);
    assert(gf_res.bytes_on_disk == 4 * 1024 * 1024);

    // 7c. Simulate expired Dropbox session (`reauth_needed`) while `.part` stays intact.
    mem.dropbox_reauth_needed = true;
    assert(bb::android::saverelay::DownloadGameFiles(gf_req, nullptr, gf_res) ==
           BB_SAVE_RELAY_OUTCOME_ERROR);
    assert(gf_res.reauth_needed == 1);
    assert(std::strcmp(gf_res.error_code, "reauth_needed") == 0);
    assert(fs::file_size(part_file) == 4 * 1024 * 1024);
    mem.dropbox_reauth_needed = false;

    // 7d. Resume from `.part` byte offset (4 MiB -> 5 MiB), verify SHA-256, and finalize.
    assert(bb::android::saverelay::DownloadGameFiles(gf_req, nullptr, gf_res) ==
           BB_SAVE_RELAY_OUTCOME_DONE);
    assert(gf_res.complete == 1);
    assert(gf_res.downloaded_files == 1);
    assert(gf_res.skipped_files == 0);
    assert(gf_res.bytes_downloaded == 1024 * 1024);
    assert(!mem.dropbox_ranges.empty());
    assert(mem.dropbox_ranges.back() == "bytes=4194304-5242879");
    assert(!fs::exists(part_file));
    assert(fs::exists(game_dest / "u1data" / "pak01.psarc"));
    assert(fs::file_size(game_dest / "u1data" / "pak01.psarc") == kAssetSize);

    // 7e. Completed-file skip on subsequent run (0 network downloads).
    const int dl_calls_before = mem.dropbox_download_calls;
    assert(bb::android::saverelay::DownloadGameFiles(gf_req, nullptr, gf_res) ==
           BB_SAVE_RELAY_OUTCOME_DONE);
    assert(gf_res.complete == 1);
    assert(gf_res.downloaded_files == 0);
    assert(gf_res.skipped_files == 1);
    assert(gf_res.bytes_downloaded == 0);
    assert(mem.dropbox_download_calls == dl_calls_before);

    fs::remove_all(base, ec);
    std::printf("test_save_relay_real: all real libsave_relay checks passed\n");
    return 0;
}
