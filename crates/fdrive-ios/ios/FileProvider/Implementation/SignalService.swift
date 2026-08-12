import FileProvider
import Foundation

final class SignalService {
    private let trigger: () -> Void
    private let lock = NSLock()
    private var pending: Set<NSFileProviderItemIdentifier> = []

    init(trigger: @escaping () -> Void) {
        self.trigger = trigger
    }

    func add(_ container: NSFileProviderItemIdentifier) {
        lock.lock()
        pending.insert(container)
        lock.unlock()
        trigger()
    }

    func targets() -> [NSFileProviderItemIdentifier] {
        lock.lock()
        defer { lock.unlock() }
        let all = Array(pending)
        pending.removeAll()
        return all
    }
}
