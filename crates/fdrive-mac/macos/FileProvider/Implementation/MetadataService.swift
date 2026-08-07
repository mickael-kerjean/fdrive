import FileProvider
import Foundation

final class MetadataService {
    struct Delta {
        let updated: [FileProviderItem]
        let deleted: [NSFileProviderItemIdentifier]

        var isEmpty: Bool { updated.isEmpty && deleted.isEmpty }
    }

    private let lock = NSLock()
    private var revision: UInt64 = 1
    private var known: [NSFileProviderItemIdentifier: [NSFileProviderItemIdentifier: Data]] = [:]

    func record(_ items: [FileProviderItem], in container: NSFileProviderItemIdentifier) {
        let fresh = versions(items)
        lock.lock()
        known[container] = fresh
        lock.unlock()
    }

    func delta(_ items: [FileProviderItem], in container: NSFileProviderItemIdentifier) -> Delta {
        let fresh = versions(items)
        lock.lock()
        defer { lock.unlock() }
        let previous = known[container] ?? [:]
        let updated = items.filter { previous[$0.itemIdentifier] != $0.itemVersion.contentVersion }
        let deleted = previous.keys.filter { fresh[$0] == nil }
        let delta = Delta(updated: updated, deleted: deleted)
        guard !delta.isEmpty else { return delta }
        known[container] = fresh
        for identifier in deleted where identifier.rawValue.hasSuffix("/") {
            let subtree = identifier.rawValue
            known = known.filter { !$0.key.rawValue.hasPrefix(subtree) }
        }
        revision += 1
        return delta
    }

    func version() -> NSFileProviderSyncAnchor {
        lock.lock()
        defer { lock.unlock() }
        return NSFileProviderSyncAnchor(Data(String(revision).utf8))
    }

    private func versions(_ items: [FileProviderItem]) -> [NSFileProviderItemIdentifier: Data] {
        Dictionary(
            items.map { ($0.itemIdentifier, $0.itemVersion.contentVersion) },
            uniquingKeysWith: { first, _ in first }
        )
    }
}
