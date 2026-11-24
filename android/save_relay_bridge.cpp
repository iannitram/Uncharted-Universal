// SPDX-License-Identifier: GPL-2.0-only
#include "save_relay_bridge.h"

#include <cctype>
#include <cerrno>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <utility>
#include <vector>

#ifdef BB_HAVE_SAVE_RELAY
extern "C" {
char* save_relay_init(const SaveRelayHost* host);
char* save_relay_pull(const char* manifest_json, const char* saves_dir, const char* resolve);
char* save_relay_push(const char* manifest_json, const char* saves_dir, const char* resolve);
char* save_relay_status(const char* manifest_json, const char* saves_dir);
char* save_relay_setup(const char* server, const char* user, const char* password,
                       const char* device, const char* root);
char* save_relay_sign_in_start(const char* service, const char* redirect_uri, const char* device,
                               const char* root);
char* save_relay_sign_in_finish(const char* redirect_url);
void save_relay_free(char* pointer);
int save_relay_http_respond(SaveRelayHttpSink* sink, uint16_t status, const char* headers,
                            const uint8_t* body, size_t body_len);
char* save_relay_download_game_files(const char* manifest_json, const char* dest_dir);
char* save_relay_game_files_status(const char* manifest_json, const char* dest_dir);
}
#endif

namespace {

bool NonEmpty(const char* s) {
    return s && s[0] != '\0';
}

void CopyCString(char* dst, size_t cap, const std::string& src) {
    if (!dst || cap == 0) {
        return;
    }
    const size_t n = (src.size() < cap - 1) ? src.size() : (cap - 1);
    if (n > 0) {
        std::memcpy(dst, src.data(), n);
    }
    dst[n] = '\0';
}

std::string SanitizeText(const std::string& raw, size_t max_len) {
    std::string out;
    out.reserve(raw.size() < max_len ? raw.size() : max_len);
    for (unsigned char ch : raw) {
        if (ch < 0x20 || ch == 0x7f) {
            continue;
        }
        out.push_back(static_cast<char>(ch));
        if (out.size() >= max_len) {
            break;
        }
    }
    return out;
}

void SetErr(char* err_buf, size_t err_cap, const std::string& msg) {
    CopyCString(err_buf, err_cap, msg);
}

bool HasPathTraversal(const std::string& path) {
    if (path.empty() || path.find('\0') != std::string::npos) {
        return true;
    }
    size_t pos = 0;
    while (pos <= path.size()) {
        size_t slash = path.find_first_of("/\\", pos);
        if (slash == std::string::npos) {
            slash = path.size();
        }
        const std::string seg = path.substr(pos, slash - pos);
        if (seg == "..") {
            return true;
        }
        if (slash == path.size()) {
            break;
        }
        pos = slash + 1;
    }
    return false;
}

std::string TrimTrailingSlashes(const char* raw) {
    std::string s = raw ? raw : "";
    while (s.size() > 1 && (s.back() == '/' || s.back() == '\\')) {
        s.pop_back();
    }
    return s;
}

struct GameMeta {
    const char* id;
    const char* folder;
    const char* save_prefix;
    const char* title;
};

bool LookupGame(uint32_t game, GameMeta& out) {
    switch (game) {
        case BB_ANDROID_SAVE_GAME_U1:
            out = {"uncharted-1", "Uncharted 1", "SAVEDATA01", "Uncharted: Drake's Fortune"};
            return true;
        case BB_ANDROID_SAVE_GAME_U2:
            out = {"uncharted-2", "Uncharted 2", "SAVEDATA02", "Uncharted 2: Among Thieves"};
            return true;
        case BB_ANDROID_SAVE_GAME_U3:
            out = {"uncharted-3", "Uncharted 3", "SAVEDATA03", "Uncharted 3: Drake's Deception"};
            return true;
        default:
            return false;
    }
}

bool ValidSlot(uint32_t slot) {
    return slot >= BB_ANDROID_SAVE_SLOT_MIN && slot <= BB_ANDROID_SAVE_SLOT_MAX;
}

int WriteOutString(const std::string& value, char* out, size_t out_cap) {
    if (!out || out_cap <= value.size()) {
        return -1;
    }
    std::memcpy(out, value.data(), value.size());
    out[value.size()] = '\0';
    return 0;
}

const BbSaveRelayCAbi* ResolveAbi(const BbSaveRelayCAbi* abi) {
    if (abi) {
        return abi;
    }
#ifdef BB_HAVE_SAVE_RELAY
    static const BbSaveRelayCAbi kLinkedAbi = {
        save_relay_init,
        save_relay_pull,
        save_relay_push,
        save_relay_status,
        save_relay_setup,
        save_relay_sign_in_start,
        save_relay_sign_in_finish,
        save_relay_free,
        save_relay_http_respond,
        save_relay_download_game_files,
        save_relay_game_files_status,
    };
    return &kLinkedAbi;
#else
    return nullptr;
#endif
}

std::string TakeAndFree(const BbSaveRelayCAbi* abi, char* ptr) {
    if (!ptr) {
        return {};
    }
    std::string result(ptr);
    if (abi && abi->free_str) {
        abi->free_str(ptr);
    }
    return result;
}

// ---- Minimal self-contained JSON parser/serializer (no external dependencies) ----

struct JsonValue {
    enum Type { Null, Bool, Number, String, Array, Object };
    Type type = Null;
    bool bool_val = false;
    double num_val = 0.0;
    uint64_t uint_val = 0;
    bool is_uint = false;
    std::string str_val;
    std::vector<JsonValue> arr_val;
    std::vector<std::pair<std::string, JsonValue>> obj_val;

    const JsonValue* Get(const char* key) const {
        if (type != Object || !key) {
            return nullptr;
        }
        for (const auto& kv : obj_val) {
            if (kv.first == key) {
                return &kv.second;
            }
        }
        return nullptr;
    }

    JsonValue* GetMut(const char* key) {
        if (type != Object || !key) {
            return nullptr;
        }
        for (auto& kv : obj_val) {
            if (kv.first == key) {
                return &kv.second;
            }
        }
        return nullptr;
    }

    std::string AsString() const {
        return type == String ? str_val : std::string();
    }

    bool AsBool(bool fallback = false) const {
        return type == Bool ? bool_val : fallback;
    }

    uint64_t AsUint64(uint64_t fallback = 0) const {
        return (type == Number && is_uint) ? uint_val : fallback;
    }
};

class JsonParser {
public:
    explicit JsonParser(const std::string& input) : src_(input) {}

    bool Parse(JsonValue& out) {
        SkipWs();
        if (!ParseValue(out)) {
            return false;
        }
        SkipWs();
        return pos_ == src_.size();
    }

private:
    const std::string& src_;
    size_t pos_ = 0;

