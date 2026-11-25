import Foundation

/// A plain log file in Documents. The app sets UIFileSharingEnabled, so the file comes back through
/// the Files app or Dropbox (#409). One line per event; the file is cut when it passes 1 MiB.
enum AppLog {
    private static let queue = DispatchQueue(label: "uncharted.log")

    static var url: URL {
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        return documents.appendingPathComponent("uncharted.log")
    }

    static func write(_ message: String) {
        let line = "\(ISO8601DateFormatter().string(from: Date())) \(message)\n"
        queue.async {
            let path = url.path
            if let size = (try? FileManager.default.attributesOfItem(atPath: path))?[.size] as? Int,
               size > 1 << 20 {
                try? FileManager.default.removeItem(atPath: path)
            }
            if !FileManager.default.fileExists(atPath: path) {
                FileManager.default.createFile(atPath: path, contents: nil)
            }
            guard let handle = try? FileHandle(forWritingTo: url),
                  let data = line.data(using: .utf8) else { return }
            defer { try? handle.close() }
            _ = try? handle.seekToEnd()
            try? handle.write(contentsOf: data)
        }
    }
}
