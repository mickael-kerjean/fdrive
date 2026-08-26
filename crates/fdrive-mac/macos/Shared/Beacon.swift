import Foundation

extension UserDefaults {
    @objc dynamic var beacon: Bool { bool(forKey: "beacon") }
}

enum Beacon {
    private static let store = UserDefaults(suiteName: "3736F8X9F9.group.app.filestash.sync")!
    private static let lock = NSLock()
    private static var count = 0

    static func on() -> () -> Void {
        lock.lock()
        count += 1
        if count == 1 {
            store.set(true, forKey: "beacon")
        }
        lock.unlock()
        return {
            DispatchQueue.global().asyncAfter(deadline: .now() + 0.25) {
                lock.lock()
                count -= 1
                if count == 0 {
                    store.set(false, forKey: "beacon")
                }
                lock.unlock()
            }
        }
    }

    static func reset() {
        store.set(false, forKey: "beacon")
    }

    static var active: Bool {
        store.beacon
    }

    static func watch(_ handler: @escaping () -> Void) -> NSKeyValueObservation {
        store.observe(\.beacon) { _, _ in handler() }
    }
}