    void SkipWs() {
        while (pos_ < src_.size()) {
            const char c = src_[pos_];
            if (c == ' ' || c == '\t' || c == '\n' || c == '\r') {
                ++pos_;
            } else {
                break;
            }
        }
    }

    bool Consume(char expected) {
        SkipWs();
        if (pos_ < src_.size() && src_[pos_] == expected) {
            ++pos_;
            return true;
        }
        return false;
    }

    bool ParseValue(JsonValue& out) {
        SkipWs();
        if (pos_ >= src_.size()) {
            return false;
        }
        const char c = src_[pos_];
        if (c == '"') {
            out = JsonValue{};
            out.type = JsonValue::String;
            return ParseString(out.str_val);
        }
        if (c == '{') {
            return ParseObject(out);
        }
        if (c == '[') {
            return ParseArray(out);
        }
        if (c == 't' && src_.compare(pos_, 4, "true") == 0) {
            pos_ += 4;
            out = JsonValue{};
            out.type = JsonValue::Bool;
            out.bool_val = true;
            return true;
        }
        if (c == 'f' && src_.compare(pos_, 5, "false") == 0) {
            pos_ += 5;
            out = JsonValue{};
            out.type = JsonValue::Bool;
            out.bool_val = false;
            return true;
        }
        if (c == 'n' && src_.compare(pos_, 4, "null") == 0) {
            pos_ += 4;
            out = JsonValue{};
            out.type = JsonValue::Null;
            return true;
        }
        if (c == '-' || (c >= '0' && c <= '9')) {
            return ParseNumber(out);
        }
        return false;
    }

