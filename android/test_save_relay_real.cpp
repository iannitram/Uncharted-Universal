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
    bool offline = false;
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
    const fs::path base =
        (argc > 1 && argv[1] && argv[1][0] != '\0')
            ? fs::path(argv[1])
            : (fs::temp_directory_path() / "uu_test_save_relay_real");
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

    BbSaveRelaySyncResult res{};
    assert(bb::android::saverelay::PullBeforePlay(req, nullptr, res) ==
           BB_SAVE_RELAY_OUTCOME_LOCAL_ONLY);

    // 2. Setup Nextcloud against our in-memory WebDAV HTTP callback.
    char err_buf[512] = {};
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

    // 6. OAuth PKCE browser sign-in start (Dropbox, registered in ClientSet::shipped()).
    BbSaveRelaySignInChallenge challenge{};
    assert(bb_android_save_relay_sign_in_start("dropbox", "uncharted://oauth", "pixel-fold",
                                              nullptr, nullptr, &challenge, err_buf,
                                              sizeof(err_buf)) == 0);
    assert(std::strcmp(challenge.service, "dropbox") == 0);
    assert(std::strncmp(challenge.verification_url, "https://www.dropbox.com/", 24) == 0);

    fs::remove_all(base, ec);
    std::printf("test_save_relay_real: all real libsave_relay checks passed\n");
    return 0;
}
