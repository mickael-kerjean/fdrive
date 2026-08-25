import Foundation

final class Breaker {
    private let lock = NSLock()
    private var holdUntil = Date.distantPast
    private var active: [ObjectIdentifier: Progress] = [:]

    var isTripped: Bool {
        lock.lock()
        defer { lock.unlock() }
        return Date() < holdUntil
    }

    func trip() {
        lock.lock()
        holdUntil = Date().addingTimeInterval(30)
        let transfers = Array(active.values)
        lock.unlock()
        for progress in transfers {
            progress.cancel()
        }
    }

    func track(_ progress: Progress) {
        lock.lock()
        active[ObjectIdentifier(progress)] = progress
        lock.unlock()
    }

    func untrack(_ progress: Progress) {
        lock.lock()
        active.removeValue(forKey: ObjectIdentifier(progress))
        lock.unlock()
    }
}
