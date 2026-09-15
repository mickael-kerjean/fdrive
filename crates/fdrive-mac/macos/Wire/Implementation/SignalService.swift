import FileProvider
import Foundation

final class SignalService: RemoteObserver, @unchecked Sendable {
    private let trigger: @Sendable () -> Void
    private let lock = NSLock()
    private var pending: Set<NSFileProviderItemIdentifier> = []

    init(trigger: @escaping @Sendable () -> Void) {
        self.trigger = trigger
    }

    func add(_ container: NSFileProviderItemIdentifier) {
        lock.lock()
        pending.insert(container)
        lock.unlock()
        trigger()
    }

    func changed(directories: [String]) {
        lock.lock()
        for directory in directories {
            pending.insert(directory == "/" ? .rootContainer : .init(directory))
        }
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
