import FileProvider
import OSLog

final class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {
    private let logger = Logger(subsystem: "app.filestash.mac.fileprovider", category: "Enumerator")
    private let adapter: Adapter
    private let container: NSFileProviderItemIdentifier
    private let signals: SignalService
    private let metadata: MetadataService
    private let onInvalidate: () -> Void

    init(
        adapter: Adapter,
        container: NSFileProviderItemIdentifier,
        signals: SignalService,
        metadata: MetadataService,
        onInvalidate: @escaping () -> Void = {}
    ) {
        self.adapter = adapter
        self.container = container
        self.signals = signals
        self.metadata = metadata
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
            if container == .workingSet {
                let items = try list("/")
                metadata.record(items, in: .rootContainer)
                observer.didEnumerate(items)
            } else {
                observer.didEnumerate(try list(FileProviderPath.path(for: container)))
            }
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
        logger.debug("Changes \(self.container.rawValue, privacy: .public) from=\(String(data: syncAnchor.rawValue, encoding: .utf8) ?? "?", privacy: .public)")
        guard container == .workingSet else {
            observer.finishEnumeratingChanges(upTo: metadata.version(), moreComing: false)
            return
        }
        do {
            let watched = signals.targets()
            logger.debug("Changes reporting \(watched.count) watched containers: \(watched.map(\.rawValue).joined(separator: ","), privacy: .public)")
            for target in watched {
                let items = try list(FileProviderPath.path(for: target))
                let delta = metadata.delta(items, in: target)
                if !delta.updated.isEmpty {
                    observer.didUpdate(delta.updated)
                }
                if !delta.deleted.isEmpty {
                    observer.didDeleteItems(withIdentifiers: delta.deleted)
                }
            }
            observer.finishEnumeratingChanges(upTo: metadata.version(), moreComing: false)
        } catch {
            logger.error("Changes failed: \(error.localizedDescription, privacy: .public)")
            observer.finishEnumeratingWithError(mapToProviderError(error))
        }
    }

    func currentSyncAnchor(
        completionHandler: @escaping (NSFileProviderSyncAnchor?) -> Void
    ) {
        let anchor = metadata.version()
        logger.debug("Anchor requested for \(self.container.rawValue, privacy: .public) -> \(String(data: anchor.rawValue, encoding: .utf8) ?? "?", privacy: .public)")
        completionHandler(anchor)
    }

    private func list(_ directory: String) throws -> [FileProviderItem] {
        try adapter.ls(path: directory).map { entry in
            let isDirectory = entry.kind == .directory
            let path = FileProviderPath.child(of: directory, name: entry.name, isDirectory: isDirectory)
            return FileProviderItem(path: path, parent: FileProviderPath.parent(of: path), entry: entry)
        }
    }
}
