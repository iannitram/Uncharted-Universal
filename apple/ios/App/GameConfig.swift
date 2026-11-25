import Foundation

/// Which game this app is, read from the Info.plist that apple/build_ios.sh writes from games.json.
/// The three apps are one binary layout with different plists. Code must not depend on the exact
/// bundle ID: SideStore rewrites it with a team suffix when it re-signs the app.
struct GameConfig {
    let game: String
    let number: Int
    let name: String
    let version: String
    let relayManifestName: String

    init(info: [String: Any]) {
        game = info["UUGame"] as? String ?? "u?"
        number = info["UUGameNumber"] as? Int ?? 0
        name = info["CFBundleDisplayName"] as? String ?? "Uncharted"
        version = info["CFBundleShortVersionString"] as? String ?? "0"
        relayManifestName = info["UURelayManifest"] as? String ?? ""
    }

    static var current: GameConfig { GameConfig(info: Bundle.main.infoDictionary ?? [:]) }

    /// The per-game Save Relay manifest copied into the app at build time (never game data).
    func relayManifestJSON(bundle: Bundle = .main) -> String? {
        guard !relayManifestName.isEmpty,
              let url = bundle.url(forResource: relayManifestName, withExtension: nil,
                                   subdirectory: "save-relay-manifests"),
              let text = try? String(contentsOf: url, encoding: .utf8)
        else { return nil }
        return text
    }
}