    static void AppendUtf8(std::string& out, uint32_t cp) {
        if (cp <= 0x7F) {
            out.push_back(static_cast<char>(cp));
        } else if (cp <= 0x7FF) {
            out.push_back(static_cast<char>(0xC0 | (cp >> 6)));
            out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        } else if (cp <= 0xFFFF) {
            out.push_back(static_cast<char>(0xE0 | (cp >> 12)));
            out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        } else if (cp <= 0x10FFFF) {
            out.push_back(static_cast<char>(0xF0 | (cp >> 18)));
            out.push_back(static_cast<char>(0x80 | ((cp >> 12) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        }
    }

    bool ParseHex4(uint32_t& out) {
        if (pos_ + 4 > src_.size()) {
            return false;
        }
        uint32_t v = 0;
        for (int i = 0; i < 4; ++i) {
            const char c = src_[pos_++];
            v <<= 4;
            if (c >= '0' && c <= '9') v |= static_cast<uint32_t>(c - '0');
            else if (c >= 'a' && c <= 'f') v |= static_cast<uint32_t>(c - 'a' + 10);
            else if (c >= 'A' && c <= 'F') v |= static_cast<uint32_t>(c - 'A' + 10);
            else return false;
        }
        out = v;
        return true;
    }

    bool ParseString(std::string& out) {
        if (pos_ >= src_.size() || src_[pos_] != '"') {
            return false;
        }
        ++pos_;
        out.clear();
        while (pos_ < src_.size()) {
            const unsigned char c = static_cast<unsigned char>(src_[pos_++]);
            if (c == '"') {
                return true;
            }
            if (c < 0x20) {
                return false;
            }
            if (c != '\\') {
                out.push_back(static_cast<char>(c));
                continue;
            }
            if (pos_ >= src_.size()) {
                return false;
            }
            const char esc = src_[pos_++];
            switch (esc) {
                case '"': out.push_back('"'); break;
                case '\\': out.push_back('\\'); break;
                case '/': out.push_back('/'); break;
                case 'b': out.push_back('\b'); break;
                case 'f': out.push_back('\f'); break;
                case 'n': out.push_back('\n'); break;
                case 'r': out.push_back('\r'); break;
                case 't': out.push_back('\t'); break;
                case 'u': {
                    uint32_t cp = 0;
                    if (!ParseHex4(cp)) {
                        return false;
                    }
                    if (cp >= 0xD800 && cp <= 0xDBFF && pos_ + 2 <= src_.size() &&
                        src_[pos_] == '\\' && src_[pos_ + 1] == 'u') {
                        pos_ += 2;
                        uint32_t low = 0;
                        if (!ParseHex4(low) || low < 0xDC00 || low > 0xDFFF) {
                            return false;
                        }
                        cp = 0x10000 + (((cp - 0xD800) << 10) | (low - 0xDC00));
                    }
                    AppendUtf8(out, cp);
                    break;
                }
                default: return false;
            }
        }
        return false;
    }

    bool ParseNumber(JsonValue& out) {
        const size_t start = pos_;
        if (src_[pos_] == '-') {
            ++pos_;
        }
        if (pos_ >= src_.size() || !std::isdigit(static_cast<unsigned char>(src_[pos_]))) {
            return false;
        }
        bool is_int = (src_[start] != '-');
        if (src_[pos_] == '0') {
            ++pos_;
        } else {
            while (pos_ < src_.size() && std::isdigit(static_cast<unsigned char>(src_[pos_]))) {
                ++pos_;
            }
        }
        if (pos_ < src_.size() && src_[pos_] == '.') {
            is_int = false;
            ++pos_;
            if (pos_ >= src_.size() || !std::isdigit(static_cast<unsigned char>(src_[pos_]))) {
                return false;
            }
            while (pos_ < src_.size() && std::isdigit(static_cast<unsigned char>(src_[pos_]))) {
                ++pos_;
            }
        }
        if (pos_ < src_.size() && (src_[pos_] == 'e' || src_[pos_] == 'E')) {
            is_int = false;
            ++pos_;
            if (pos_ < src_.size() && (src_[pos_] == '+' || src_[pos_] == '-')) {
                ++pos_;
            }
            if (pos_ >= src_.size() || !std::isdigit(static_cast<unsigned char>(src_[pos_]))) {
                return false;
            }
            while (pos_ < src_.size() && std::isdigit(static_cast<unsigned char>(src_[pos_]))) {
                ++pos_;
            }
        }
        const std::string token = src_.substr(start, pos_ - start);
        out = JsonValue{};
        out.type = JsonValue::Number;
        out.str_val = token;
        if (is_int) {
            errno = 0;
            char* end_ptr = nullptr;
            const unsigned long long val = std::strtoull(token.c_str(), &end_ptr, 10);
            if (errno == 0 && end_ptr == token.c_str() + token.size()) {
                out.is_uint = true;
                out.uint_val = static_cast<uint64_t>(val);
            }
        }
        out.num_val = std::strtod(token.c_str(), nullptr);
        return true;
    }

    bool ParseArray(JsonValue& out) {
        if (!Consume('[')) {
            return false;
        }
        out = JsonValue{};
        out.type = JsonValue::Array;
        SkipWs();
        if (pos_ < src_.size() && src_[pos_] == ']') {
            ++pos_;
            return true;
        }
        while (true) {
            JsonValue elem;
            if (!ParseValue(elem)) {
                return false;
            }
            out.arr_val.push_back(std::move(elem));
            if (Consume(']')) {
                return true;
            }
            if (!Consume(',')) {
                return false;
            }
        }
    }

    bool ParseObject(JsonValue& out) {
        if (!Consume('{')) {
            return false;
        }
        out = JsonValue{};
        out.type = JsonValue::Object;
        SkipWs();
        if (pos_ < src_.size() && src_[pos_] == '}') {
            ++pos_;
            return true;
        }
        while (true) {
            SkipWs();
            std::string key;
            if (!ParseString(key)) {
                return false;
            }
            if (!Consume(':')) {
                return false;
            }
            JsonValue val;
            if (!ParseValue(val)) {
                return false;
            }
            out.obj_val.emplace_back(std::move(key), std::move(val));
            if (Consume('}')) {
                return true;
            }
            if (!Consume(',')) {
                return false;
            }
        }
    }
};

void WriteJsonString(std::string& out, const std::string& s) {
    out.push_back('"');
    for (unsigned char c : s) {
        switch (c) {
            case '"': out.append("\\\""); break;
            case '\\': out.append("\\\\"); break;
            case '\b': out.append("\\b"); break;
            case '\f': out.append("\\f"); break;
            case '\n': out.append("\\n"); break;
            case '\r': out.append("\\r"); break;
            case '\t': out.append("\\t"); break;
            default:
                if (c < 0x20) {
                    char buf[7];
                    std::snprintf(buf, sizeof(buf), "\\u%04x", static_cast<unsigned>(c));
                    out.append(buf);
                } else {
                    out.push_back(static_cast<char>(c));
                }
                break;
        }
    }
    out.push_back('"');
}

void SerializeJson(std::string& out, const JsonValue& v) {
    switch (v.type) {
        case JsonValue::Null: out.append("null"); break;
        case JsonValue::Bool: out.append(v.bool_val ? "true" : "false"); break;
        case JsonValue::Number: out.append(v.str_val); break;
        case JsonValue::String: WriteJsonString(out, v.str_val); break;
        case JsonValue::Array:
            out.push_back('[');
            for (size_t i = 0; i < v.arr_val.size(); ++i) {
                if (i > 0) out.push_back(',');
                SerializeJson(out, v.arr_val[i]);
            }
            out.push_back(']');
            break;
        case JsonValue::Object:
            out.push_back('{');
            for (size_t i = 0; i < v.obj_val.size(); ++i) {
                if (i > 0) out.push_back(',');
                WriteJsonString(out, v.obj_val[i].first);
                out.push_back(':');
                SerializeJson(out, v.obj_val[i].second);
            }
            out.push_back('}');
            break;
    }
}

std::string StripSlotSuffix(const std::string& id) {
    const std::string marker = "-slot-";
    const size_t pos = id.rfind(marker);
    if (pos == std::string::npos || pos == 0) {
        return id;
    }
    const std::string tail = id.substr(pos + marker.size());
    if (tail.empty()) {
        return id;
    }
    for (unsigned char c : tail) {
        if (!std::isdigit(c)) {
            return id;
        }
    }
    const unsigned long slot = std::strtoul(tail.c_str(), nullptr, 10);
    if (slot >= BB_ANDROID_SAVE_SLOT_MIN && slot <= BB_ANDROID_SAVE_SLOT_MAX) {
        return id.substr(0, pos);
    }
    return id;
}

bool ParseSideCard(const JsonValue* side, const JsonValue* card, BbSaveRelaySideCard& out) {
    out = BbSaveRelaySideCard{};
    if (!side || side->type != JsonValue::Object) {
        return false;
    }
    const JsonValue* dev = side->Get("device");
    const JsonValue* time = side->Get("time");
    if (!dev || dev->type != JsonValue::String || !time || time->type != JsonValue::String) {
        return false;
    }
    out.present = 1;
    CopyCString(out.device, sizeof(out.device), SanitizeText(dev->str_val, 128));
    CopyCString(out.time, sizeof(out.time), SanitizeText(time->str_val, 63));
    if (const JsonValue* rev = side->Get("revision")) {
        if (rev->type == JsonValue::Number && rev->is_uint) {
            out.revision = rev->uint_val;
            out.has_revision = 1;
        }
    }
    if (card && card->type == JsonValue::Object) {
        if (const JsonValue* place = card->Get("place")) {
            if (place->type == JsonValue::String) {
                CopyCString(out.place, sizeof(out.place), SanitizeText(place->str_val, 255));
            }
        }
        if (const JsonValue* pt = card->Get("playtime_secs")) {
            if (pt->type == JsonValue::Number && pt->is_uint) {
                out.playtime_secs = pt->uint_val;
                out.has_playtime = 1;
            }
        }
    }
    return true;
}

void ParseHeldInfo(const JsonValue* held, BbSaveRelayHeldInfo& out) {
    out = BbSaveRelayHeldInfo{};
    if (!held || held->type != JsonValue::Object) {
        return;
    }
    const JsonValue* dev = held->Get("device");
    const JsonValue* since = held->Get("since");
    if (dev && dev->type == JsonValue::String) {
        out.present = 1;
        CopyCString(out.device, sizeof(out.device), SanitizeText(dev->str_val, 128));
        if (since && since->type == JsonValue::String) {
            CopyCString(out.since, sizeof(out.since), SanitizeText(since->str_val, 63));
        }
    }
}

int SetResultError(BbSaveRelaySyncResult* out, int32_t outcome, const std::string& code,
                   const std::string& message) {
    *out = BbSaveRelaySyncResult{};
    out->outcome = outcome;
    CopyCString(out->error_code, sizeof(out->error_code), SanitizeText(code, 63));
    CopyCString(out->message, sizeof(out->message), SanitizeText(message, 511));
    return outcome;
}

const char* ResolveArg(uint32_t resolve, bool& valid) {
    valid = true;
    switch (resolve) {
        case BB_SAVE_RELAY_RESOLVE_NONE: return nullptr;
        case BB_SAVE_RELAY_RESOLVE_KEEP_SERVER: return "keep-server";
        case BB_SAVE_RELAY_RESOLVE_KEEP_LOCAL: return "keep-local";
        default:
            valid = false;
            return nullptr;
    }
}

int DecodeSyncEnvelope(const char* phase, const std::string& expected_game,
                       const std::string& raw_json, BbSaveRelaySyncResult* out) {
    *out = BbSaveRelaySyncResult{};
    if (raw_json.empty()) {
        return SetResultError(
            out, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_response",
            "Save Relay returned an unexpected response. Both saves need checking before play.");
    }
    JsonValue root;
    if (!JsonParser(raw_json).Parse(root) || root.type != JsonValue::Object) {
        return SetResultError(
            out, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_response",
            "Save Relay returned an unexpected response. Both saves need checking before play.");
    }
    const JsonValue* cmd = root.Get("command");
    const JsonValue* ok = root.Get("ok");
    if (!cmd || cmd->type != JsonValue::String || cmd->str_val != phase || !ok ||
        ok->type != JsonValue::Bool) {
        return SetResultError(
            out, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_response",
            "Save Relay returned an unexpected response. Both saves need checking before play.");
    }

    if (!ok->bool_val) {
        std::string code;
        if (const JsonValue* err = root.Get("error")) {
            if (const JsonValue* err_code = err->Get("code")) {
                code = err_code->AsString();
            }
        }
        if (code == "not_set_up") {
            return SetResultError(out, BB_SAVE_RELAY_OUTCOME_LOCAL_ONLY, code,
                                  "Save Relay needs setup; using local saves.");
        }
        if (code == "server_unreachable") {
            return SetResultError(out, BB_SAVE_RELAY_OUTCOME_OFFLINE, code,
                                  "Save Relay server unreachable; using local saves.");
        }
        if (code == "auth_failed" || code == "reauth_needed" || code == "credentials" ||
            code == "sign_in_failed") {
            return SetResultError(out, BB_SAVE_RELAY_OUTCOME_ERROR, code,
                                  "Save Relay could not sign in. Check its account setup.");
        }
        return SetResultError(
            out, BB_SAVE_RELAY_OUTCOME_ERROR, code.empty() ? "sync_failed" : code,
            "Save Relay could not complete the sync. Check its setup and try again.");
    }

    const JsonValue* data = root.Get("data");
    if (!data || data->type != JsonValue::Object) {
        return SetResultError(
            out, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_response",
            "Save Relay returned an unexpected response. Both saves need checking before play.");
    }
    const JsonValue* game = data->Get("game");
    if (!game || game->type != JsonValue::String || game->str_val != expected_game) {
        return SetResultError(
            out, BB_SAVE_RELAY_OUTCOME_ERROR, "game_mismatch",
            "Save Relay returned an unexpected response. Both saves need checking before play.");
    }
    CopyCString(out->game_id, sizeof(out->game_id), expected_game);

    if (const JsonValue* rev = data->Get("revision")) {
        if (rev->type == JsonValue::Number && rev->is_uint) {
            out->revision = rev->uint_val;
            out->has_revision = 1;
        }
    }

    const std::string data_msg =
        data->Get("message") ? SanitizeText(data->Get("message")->AsString(), 511) : std::string();

    if (std::strcmp(phase, "status") == 0) {
        ParseSideCard(data->Get("server"), nullptr, out->server);
        ParseSideCard(data->Get("local"), nullptr, out->local);
        ParseHeldInfo(data->Get("held"), out->held);
        const std::string would =
            data->Get("would") ? data->Get("would")->AsString() : std::string();
        CopyCString(out->action, sizeof(out->action), SanitizeText(would, 31));
        const bool reachable = data->Get("reachable") ? data->Get("reachable")->AsBool(true) : true;
        if (out->held.present) {
            out->outcome = BB_SAVE_RELAY_OUTCOME_HELD;
        } else if (!reachable || would == "offline") {
            out->outcome = BB_SAVE_RELAY_OUTCOME_OFFLINE;
        } else if (would == "conflict") {
            out->outcome = BB_SAVE_RELAY_OUTCOME_CONFLICT;
            if (out->server.present && out->local.present) {
                out->server_is_newer =
                    std::string(out->server.time) > std::string(out->local.time) ? 1 : 0;
            }
        } else {
            out->outcome = BB_SAVE_RELAY_OUTCOME_DONE;
        }
        CopyCString(out->message, sizeof(out->message),
                    data_msg.empty() ? "status checked" : data_msg);
        return out->outcome;
    }

    const JsonValue* action_val = data->Get("action");
    if (!action_val || action_val->type != JsonValue::String) {
        return SetResultError(
            out, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_response",
            "Save Relay returned an unexpected response. Both saves need checking before play.");
    }
    const std::string action = action_val->str_val;
    CopyCString(out->action, sizeof(out->action), SanitizeText(action, 31));

    if (action == "noop") {
        out->outcome = BB_SAVE_RELAY_OUTCOME_DONE;
        CopyCString(out->message, sizeof(out->message),
                    data_msg.empty() ? "already in sync" : data_msg);
        return out->outcome;
    }
    if (action == "adopted") {
        out->outcome = BB_SAVE_RELAY_OUTCOME_DONE;
        CopyCString(out->message, sizeof(out->message),
                    data_msg.empty() ? "matching revision recorded" : data_msg);
        return out->outcome;
    }
    if (action == "pulled" && std::strcmp(phase, "pull") == 0) {
        out->outcome = BB_SAVE_RELAY_OUTCOME_DONE;
        CopyCString(out->message, sizeof(out->message),
                    data_msg.empty() ? "downloaded saves" : data_msg);
        return out->outcome;
    }
    if (action == "pushed" && std::strcmp(phase, "push") == 0) {
        out->outcome = BB_SAVE_RELAY_OUTCOME_DONE;
        CopyCString(out->message, sizeof(out->message),
                    data_msg.empty() ? "uploaded saves" : data_msg);
        return out->outcome;
    }
    if ((action == "offline" && std::strcmp(phase, "pull") == 0) ||
        (action == "deferred" && std::strcmp(phase, "push") == 0)) {
        out->outcome = BB_SAVE_RELAY_OUTCOME_OFFLINE;
        CopyCString(out->message, sizeof(out->message),
                    data_msg.empty() ? "offline; using local saves" : data_msg);
        return out->outcome;
    }
    if (action == "conflict") {
        if (!ParseSideCard(data->Get("server"), data->Get("server_card"), out->server) ||
            !ParseSideCard(data->Get("local"), data->Get("local_card"), out->local)) {
            return SetResultError(
                out, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_conflict",
                "Save Relay returned an unexpected response. Both saves need checking before play.");
        }
        out->server_is_newer =
            std::string(out->server.time) > std::string(out->local.time) ? 1 : 0;
        out->outcome = BB_SAVE_RELAY_OUTCOME_CONFLICT;
        CopyCString(out->message, sizeof(out->message),
                    data_msg.empty() ? "Save conflict requires player choice." : data_msg);
        return out->outcome;
    }
    if (action == "held" && std::strcmp(phase, "pull") == 0) {
        ParseHeldInfo(data->Get("held"), out->held);
        out->outcome = BB_SAVE_RELAY_OUTCOME_HELD;
        CopyCString(out->message, sizeof(out->message),
                    "Another device has this game open. Quit it there and retry.");
        return out->outcome;
    }

    return SetResultError(
        out, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_response",
        "Save Relay returned an unexpected response. Both saves need checking before play.");
}

bool ReadFileToString(const char* path, std::string& out) {
    if (!NonEmpty(path) || HasPathTraversal(path)) {
        return false;
    }
    std::ifstream in(path, std::ios::in | std::ios::binary);
    if (!in) {
        return false;
    }
    std::ostringstream ss;
    ss << in.rdbuf();
    out = ss.str();
    return !out.empty();
}

int PrepareRequest(const BbSaveRelaySyncRequest* req, std::string& out_saves_dir,
                   std::string& out_manifest_json, std::string& out_expected_game,
                   const char*& out_resolve, BbSaveRelaySyncResult* out_result) {
    if (!req) {
        return SetResultError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_arg",
                              "Save Relay request is null.");
    }
    GameMeta meta{};
    if (!LookupGame(req->game, meta)) {
        return SetResultError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_game",
                              "Unknown Uncharted game id.");
    }
    if (!ValidSlot(req->slot)) {
        return SetResultError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_slot",
                              "Save Relay slot must be from 1 to 10.");
    }
    bool resolve_valid = false;
    out_resolve = ResolveArg(req->resolve, resolve_valid);
    if (!resolve_valid) {
        return SetResultError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_resolve",
                              "Invalid conflict resolution option.");
    }

