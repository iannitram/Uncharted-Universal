import UIKit
import UnchartedRuntimeC

/// Placeholder title screen: the game's name, what is missing, and a way into Settings. The buttons
/// are plain UIButtons so the system focus engine moves between them with a controller
/// (GCSupportsControllerUserInteraction); proper controller navigation is #410.
final class TitleViewController: UIViewController {
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

        let title = UILabel()
        title.text = game.name
        title.font = .systemFont(ofSize: 44, weight: .bold)
        title.textColor = .white
        title.accessibilityIdentifier = "title"

        let note = UILabel()
        note.text = "The game files are not installed yet."
        note.font = .systemFont(ofSize: 18)
        note.textColor = .lightGray
        note.accessibilityIdentifier = "note"

        let abi = UILabel()
        abi.text = "Runtime ABI \(uncharted_runtime_version())  ·  app \(game.version)"
        abi.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        abi.textColor = .gray
        abi.accessibilityIdentifier = "abi"

        var playConfig = UIButton.Configuration.filled()
        playConfig.title = "Play"
        let play = UIButton(configuration: playConfig)
        play.isEnabled = false
        play.accessibilityIdentifier = "play"

        var settingsConfig = UIButton.Configuration.tinted()
        settingsConfig.title = "Settings"
        let settings = UIButton(configuration: settingsConfig, primaryAction: UIAction { [weak self] _ in
            guard let self else { return }
            self.navigationController?.pushViewController(SettingsViewController(game: self.game), animated: true)
        })
        settings.accessibilityIdentifier = "settings"

        let stack = UIStackView(arrangedSubviews: [title, note, play, settings, abi])
        stack.axis = .vertical
        stack.alignment = .center
        stack.spacing = 16
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            stack.centerYAnchor.constraint(equalTo: view.centerYAnchor),
        ])
    }
}
