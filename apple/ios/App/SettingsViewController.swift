import UIKit
import UnchartedRuntimeC

/// Settings stub: facts the owner needs when something goes wrong. Real settings (and the Save Relay
/// sign-in) come with the launcher work; the log file is reachable through the Files app.
final class SettingsViewController: UIViewController {
    private let game: GameConfig

    init(game: GameConfig) {
        self.game = game
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override var prefersStatusBarHidden: Bool { true }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black

        let heading = UILabel()
        heading.text = "Settings"
        heading.font = .systemFont(ofSize: 32, weight: .bold)
        heading.textColor = .white

        let lock = Bundle.main.url(forResource: "components", withExtension: "lock.json")
            .flatMap { try? Data(contentsOf: $0) }
            .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
        let relay = (lock?["save-relay"] as? [String: Any])?["version"] as? String ?? "unknown"

        let facts = UILabel()
        facts.numberOfLines = 0
        facts.font = .monospacedSystemFont(ofSize: 14, weight: .regular)
        facts.textColor = .lightGray
        facts.text = """
        Game: \(game.name)
        App version: \(game.version)
        Runtime ABI: \(uncharted_runtime_version())
        Save Relay: \(relay) (not signed in)
        Log: Files > Uncharted > uncharted.log
        """
        facts.accessibilityIdentifier = "facts"

        var backConfig = UIButton.Configuration.tinted()
        backConfig.title = "Back"
        let back = UIButton(configuration: backConfig, primaryAction: UIAction { [weak self] _ in
            self?.navigationController?.popViewController(animated: true)
        })

        let stack = UIStackView(arrangedSubviews: [heading, facts, back])
        stack.axis = .vertical
        stack.alignment = .leading
        stack.spacing = 16
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 32),
            stack.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 24),
        ])
    }
}
