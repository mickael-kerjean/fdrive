import FileProvider
import Foundation

final class RemoteChangeMonitor {
    private let lock = NSLock()
    private var snapshots: [NSFileProviderItemIdentifier: Set<NSFileProviderItemIdentifier>] = [:]
    private var revision: UInt64 = 0

    func record(
        _ items: [FileProviderItem],
        in container: NSFileProviderItemIdentifier
    ) -> (deleted: [NSFileProviderItemIdentifier], anchor: NSFileProviderSyncAnchor) {
        let current = Set(items.map(\.itemIdentifier))
        lock.lock()
        let deleted = Array((snapshots[container] ?? []).subtracting(current))
        snapshots[container] = current
        revision += 1
        let anchor = NSFileProviderSyncAnchor(Data(String(revision).utf8))
        lock.unlock()
        return (deleted, anchor)
    }

    func currentAnchor() -> NSFileProviderSyncAnchor {
        lock.lock()
        defer { lock.unlock() }
        revision += 1
        return NSFileProviderSyncAnchor(Data(String(revision).utf8))
    }
}
