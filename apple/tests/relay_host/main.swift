import Foundation

// The RelayHost test (apple/tests/relay_host_test.sh): runs in the iOS Simulator against a local fake
// server and the real Save Relay library. Usage: relay_host_test <port>

var failures = 0
func check(_ condition: Bool, _ label: String) {
    if condition { print("ok: \(label)") } else { print("FAIL: \(label)"); failures += 1 }
}

guard CommandLine.arguments.count == 2, let port = Int(CommandLine.arguments[1]) else {
    print("usage: relay_host_test <port>")
    exit(2)
}
let base = "http://127.0.0.1:\(port)"

// Keychain: set, replace, read, delete, delete again.
let keychain = KeychainCredentials(service: "uu-relay-host-test")
let account = "test-\(UUID().uuidString)"
_ = keychain.deleteSecret(for: account)
check(keychain.secret(for: account) == nil, "keychain: nothing stored yet")
check(keychain.setSecret("first", for: account), "keychain: set (add status \(keychain.lastStatus))")
check(keychain.secret(for: account) == Data("first".utf8), "keychain: read back")
check(keychain.setSecret("second", for: account), "keychain: replace")
check(keychain.secret(for: account) == Data("second".utf8), "keychain: read the replacement")
check(keychain.deleteSecret(for: account), "keychain: delete")
check(keychain.secret(for: account) == nil, "keychain: gone after delete")
check(keychain.deleteSecret(for: account), "keychain: deleting a missing secret succeeds")

// Network: every request runs off the main thread, as RelayHost does.
func send(_ network: URLSessionNetwork, _ request: SaveRelayRequest) -> SaveRelayResponse? {
    var result: SaveRelayResponse?
    let done = DispatchSemaphore(value: 0)
    DispatchQueue.global().async { result = network.respond(to: request); done.signal() }
    done.wait()
    return result
}
func request(_ method: String, _ path: String, headers: String = "", body: String = "", max: Int = 0) -> SaveRelayRequest {
    SaveRelayRequest(method: method, url: base + path, headers: headers, body: Data(body.utf8), maxResponseBytes: max)
}
let network = URLSessionNetwork(timeout: 10)
let ok = send(network, request("GET", "/ok"))
check(ok?.status == 200 && ok?.body == Data("hello".utf8), "network: GET 200 and body")
check(ok?.headers?.lowercased().contains("x-answer: yes") == true, "network: response headers come back as Name: value lines")
check(send(network, request("GET", "/missing"))?.status == 404, "network: a 404 is an answer, not a failure")
check(send(network, request("GET", "/big", max: 100)) == nil, "network: an answer over max_response_bytes fails")
check(send(network, request("GET", "/big", max: 5000))?.body.count == 5000, "network: an answer at the bound passes")
check(send(network, request("GET", "/big"))?.body.count == 5000, "network: bound 0 means no bound")
func echo(_ response: SaveRelayResponse?) -> [String: Any] {
    (response.flatMap { try? JSONSerialization.jsonObject(with: $0.body) } as? [String: Any]) ?? [:]
}
let bodiless = echo(send(network, request("POST", "/echo")))
check(bodiless["content_type"] as? String == "application/octet-stream", "network: a bodiless POST is octet-stream")
check(bodiless["accept_encoding"] as? String == "identity", "network: Accept-Encoding is identity")
let withBody = echo(send(network, request("POST", "/echo", headers: "X-Test: abc\nContent-Type: text/x-test\n", body: "payload")))
check(withBody["body"] as? String == "payload", "network: the body is sent")
check(withBody["x_test"] as? String == "abc", "network: request headers are sent")
check(withBody["content_type"] as? String == "text/x-test", "network: an explicit Content-Type is kept")
let dead = URLSessionNetwork(timeout: 3)
check(send(dead, SaveRelayRequest(method: "GET", url: "http://127.0.0.1:1/", headers: "", body: Data(), maxResponseBytes: 0)) == nil,
      "network: a refused connection is a failure (nil)")
check(network.respond(to: request("GET", "/ok")) == nil, "network: calling on the main thread fails instead of deadlocking")

// The real Save Relay library over this host: init, then a Nextcloud setup against the fake server.
// The fake server is not a WebDAV server, so setup fails; what matters is that the library answered in
// its JSON envelope, and that the request it made reached the server through URLSession.
let root = FileManager.default.temporaryDirectory.appendingPathComponent("uu-relay-host-\(UUID().uuidString)")
try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
let memory = MemoryCredentials()
let host = RelayHost(root: root, credentials: memory, network: URLSessionNetwork(timeout: 10))
func isEnvelope(_ text: String) -> Bool {
    (try? JSONSerialization.jsonObject(with: Data(text.utf8))) is [String: Any]
}
check(isEnvelope(host.initEnvelope), "relay: save_relay_init answers a JSON envelope")
let setupDone = DispatchSemaphore(value: 0)
var setupAnswer = ""
DispatchQueue.global().async {
    setupAnswer = host.relay.setup(server: "\(base)/remote.php/dav/files/u", user: "u", password: "secret-password")
    setupDone.signal()
}
setupDone.wait()
check(isEnvelope(setupAnswer), "relay: setup answers a JSON envelope")
check(!setupAnswer.contains("secret-password"), "relay: the password is not in the envelope")
let seenAnswer = send(network, request("GET", "/requests"))
let seen = (seenAnswer.flatMap { try? JSONSerialization.jsonObject(with: $0.body) } as? [String]) ?? []
check(seen.contains { $0.contains("/remote.php/dav/files/u") }, "relay: Save Relay's request reached the server through URLSession (\(seen.count) seen)")
let statusDone = DispatchSemaphore(value: 0)
var statusAnswer = ""
host.status(manifestJSON: "{\"format\":1,\"id\":\"t\",\"name\":\"t\",\"saves\":{},\"saves_from\":\"launcher\",\"include\":[\"**\"],\"exclude\":[]}",
            savesDir: root.path) { statusAnswer = $0; statusDone.signal() }
statusDone.wait()
check(isEnvelope(statusAnswer), "relay: status answers a JSON envelope through RelayHost's queue")
print("setup answer: \(setupAnswer.prefix(300))")
print("status answer: \(statusAnswer.prefix(300))")
try? FileManager.default.removeItem(at: root)

/// A credential store for the test, so it does not depend on the Simulator's Keychain.
final class MemoryCredentials: SaveRelayCredentials {
    private var items: [String: Data] = [:]
    func secret(for account: String) -> Data? { items[account] }
    func setSecret(_ secret: String, for account: String) -> Bool { items[account] = Data(secret.utf8); return true }
    func deleteSecret(for account: String) -> Bool { items[account] = nil; return true }
}

print(failures == 0 ? "all relay host tests passed" : "\(failures) relay host test(s) failed")
exit(failures == 0 ? 0 : 1)
