import FileProvider
import Foundation

final class FileProviderMetadataService {
    private let lock = NSLock()
    private var revision: UInt64 = 1

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
