import AppKit

@MainActor
final class StatusTooltip: NSObject {
    static let shared = StatusTooltip()

    private weak var button: NSStatusBarButton?
    private var polling: Task<Void, Never>?

    func attach() {
        button = NSApp.windows.lazy.compactMap { $0.contentView.flatMap(Self.button) }.first
        button?.addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
    }

    @objc(mouseEntered:)
    func mouseEntered(with event: NSEvent) {
        guard AppState.shared.online, polling == nil else { return }
        polling = Task {
            let activity = ActivityMonitor()
            let update = activity.$meter.sink { meter in
                self.button?.toolTip = "↓ \(meter.rate(\.down))   ↑ \(meter.rate(\.up))"
            }
            await activity.run()
            update.cancel()
        }
    }

    @objc(mouseExited:)
    func mouseExited(with event: NSEvent) {
        polling?.cancel()
        polling = nil
    }

    private static func button(in view: NSView) -> NSStatusBarButton? {
        view as? NSStatusBarButton ?? view.subviews.lazy.compactMap(button).first
    }
}
