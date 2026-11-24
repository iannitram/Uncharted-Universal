import UIKit
import UnchartedRuntimeC

// Entry point. SDL3 normally owns UIApplicationMain on iOS; when the runtime's SDL host is linked
// (the pad and window work of #410 and 8.05), either this file keeps `@main` and the runtime calls
// SDL_SetMainReady, or SDL owns `main` and calls back into AppDelegate. That is decided with that
// work; until then this is a plain UIKit shell.
@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?

    func application(_ application: UIApplication,
                     didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        let game = GameConfig.current
        AppLog.write("launch \(game.game) \(game.version) runtime-abi \(uncharted_runtime_version())")
        let window = UIWindow(frame: UIScreen.main.bounds)
        let navigation = UINavigationController(rootViewController: TitleViewController(game: game))
        navigation.isNavigationBarHidden = true
        window.rootViewController = navigation
        window.makeKeyAndVisible()
        self.window = window
        return true
    }

    func application(_ application: UIApplication,
                     supportedInterfaceOrientationsFor window: UIWindow?) -> UIInterfaceOrientationMask {
        .landscape
    }
}
