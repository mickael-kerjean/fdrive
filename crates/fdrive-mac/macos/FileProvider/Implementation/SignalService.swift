import FileProvider
import Foundation

final class SignalService {
    private let trigger: () -> Void
    private let lock = NSLock()
    private var attention: Set<NSFileProviderItemIdentifier> = []

    init(trigger: @escaping () -> Void) {
        self.trigger = trigger
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