    if (NonEmpty(req->saves_dir_override)) {
        if (HasPathTraversal(req->saves_dir_override)) {
            return SetResultError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_path",
                                  "Invalid save folder path.");
        }
        out_saves_dir = TrimTrailingSlashes(req->saves_dir_override);
    } else {
        char buf[1024] = {};
        if (bb_android_save_relay_slot_saves_dir(req->user_dir, req->game, req->slot, buf,
                                                 sizeof(buf)) != 0) {
            return SetResultError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_path",
                                  "Invalid user save directory.");
        }
        out_saves_dir = buf;
    }

    char expected_buf[80] = {};
    std::snprintf(expected_buf, sizeof(expected_buf), "%s-slot-%u", meta.id, req->slot);
    out_expected_game = expected_buf;

    char json_buf[4096] = {};
    if (NonEmpty(req->manifest_json_override)) {
        if (bb_android_save_relay_rewrite_manifest_for_slot(req->manifest_json_override, req->slot,
                                                            json_buf, sizeof(json_buf)) != 0) {
            return SetResultError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_manifest",
                                  "Save Relay manifest JSON is invalid.");
        }
        out_manifest_json = json_buf;
    } else if (NonEmpty(req->manifest_path)) {
        std::string file_json;
        if (!ReadFileToString(req->manifest_path, file_json) ||
            bb_android_save_relay_rewrite_manifest_for_slot(file_json.c_str(), req->slot, json_buf,
                                                            sizeof(json_buf)) != 0) {
            return SetResultError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_manifest",
                                  "Save Relay manifest file is missing or invalid.");
        }
        out_manifest_json = json_buf;
    } else {
        if (bb_android_save_relay_manifest_json(req->game, req->slot, json_buf, sizeof(json_buf)) !=
            0) {
            return SetResultError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_manifest",
                                  "Could not construct Save Relay manifest.");
        }
        out_manifest_json = json_buf;
    }
    return 0;
}

