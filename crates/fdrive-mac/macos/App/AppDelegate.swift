import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        Task { @MainActor in StatusTooltip.shared.attach() }
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        Task { @MainActor in
            await AppState.shared.disconnect(withClear: false)
            NSApp.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }
}
