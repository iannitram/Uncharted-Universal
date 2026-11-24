import UIKit
import UnchartedRuntimeC

// Entry point. The window comes from SceneDelegate: iOS 27 stops an app without the UIScene lifecycle.
// SDL3 normally owns UIApplicationMain on iOS. When the runtime's SDL host is linked (the pad and window
// work of #410 and 8.05), either this file keeps the main attribute and the runtime calls
// SDL_SetMainReady, or SDL owns main() and calls back into AppDelegate. That is decided with that work;
// until then this is a plain UIKit shell.
@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication,
                     didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        let game = GameConfig.current
        AppLog.write("launch \(game.game) \(game.version) runtime-abi \(uncharted_runtime_version())")
        return true
    }

    func application(_ application: UIApplication,
                     supportedInterfaceOrientationsFor window: UIWindow?) -> UIInterfaceOrientationMask {
        .landscape
    }
}
