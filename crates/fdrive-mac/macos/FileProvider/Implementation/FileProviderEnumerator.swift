import FileProvider
import OSLog

final class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {
    private let logger = Logger(subsystem: "app.filestash.mac.fileprovider", category: "Enumerator")
    private let adapter: Adapter
    private let container: NSFileProviderItemIdentifier

    init(
        adapter: Adapter,
        container: NSFileProviderItemIdentifier
    ) {
        self.adapter = adapter
        self.container = container
    }

    func invalidate() {}

    func enumerateItems(
        for observer: NSFileProviderEnumerationObserver,
        startingAt page: NSFileProviderPage
    ) {
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