int RunSyncPhase(const char* phase, const BbSaveRelaySyncRequest* req, const BbSaveRelayCAbi* abi,
                 BbSaveRelaySyncResult* out_result) {
    if (!out_result) {
        return BB_SAVE_RELAY_OUTCOME_ERROR;
    }
    *out_result = BbSaveRelaySyncResult{};

    const BbSaveRelayCAbi* resolved = ResolveAbi(abi);
    if (!resolved || !resolved->free_str) {
        return SetResultError(out_result, BB_SAVE_RELAY_OUTCOME_UNAVAILABLE, "unavailable",
                              "Save Relay C library is not linked on this target.");
    }
    if ((std::strcmp(phase, "pull") == 0 && !resolved->pull) ||
        (std::strcmp(phase, "push") == 0 && !resolved->push) ||
        (std::strcmp(phase, "status") == 0 && !resolved->status)) {
        return SetResultError(out_result, BB_SAVE_RELAY_OUTCOME_UNAVAILABLE, "unavailable",
                              "Save Relay C entry point is missing.");
    }

    std::string saves_dir;
    std::string manifest_json;
    std::string expected_game;
    const char* resolve = nullptr;
    const int prep_rc = PrepareRequest(req, saves_dir, manifest_json, expected_game, resolve,
                                       out_result);
    if (prep_rc != 0) {
        return prep_rc;
    }

    char* raw_ptr = nullptr;
    if (std::strcmp(phase, "pull") == 0) {
        raw_ptr = resolved->pull(manifest_json.c_str(), saves_dir.c_str(), resolve);
    } else if (std::strcmp(phase, "push") == 0) {
        raw_ptr = resolved->push(manifest_json.c_str(), saves_dir.c_str(), resolve);
    } else {
        raw_ptr = resolved->status(manifest_json.c_str(), saves_dir.c_str());
    }
    const std::string raw_json = TakeAndFree(resolved, raw_ptr);
    return DecodeSyncEnvelope(phase, expected_game, raw_json, out_result);
}

int DecodeSimpleEnvelope(const char* expected_command, const std::string& raw_json,
                         const char* fallback_err, char* err_buf, size_t err_cap) {
    if (raw_json.empty()) {
        SetErr(err_buf, err_cap, fallback_err);
        return -1;
    }
    JsonValue root;
    if (!JsonParser(raw_json).Parse(root) || root.type != JsonValue::Object) {
        SetErr(err_buf, err_cap, fallback_err);
        return -1;
    }
    const JsonValue* cmd = root.Get("command");
    const JsonValue* ok = root.Get("ok");
    if (!cmd || cmd->type != JsonValue::String || cmd->str_val != expected_command || !ok ||
        ok->type != JsonValue::Bool) {
        SetErr(err_buf, err_cap, fallback_err);
        return -1;
    }
    if (ok->bool_val) {
        SetErr(err_buf, err_cap, "");
        return 0;
    }
    std::string code;
    if (const JsonValue* err = root.Get("error")) {
        if (const JsonValue* c = err->Get("code")) {
            code = c->AsString();
        }
    }
    if (code == "auth_failed" || code == "reauth_needed" || code == "credentials" ||
        code == "sign_in_failed") {
        SetErr(err_buf, err_cap, "Save Relay could not sign in. Check its account setup.");
    } else if (code == "server_unreachable") {
        SetErr(err_buf, err_cap, "Save Relay server is unreachable.");
    } else {
        SetErr(err_buf, err_cap, fallback_err);
    }
    return -1;
}

