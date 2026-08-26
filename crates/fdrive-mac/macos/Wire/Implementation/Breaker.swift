import Foundation

final class Breaker {
    private let lock = NSLock()
    private var lastTrip = Date.distantPast
    private var lastLoad = Date.distantPast
    private var active: [ObjectIdentifier: Progress] = [:]

    func trip() {
        lock.lock()
        lastTrip = Date()
        let transfers = Array(active.values)
        lock.unlock()
        for progress in transfers {
            progress.cancel()
        }
    }

    var isTripped: Bool {
        lock.lock()
        defer { lock.unlock() }
        return Date().timeIntervalSince(lastTrip) < 10
    }

    func load(_ progress: Progress) -> () -> Void {
        let key = ObjectIdentifier(progress)
        lock.lock()
        lastLoad = Date()
        active[key] = progress
        lock.unlock()
        return {
            self.lock.lock()
            self.lastLoad = Date()
            self.active.removeValue(forKey: key)
            self.lock.unlock()
        }
    }

    var underLoad: Bool {
        lock.lock()
        defer { lock.unlock() }
        return !active.isEmpty || Date().timeIntervalSince(lastLoad) < 5
    }
}
