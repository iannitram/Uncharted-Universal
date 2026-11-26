import Foundation
import Security

// The app's side of the Save Relay C interface (bindings/ios/SaveRelay.swift, save-relay
// v1.0.0-rc.3): the Keychain keeps the credential, URLSession carries the requests. Nothing here
// imports UIKit, so the unit test (apple/tests/relay_host_test.sh) builds it for the Simulator
// without the app.

/// Keeps Save Relay's secrets as Keychain generic passwords. A secret never leaves this store: it
/// is not written to a file, a log or UserDefaults. `ThisDeviceOnly` keeps it out of backups.
final class KeychainCredentials: SaveRelayCredentials {
    private let service: String
    /// The OSStatus of the last write, for tests and the log.
    private(set) var lastStatus: OSStatus = errSecSuccess

    init(service: String = "save-relay") { self.service = service }

    private func query(_ account: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account]
    }

    func secret(for account: String) -> Data? {
        var request = query(account)
        request[kSecReturnData as String] = true
        request[kSecMatchLimit as String] = kSecMatchLimitOne
        var found: CFTypeRef?
        guard SecItemCopyMatching(request as CFDictionary, &found) == errSecSuccess else { return nil }
        return found as? Data
    }

    func setSecret(_ secret: String, for account: String) -> Bool {
        let data = Data(secret.utf8)
        let update = [kSecValueData as String: data]
        let status = SecItemUpdate(query(account) as CFDictionary, update as CFDictionary)
        lastStatus = status
        if status == errSecSuccess { return true }
        guard status == errSecItemNotFound else { return false }
        var add = query(account)
        add[kSecValueData as String] = data
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        lastStatus = SecItemAdd(add as CFDictionary, nil)
        return lastStatus == errSecSuccess
    }

    func deleteSecret(for account: String) -> Bool {
        let status = SecItemDelete(query(account) as CFDictionary)
        return status == errSecSuccess || status == errSecItemNotFound
    }
}

/// Sends Save Relay's requests with URLSession. The C interface is synchronous, so `respond` blocks
/// on a semaphore until the delegate callbacks finish; call Save Relay only from a background
/// queue (`RelayHost` does). On the main thread it fails the request instead of deadlocking.
final class URLSessionNetwork: NSObject, SaveRelayNetwork, URLSessionDataDelegate {
    private let timeout: TimeInterval
    private lazy var session: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = timeout
        configuration.timeoutIntervalForResource = timeout * 4
        configuration.httpShouldSetCookies = false
        configuration.httpCookieStorage = nil
        configuration.urlCredentialStorage = nil
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        return URLSession(configuration: configuration, delegate: self, delegateQueue: queue)
    }()

    /// What one in-flight request has collected. The delegate queue is serial, so no lock is needed
    /// beyond the semaphore handover.
    private final class Transfer {
        var status: UInt16 = 0
        var headers = ""
        var body = Data()
        var limit = 0
        var tooLarge = false
        var failed = false
        let done = DispatchSemaphore(value: 0)
    }
    private var transfers: [Int: Transfer] = [:]
    private let lock = NSLock()

    init(timeout: TimeInterval = 60) {
        self.timeout = timeout
        super.init()
    }

    func respond(to request: SaveRelayRequest) -> SaveRelayResponse? {
        if Thread.isMainThread { return nil }
        guard let url = URL(string: request.url) else { return nil }
        var outgoing = URLRequest(url: url)
        outgoing.httpMethod = request.method
        var hasContentType = false
        for line in request.headers.split(separator: "\n") {
            guard let colon = line.firstIndex(of: ":") else { continue }
            let name = String(line[..<colon]).trimmingCharacters(in: .whitespaces)
            let value = String(line[line.index(after: colon)...]).trimmingCharacters(in: .whitespaces)
            if name.lowercased() == "content-type" { hasContentType = true }
            outgoing.setValue(value, forHTTPHeaderField: name)
        }
        // Whole answers are buffered by Save Relay, so ask for no compression and keep the bytes exact.
        outgoing.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
        if !request.body.isEmpty {
            outgoing.httpBody = request.body
        } else if request.method.uppercased() == "POST", !hasContentType {
            // URLSession would otherwise label a bodiless POST as a form.
            outgoing.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
        }
        let transfer = Transfer()
        transfer.limit = request.maxResponseBytes
        let task = session.dataTask(with: outgoing)
        lock.lock(); transfers[task.taskIdentifier] = transfer; lock.unlock()
        task.resume()
        transfer.done.wait()
        lock.lock(); transfers[task.taskIdentifier] = nil; lock.unlock()
        if transfer.failed || transfer.tooLarge { return nil }
        return SaveRelayResponse(status: transfer.status, headers: transfer.headers, body: transfer.body)
    }

    private func transfer(for task: URLSessionTask) -> Transfer? {
        lock.lock(); defer { lock.unlock() }
        return transfers[task.taskIdentifier]
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask,
                    didReceive response: URLResponse,
                    completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
        guard let http = response as? HTTPURLResponse, let transfer = transfer(for: dataTask) else {
            completionHandler(.cancel)
            return
        }
        transfer.status = UInt16(clamping: http.statusCode)
        transfer.headers = http.allHeaderFields
            .map { key, value in "\(key): \(value)\n" }
            .joined()
        completionHandler(.allow)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        guard let transfer = transfer(for: dataTask) else { return }
        transfer.body.append(data)
        if transfer.limit > 0 && transfer.body.count > transfer.limit {
            transfer.tooLarge = true
            dataTask.cancel()
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        guard let transfer = transfer(for: task) else { return }
        if error != nil && !transfer.tooLarge { transfer.failed = true }
        transfer.done.signal()
    }
}

/// Save Relay for one game's app: its own folders, Keychain and network, and a serial queue so the
/// blocking C calls never run on the main thread. Each app keeps its own sign-in (the three apps
/// share no container).
final class RelayHost {
    let relay: SaveRelay
    private let queue = DispatchQueue(label: "uncharted.relay")

    init(root: URL, credentials: SaveRelayCredentials = KeychainCredentials(),
         network: SaveRelayNetwork = URLSessionNetwork()) {
        let data = root.appendingPathComponent("relay-data").path
        let config = root.appendingPathComponent("relay-config").path
        for folder in [data, config] {
            try? FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
        }
        relay = SaveRelay(home: root.path, data: data, config: config, documents: nil,
                          credentials: credentials, network: network)
    }

    /// The envelope `save_relay_init` answered, for logging.
    var initEnvelope: String { relay.initialized }

    func status(manifestJSON: String, savesDir: String, completion: @escaping (String) -> Void) {
        queue.async { completion(self.relay.status(manifestJson: manifestJSON, savesDir: savesDir)) }
    }

    func pull(manifestJSON: String, savesDir: String, resolve: String? = nil,
              completion: @escaping (String) -> Void) {
        queue.async { completion(self.relay.pull(manifestJson: manifestJSON, savesDir: savesDir, resolve: resolve)) }
    }

    func push(manifestJSON: String, savesDir: String, resolve: String? = nil,
              completion: @escaping (String) -> Void) {
        queue.async { completion(self.relay.push(manifestJson: manifestJSON, savesDir: savesDir, resolve: resolve)) }
    }
}