int SetGameFilesError(BbSaveRelayGameFilesResult* out, int32_t outcome, const std::string& code,
                      const std::string& msg, bool reauth_needed = false) {
    if (!out) {
        return BB_SAVE_RELAY_OUTCOME_ERROR;
    }
    out->outcome = outcome;
    out->reauth_needed = reauth_needed ? 1 : 0;
    CopyCString(out->error_code, sizeof(out->error_code), SanitizeText(code, 63));
    CopyCString(out->message, sizeof(out->message), SanitizeText(msg, 511));
    return outcome;
}

int PrepareGameFilesRequest(const BbSaveRelayGameFilesRequest* req, std::string& out_dest_dir,
                            std::string& out_manifest_json, std::string& out_expected_game,
                            BbSaveRelayGameFilesResult* out_result) {
    if (!req) {
        return SetGameFilesError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_arg",
                                 "Game files request is null.");
    }
    GameMeta meta{};
    if (!LookupGame(req->game, meta)) {
        return SetGameFilesError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_game",
                                 "Unknown Uncharted game id.");
    }
    if (!NonEmpty(req->dest_dir) || HasPathTraversal(req->dest_dir)) {
        return SetGameFilesError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_path",
                                 "Invalid game destination directory.");
    }
    out_dest_dir = TrimTrailingSlashes(req->dest_dir);
    out_expected_game = meta.id;

    std::string raw_manifest;
    if (NonEmpty(req->manifest_json_override)) {
        raw_manifest = req->manifest_json_override;
    } else if (NonEmpty(req->manifest_path)) {
        if (!ReadFileToString(req->manifest_path, raw_manifest)) {
            return SetGameFilesError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_manifest",
                                     "Game files manifest file is missing or unreadable.");
        }
    } else {
        return SetGameFilesError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_manifest",
                                 "Game files manifest JSON or path is required.");
    }

    JsonValue root;
    if (!JsonParser(raw_manifest).Parse(root) || root.type != JsonValue::Object) {
        return SetGameFilesError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_manifest",
                                 "Game files manifest JSON is invalid.");
    }
    const JsonValue* gid = root.Get("game_id");
    if (!gid || gid->type != JsonValue::String || gid->str_val != meta.id) {
        return SetGameFilesError(out_result, BB_SAVE_RELAY_OUTCOME_ERROR, "game_mismatch",
                                 "Game files manifest game_id does not match selected game.");
    }
    out_manifest_json = std::move(raw_manifest);
    return 0;
}

int DecodeGameFilesEnvelope(const char* expected_command, const std::string& expected_game,
                            const std::string& raw_json, BbSaveRelayGameFilesResult* out) {
    if (!out) {
        return BB_SAVE_RELAY_OUTCOME_ERROR;
    }
    if (raw_json.empty()) {
        return SetGameFilesError(out, BB_SAVE_RELAY_OUTCOME_ERROR, "empty_response",
                                 "Save Relay returned an empty game-files response.");
    }
    JsonValue root;
    if (!JsonParser(raw_json).Parse(root) || root.type != JsonValue::Object) {
        return SetGameFilesError(out, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_json",
                                 "Save Relay returned invalid game-files JSON.");
    }
    const JsonValue* cmd = root.Get("command");
    const JsonValue* ok = root.Get("ok");
    if (!cmd || cmd->type != JsonValue::String || cmd->str_val != expected_command || !ok ||
        ok->type != JsonValue::Bool) {
        return SetGameFilesError(out, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_response",
                                 "Save Relay returned an unexpected game-files response.");
    }

    if (!ok->bool_val) {
        std::string code;
        std::string raw_msg;
        if (const JsonValue* err = root.Get("error")) {
            if (const JsonValue* c = err->Get("code")) {
                code = c->AsString();
            }
            if (const JsonValue* m = err->Get("message")) {
                raw_msg = SanitizeText(m->AsString(), 511);
            }
        }
        if (code == "not_set_up") {
            return SetGameFilesError(
                out, BB_SAVE_RELAY_OUTCOME_LOCAL_ONLY, code,
                raw_msg.empty() ? "Dropbox sign-in is required to download game files." : raw_msg);
        }
        if (code == "server_unreachable") {
            return SetGameFilesError(
                out, BB_SAVE_RELAY_OUTCOME_OFFLINE, code,
                raw_msg.empty() ? "Dropbox is unreachable; retry when online." : raw_msg);
        }
        if (code == "reauth_needed" || code == "auth_failed" || code == "credentials" ||
            code == "sign_in_failed") {
            return SetGameFilesError(
                out, BB_SAVE_RELAY_OUTCOME_ERROR, code,
                raw_msg.empty() ? "Dropbox session expired; please sign in again." : raw_msg,
                true);
        }
        return SetGameFilesError(
            out, BB_SAVE_RELAY_OUTCOME_ERROR, code.empty() ? "download_failed" : code,
            raw_msg.empty() ? "Save Relay could not complete game-file download." : raw_msg);
    }

    const JsonValue* data = root.Get("data");
    if (!data || data->type != JsonValue::Object) {
        return SetGameFilesError(out, BB_SAVE_RELAY_OUTCOME_ERROR, "invalid_response",
                                 "Save Relay game-files response omitted data.");
    }
    const JsonValue* gid = data->Get("game_id");
    if (!gid || gid->type != JsonValue::String || gid->str_val != expected_game) {
        return SetGameFilesError(out, BB_SAVE_RELAY_OUTCOME_ERROR, "game_mismatch",
                                 "Save Relay returned game-files data for a different game.");
    }
    CopyCString(out->game_id, sizeof(out->game_id), expected_game);

    if (const JsonValue* v = data->Get("total_files")) {
        out->total_files = v->AsUint64();
    }
    if (const JsonValue* v = data->Get("completed_files")) {
        out->completed_files = v->AsUint64();
    }
    if (const JsonValue* v = data->Get("downloaded_files")) {
        out->downloaded_files = v->AsUint64();
    }
    if (const JsonValue* v = data->Get("skipped_files")) {
        out->skipped_files = v->AsUint64();
    }
    if (const JsonValue* v = data->Get("partial_files")) {
        out->partial_files = v->AsUint64();
    }
    if (const JsonValue* v = data->Get("bytes_on_disk")) {
        out->bytes_on_disk = v->AsUint64();
    }
    if (const JsonValue* v = data->Get("bytes_downloaded")) {
        out->bytes_downloaded = v->AsUint64();
    }
    if (const JsonValue* v = data->Get("total_bytes")) {
        out->total_bytes = v->AsUint64();
    }
    if (const JsonValue* v = data->Get("complete")) {
        out->complete = v->AsBool(false) ? 1 : 0;
    }
    out->outcome = BB_SAVE_RELAY_OUTCOME_DONE;
    CopyCString(out->message, sizeof(out->message),
                out->complete ? "game files ready" : "game files incomplete");
    return out->outcome;
}

