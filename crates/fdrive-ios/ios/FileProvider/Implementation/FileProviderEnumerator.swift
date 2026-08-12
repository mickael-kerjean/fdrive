import FileProvider

final class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {
    private let adapter: Adapter
    let container: NSFileProviderItemIdentifier
    private let signals: SignalService
    private let metadata: MetadataService
    private var keepalive: DispatchSourceTimer?

    init(
        adapter: Adapter,
        container: NSFileProviderItemIdentifier,
        signals: SignalService,
        metadata: MetadataService,
        tracked: Bool
    ) {
        self.adapter = adapter
        self.container = container
        self.signals = signals
        self.metadata = metadata
        super.init()
        if tracked {
            signals.add(container)
            let timer = DispatchSource.makeTimerSource(queue: .global(qos: .utility))
            timer.schedule(deadline: .now() + 10, repeating: 10)
            timer.setEventHandler { [weak self] in
                guard let self else { return }
                self.signals.add(self.container)
            }
            timer.resume()
            keepalive = timer
        }
    }

    func invalidate() {
        keepalive?.cancel()
        keepalive = nil
    }

    func enumerateItems(
        for observer: NSFileProviderEnumerationObserver,
        startingAt page: NSFileProviderPage
    ) {
        if container == .trashContainer {
            observer.finishEnumerating(upTo: nil)
            return
        }
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                if self.container == .workingSet {
                    let items = try self.list("/")
                    self.metadata.record(items, in: .rootContainer)
                    observer.didEnumerate(items)
                } else {
                    observer.didEnumerate(try self.list(FileProviderPath.path(for: self.container)))
                }
                observer.finishEnumerating(upTo: nil)
            } catch {
                observer.finishEnumeratingWithError(mapToProviderError(error))
            }
        }
    }

    func enumerateChanges(
        for observer: NSFileProviderChangeObserver,
        from syncAnchor: NSFileProviderSyncAnchor
    ) {
        guard container == .workingSet else {
            observer.finishEnumeratingChanges(upTo: metadata.version(), moreComing: false)
            return
        }
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                for target in self.signals.targets() {
                    let items = try self.list(FileProviderPath.path(for: target))
                    let delta = self.metadata.delta(items, in: target)
                    if !delta.updated.isEmpty {
                        observer.didUpdate(delta.updated)
                    }
                    if !delta.deleted.isEmpty {
                        observer.didDeleteItems(withIdentifiers: delta.deleted)
                    }
                }
                observer.finishEnumeratingChanges(upTo: self.metadata.version(), moreComing: false)
            } catch {
                observer.finishEnumeratingWithError(mapToProviderError(error))
            }
        }
    }

    func currentSyncAnchor(completionHandler: @escaping (NSFileProviderSyncAnchor?) -> Void) {
        completionHandler(metadata.version())
    }

    private func list(_ directory: String) throws -> [FileProviderItem] {
        try adapter.ls(path: directory).map { entry in
            let isDirectory = entry.kind == .directory
            let path = FileProviderPath.child(of: directory, name: entry.name, isDirectory: isDirectory)
            return FileProviderItem(path: path, entry: entry)
        }
    }
}
