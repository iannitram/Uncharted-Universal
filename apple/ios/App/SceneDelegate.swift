import UIKit

/// Owns the window. The class name is fixed with @objc so Info.plist can name it whatever the module
/// is called in each of the three apps.
@objc(SceneDelegate)
final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession,
               options connectionOptions: UIScene.ConnectionOptions) {
        guard let windowScene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: windowScene)
        let navigation = UINavigationController(rootViewController: TitleViewController(game: GameConfig.current))
        navigation.isNavigationBarHidden = true
        window.rootViewController = navigation
        window.makeKeyAndVisible()
        self.window = window
    }
}