int RunGameFilesPhase(bool is_download, const BbSaveRelayGameFilesRequest* req,
                      const BbSaveRelayCAbi* abi, BbSaveRelayGameFilesResult* out_result) {
    if (!out_result) {
        return BB_SAVE_RELAY_OUTCOME_ERROR;
    }
    *out_result = BbSaveRelayGameFilesResult{};

    const BbSaveRelayCAbi* resolved = ResolveAbi(abi);
    if (!resolved || !resolved->free_str) {
        return SetGameFilesError(out_result, BB_SAVE_RELAY_OUTCOME_UNAVAILABLE, "unavailable",
                                 "Save Relay C library is not linked on this target.");
    }
    if ((is_download && !resolved->download_game_files) ||
        (!is_download && !resolved->game_files_status)) {
        return SetGameFilesError(out_result, BB_SAVE_RELAY_OUTCOME_UNAVAILABLE, "unavailable",
                                 "Save Relay game-files C entry point is missing.");
    }

    std::string dest_dir;
    std::string manifest_json;
    std::string expected_game;
    const int prep_rc =
        PrepareGameFilesRequest(req, dest_dir, manifest_json, expected_game, out_result);
    if (prep_rc != 0) {
        return prep_rc;
    }

    char* raw_ptr = is_download
                        ? resolved->download_game_files(manifest_json.c_str(), dest_dir.c_str())
                        : resolved->game_files_status(manifest_json.c_str(), dest_dir.c_str());
    const std::string raw_json = TakeAndFree(resolved, raw_ptr);
    return DecodeGameFilesEnvelope(is_download ? "game-files" : "game-files-status", expected_game,
                                   raw_json, out_result);
}

} // namespace

int bb_android_save_relay_game_id(uint32_t game, char* out_id, size_t out_cap) {
    GameMeta meta{};
    if (!LookupGame(game, meta)) {
        return -1;
    }
    return WriteOutString(meta.id, out_id, out_cap);
}

int bb_android_save_relay_slot_saves_dir(const char* user_dir, uint32_t game, uint32_t slot,
                                         char* out_dir, size_t out_cap) {
    GameMeta meta{};
    if (!NonEmpty(user_dir) || HasPathTraversal(user_dir) || !LookupGame(game, meta) ||
        !ValidSlot(slot)) {
        return -1;
    }
    const std::string base = TrimTrailingSlashes(user_dir);
    std::string path;
    if (slot == 1) {
        path = base + "/" + meta.folder;
    } else {
        path = base + "/Save slots/" + meta.folder + "/Slot " + std::to_string(slot);
    }
    return WriteOutString(path, out_dir, out_cap);
}

int bb_android_save_relay_manifest_path(const char* manifests_dir, uint32_t game, uint32_t slot,
                                        char* out_path, size_t out_cap) {
    GameMeta meta{};
    if (!NonEmpty(manifests_dir) || HasPathTraversal(manifests_dir) || !LookupGame(game, meta)) {
        return -1;
    }
    const std::string base = TrimTrailingSlashes(manifests_dir);
    std::string path;
    if (slot == 0) {
        path = base + "/" + meta.id + ".json";
    } else if (ValidSlot(slot)) {
        path = base + "/" + meta.id + "-slot-" + std::to_string(slot) + ".json";
    } else {
        return -1;
    }
    return WriteOutString(path, out_path, out_cap);
}

int bb_android_save_relay_manifest_json(uint32_t game, uint32_t slot, char* out_json,
                                        size_t out_cap) {
    GameMeta meta{};
    if (!LookupGame(game, meta) || !ValidSlot(slot)) {
        return -1;
    }
    char buf[512];
    const int n = std::snprintf(
        buf, sizeof(buf),
        "{\"format\":1,\"id\":\"%s-slot-%u\",\"name\":\"%s\",\"saves\":{},"
        "\"saves_from\":\"launcher\",\"include\":[\"%s*/**\"],"
        "\"exclude\":[\"**/*.tmp\",\"**/.save-relay-*\"]}",
        meta.id, slot, meta.title, meta.save_prefix);
    if (n < 0 || static_cast<size_t>(n) >= sizeof(buf)) {
        return -1;
    }
    return WriteOutString(std::string(buf, static_cast<size_t>(n)), out_json, out_cap);
}

int bb_android_save_relay_rewrite_manifest_for_slot(const char* base_manifest_json, uint32_t slot,
                                                    char* out_json, size_t out_cap) {
    if (!NonEmpty(base_manifest_json) || !ValidSlot(slot)) {
        return -1;
    }
    JsonValue root;
    if (!JsonParser(base_manifest_json).Parse(root) || root.type != JsonValue::Object) {
        return -1;
    }
    JsonValue* id = root.GetMut("id");
    if (!id || id->type != JsonValue::String || id->str_val.empty()) {
        return -1;
    }
    const std::string base_id = StripSlotSuffix(id->str_val);
    id->str_val = base_id + "-slot-" + std::to_string(slot);
    std::string serialized;
    SerializeJson(serialized, root);
    return WriteOutString(serialized, out_json, out_cap);
}

int bb_android_save_relay_init(const SaveRelayHost* host, const BbSaveRelayCAbi* abi, char* err_buf,
                               size_t err_cap) {
    if (!host || !NonEmpty(host->home) || !NonEmpty(host->data) || !NonEmpty(host->config)) {
        SetErr(err_buf, err_cap, "Save Relay host folders are required.");
        return -1;
    }
    if (!host->cred_get || !host->cred_set || !host->cred_delete || !host->http) {
        SetErr(err_buf, err_cap, "Save Relay host credential and HTTP callbacks are required.");
        return -1;
    }
    const BbSaveRelayCAbi* resolved = ResolveAbi(abi);
    if (!resolved || !resolved->init || !resolved->free_str) {
        SetErr(err_buf, err_cap, "Save Relay C library is not linked on this target.");
        return BB_SAVE_RELAY_OUTCOME_UNAVAILABLE;
    }
    const std::string raw = TakeAndFree(resolved, resolved->init(host));
    return DecodeSimpleEnvelope("init", raw, "Save Relay initialization failed.", err_buf, err_cap);
}

