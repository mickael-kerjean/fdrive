import FileProvider
import Foundation

final class MetadataService {
    private let lock = NSLock()
    private var revision: UInt64 = 1
    private var known: [NSFileProviderItemIdentifier: [NSFileProviderItemIdentifier: Data]] = [:]

    func record(_ items: [FileProviderItem], in container: NSFileProviderItemIdentifier) {
        let versions = Dictionary(
            items.map { ($0.itemIdentifier, $0.itemVersion.contentVersion) },
            uniquingKeysWith: { first, _ in first }
        )
        lock.lock()
        known[container] = versions
        lock.unlock()
    }

    func version() -> NSFileProviderSyncAnchor {
        lock.lock()
        defer { lock.unlock() }
        return NSFileProviderSyncAnchor(Data(String(revision).utf8))
    }

    func advance() {
        lock.lock()
        revision += 1
        lock.unlock()
    }
}
