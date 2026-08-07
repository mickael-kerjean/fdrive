import FileProvider
import OSLog

final class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {
    private let logger = Logger(subsystem: "app.filestash.mac.fileprovider", category: "Enumerator")
    private let adapter: Adapter
    private let container: NSFileProviderItemIdentifier
    private let onInvalidate: () -> Void

    init(
        adapter: Adapter,
        container: NSFileProviderItemIdentifier,
        onInvalidate: @escaping () -> Void = {}
    ) {
        self.adapter = adapter
        self.container = container
        self.onInvalidate = onInvalidate
    }

    deinit {
        logger.debug("Enumerator deinit \(self.container.rawValue, privacy: .public)")
    }

    func invalidate() {
        logger.debug("Enumerator invalidate \(self.container.rawValue, privacy: .public)")
        onInvalidate()
    }

    func enumerateItems(
        for observer: NSFileProviderEnumerationObserver,
        startingAt page: NSFileProviderPage
    ) {
        logger.debug("Items \(self.container.rawValue, privacy: .public)")
        do {
            if container == .trashContainer {
                observer.finishEnumerating(upTo: nil)
                return
            }
            let directory = container == .workingSet ? "/" : FileProviderPath.path(for: container)
            let items = try list(directory, recursively: container == .workingSet)
            observer.didEnumerate(items)
            observer.finishEnumerating(upTo: nil)
        } catch {
            logger.error("Listing \(self.container.rawValue, privacy: .public) failed: \(error.localizedDescription, privacy: .public)")
            observer.finishEnumeratingWithError(mapToProviderError(error))
        }
    }

    func enumerateChanges(
        for observer: NSFileProviderChangeObserver,
        from syncAnchor: NSFileProviderSyncAnchor
    ) {
        logger.debug("Changes \(self.container.rawValue, privacy: .public)")
        observer.finishEnumeratingChanges(upTo: Self.anchor, moreComing: false)
    }

    func currentSyncAnchor(
        completionHandler: @escaping (NSFileProviderSyncAnchor?) -> Void
    ) {
        completionHandler(Self.anchor)
    }

    private static let anchor = NSFileProviderSyncAnchor(Data("0".utf8))

    private func list(_ directory: String, recursively: Bool) throws -> [FileProviderItem] {
        var items: [FileProviderItem] = []
        for entry in try adapter.ls(path: directory) {
            let isDirectory = entry.kind == .directory
            let path = FileProviderPath.child(of: directory, name: entry.name, isDirectory: isDirectory)
            items.append(FileProviderItem(path: path, parent: FileProviderPath.parent(of: path), entry: entry))
            if recursively, isDirectory {
                items += try list(path, recursively: true)
            }
        }
        return items
    }
}
