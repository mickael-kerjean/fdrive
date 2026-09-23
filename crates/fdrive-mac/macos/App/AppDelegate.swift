import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        Task { @MainActor in
            await AppState.shared.disconnect(withClear: false)
            NSApp.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }
}
