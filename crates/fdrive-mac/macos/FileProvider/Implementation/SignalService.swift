import FileProvider
import Foundation

final class SignalService {
    private let trigger: () -> Void
    private let lock = NSLock()
    private var attention: Set<NSFileProviderItemIdentifier> = []
    private var heartbeat: Task<Void, Never>?

    init(trigger: @escaping () -> Void) {
        self.trigger = trigger
        heartbeat = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(10))
                self?.beat()
            }
        }
    }

    deinit {
        heartbeat?.cancel()
    }

    private func beat() {
        if !targets().isEmpty {
            trigger()
        }
    }

    func add(_ container: NSFileProviderItemIdentifier) {
        lock.lock()
        attention.insert(container)
        lock.unlock()
        trigger()
    }

    func remove(_ container: NSFileProviderItemIdentifier) {
        lock.lock()
        attention.remove(container)
        lock.unlock()
    }

    func targets() -> [NSFileProviderItemIdentifier] {
        lock.lock()
        defer { lock.unlock() }
        return Array(attention)
    }
}