int bb_android_save_relay_pull_before_play(const BbSaveRelaySyncRequest* req,
                                           const BbSaveRelayCAbi* abi,
                                           BbSaveRelaySyncResult* out_result) {
    return RunSyncPhase("pull", req, abi, out_result);
}

int bb_android_save_relay_push_after_exit(const BbSaveRelaySyncRequest* req,
                                          const BbSaveRelayCAbi* abi,
                                          BbSaveRelaySyncResult* out_result) {
    return RunSyncPhase("push", req, abi, out_result);
}

int bb_android_save_relay_status(const BbSaveRelaySyncRequest* req, const BbSaveRelayCAbi* abi,
                                 BbSaveRelaySyncResult* out_result) {
    return RunSyncPhase("status", req, abi, out_result);
}

int bb_android_save_relay_setup_nextcloud(const char* server, const char* user,
                                          const char* password, const char* device,
                                          const char* root, const BbSaveRelayCAbi* abi,
                                          char* err_buf, size_t err_cap) {
    if (!NonEmpty(server) || !NonEmpty(user) || !NonEmpty(password)) {
        SetErr(err_buf, err_cap, "Nextcloud server, user, and app password are required.");
        return -1;
    }
    for (const char* p = password; *p; ++p) {
        if (*p == '\n' || *p == '\r') {
            SetErr(err_buf, err_cap, "Password must be a single line.");
            return -1;
        }
    }
    const BbSaveRelayCAbi* resolved = ResolveAbi(abi);
    if (!resolved || !resolved->setup || !resolved->free_str) {
        SetErr(err_buf, err_cap, "Save Relay C library is not linked on this target.");
        return BB_SAVE_RELAY_OUTCOME_UNAVAILABLE;
    }
    const std::string raw =
        TakeAndFree(resolved, resolved->setup(server, user, password, device, root));
    return DecodeSimpleEnvelope(
        "setup", raw,
        "Save Relay could not sign in to Nextcloud. Check the address, user and app password.",
        err_buf, err_cap);
}

int bb_android_save_relay_sign_in_start(const char* service, const char* redirect_uri,
                                        const char* device, const char* root,
                                        const BbSaveRelayCAbi* abi,
                                        BbSaveRelaySignInChallenge* out_challenge, char* err_buf,
                                        size_t err_cap) {
    if (!out_challenge) {
        SetErr(err_buf, err_cap, "Challenge output pointer is null.");
        return -1;
    }
    *out_challenge = BbSaveRelaySignInChallenge{};
    if (!NonEmpty(service) || !NonEmpty(redirect_uri)) {
        SetErr(err_buf, err_cap, "Service and redirect URI are required.");
        return -1;
    }
    if (std::strcmp(service, "gdrive") != 0 && std::strcmp(service, "dropbox") != 0) {
        SetErr(err_buf, err_cap, "OAuth browser sign-in only supports gdrive or dropbox.");
        return -1;
    }
    const BbSaveRelayCAbi* resolved = ResolveAbi(abi);
    if (!resolved || !resolved->sign_in_start || !resolved->free_str) {
        SetErr(err_buf, err_cap, "Save Relay C library is not linked on this target.");
        return BB_SAVE_RELAY_OUTCOME_UNAVAILABLE;
    }
    const std::string raw =
        TakeAndFree(resolved, resolved->sign_in_start(service, redirect_uri, device, root));
    JsonValue root_val;
    if (raw.empty() || !JsonParser(raw).Parse(root_val) || root_val.type != JsonValue::Object) {
        SetErr(err_buf, err_cap, "Save Relay returned an invalid sign-in response.");
        return -1;
    }
    const JsonValue* cmd = root_val.Get("command");
    const JsonValue* ok = root_val.Get("ok");
    if (!cmd || cmd->str_val != "setup" || !ok || ok->type != JsonValue::Bool) {
        SetErr(err_buf, err_cap, "Save Relay returned an invalid sign-in response.");
        return -1;
    }
    if (!ok->bool_val) {
        SetErr(err_buf, err_cap, "Save Relay could not start browser sign-in.");
        return -1;
    }
    const JsonValue* data = root_val.Get("data");
    if (!data || data->type != JsonValue::Object) {
        SetErr(err_buf, err_cap, "Save Relay sign-in response omitted challenge data.");
        return -1;
    }
    const JsonValue* url = data->Get("url") ? data->Get("url") : data->Get("verification_url");
    if (!url || url->type != JsonValue::String || url->str_val.empty()) {
        SetErr(err_buf, err_cap, "Save Relay sign-in response omitted verification URL.");
        return -1;
    }
    CopyCString(out_challenge->service, sizeof(out_challenge->service), service);
    CopyCString(out_challenge->verification_url, sizeof(out_challenge->verification_url),
                SanitizeText(url->str_val, 1023));
    const JsonValue* code = data->Get("user_code") ? data->Get("user_code") : data->Get("code");
    if (code && code->type == JsonValue::String) {
        CopyCString(out_challenge->user_code, sizeof(out_challenge->user_code),
                    SanitizeText(code->str_val, 63));
    }
    if (const JsonValue* msg = data->Get("message")) {
        if (msg->type == JsonValue::String) {
            CopyCString(out_challenge->message, sizeof(out_challenge->message),
                        SanitizeText(msg->str_val, 511));
        }
    }
    SetErr(err_buf, err_cap, "");
    return 0;
}

int bb_android_save_relay_sign_in_finish(const char* redirect_url, const BbSaveRelayCAbi* abi,
                                         char* err_buf, size_t err_cap) {
    if (!NonEmpty(redirect_url)) {
        SetErr(err_buf, err_cap, "Redirect URL is required.");
        return -1;
    }
    const BbSaveRelayCAbi* resolved = ResolveAbi(abi);
    if (!resolved || !resolved->sign_in_finish || !resolved->free_str) {
        SetErr(err_buf, err_cap, "Save Relay C library is not linked on this target.");
        return BB_SAVE_RELAY_OUTCOME_UNAVAILABLE;
    }
    const std::string raw = TakeAndFree(resolved, resolved->sign_in_finish(redirect_url));
    return DecodeSimpleEnvelope("setup", raw, "Save Relay could not finish browser sign-in.",
                                err_buf, err_cap);
}

int bb_android_save_relay_download_game_files(const BbSaveRelayGameFilesRequest* req,
                                              const BbSaveRelayCAbi* abi,
                                              BbSaveRelayGameFilesResult* out_result) {
    return RunGameFilesPhase(true, req, abi, out_result);
}

int bb_android_save_relay_game_files_status(const BbSaveRelayGameFilesRequest* req,
                                            const BbSaveRelayCAbi* abi,
                                            BbSaveRelayGameFilesResult* out_result) {
    return RunGameFilesPhase(false, req, abi, out_result);
}
