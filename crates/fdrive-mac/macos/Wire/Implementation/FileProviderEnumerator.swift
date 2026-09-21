import FileProvider
import OSLog

final class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {
    private let logger = Logger(subsystem: "app.filestash.mac.fileprovider", category: "Enumerator")
    private let adapter: Adapter
    private let manager: NSFileProviderManager
    let container: NSFileProviderItemIdentifier
    private let signals: SignalService
    private let metadata: MetadataService
    private let breaker: Breaker
    private let viewerRequest: Bool

    init(
        adapter: Adapter,
        manager: NSFileProviderManager,
        container: NSFileProviderItemIdentifier,
        signals: SignalService,
        metadata: MetadataService,
        shouldWatch: Bool,
        breaker: Breaker,
        viewerRequest: Bool
    ) {
        self.adapter = adapter
        self.manager = manager
        self.container = container
        self.signals = signals
        self.metadata = metadata
        self.breaker = breaker
        self.viewerRequest = viewerRequest
        super.init()
        if shouldWatch {
            signals.add(container)
            Task { [weak self] in
                while true {
                    try? await Task.sleep(for: .seconds(10))
                    guard let self else { return }
                    self.signals.add(self.container)
                }
            }
        }
    }

    deinit {
        logger.debug("Enumerator deinit \(self.container.rawValue, privacy: .public)")
    }

    func invalidate() {
        logger.debug("Enumerator invalidate \(self.container.rawValue, privacy: .public)")
    }

    func enumerateItems(
        for observer: NSFileProviderEnumerationObserver,
        startingAt page: NSFileProviderPage
    ) {
        logger.debug("Items \(self.container.rawValue, privacy: .public)")
        if container == .trashContainer {
            observer.finishEnumerating(upTo: nil)
            return
        }
        if breaker.isTripped && !viewerRequest {
            observer.finishEnumeratingWithError(CocoaError(.userCancelled))
            return
        }
        let forget: () -> Void = viewerRequest ? {} : breaker.load(Progress())
        Task {
            defer { forget() }
            do {
                let directory = container == .workingSet ? "/" : FileProviderPath.path(for: container)
                var items = try await list(directory)
                // TODO: Add pagination that handles directory changes between pages.
                if items.count > 20_000 {
                    logger.warning("Listing \(directory, privacy: .public) truncated to 20000 of \(items.count) items; pagination is not implemented")
                    items = Array(items.prefix(20_000))
                }
                if container == .workingSet {
                    metadata.record(items, in: .rootContainer)
                }
                observer.didEnumerate(items)
                observer.finishEnumerating(upTo: nil)
            } catch {
                logger.error("Listing \(self.container.rawValue, privacy: .public) failed: \(error.localizedDescription, privacy: .public)")
                observer.finishEnumeratingWithError(mapToProviderError(error))
            }
        }
    }

    func enumerateChanges(
        for observer: NSFileProviderChangeObserver,
        from syncAnchor: NSFileProviderSyncAnchor
    ) {
        logger.debug("Changes \(self.container.rawValue, privacy: .public) from=\(String(data: syncAnchor.rawValue, encoding: .utf8) ?? "?", privacy: .public)")
        if breaker.isTripped {
            observer.finishEnumeratingChanges(upTo: metadata.version(), moreComing: false)
            return
        }
        guard container == .workingSet else {
            observer.finishEnumeratingChanges(upTo: metadata.version(), moreComing: false)
            return
        }
        Task {
            do {
                let watched = signals.targets()
                var downloads = Set<NSFileProviderItemIdentifier>()
                logger.debug("Changes reporting \(watched.count) watched containers: \(watched.map(\.rawValue).joined(separator: ","), privacy: .public)")
                for target in watched {
                    let items = try await list(FileProviderPath.path(for: target))
                    let delta = metadata.delta(items, in: target)
                    if !delta.updated.isEmpty {
                        downloads.formUnion(await downloadedFiles(in: delta.updated))
                        logger.info("Delta \(target.rawValue, privacy: .public): updated \(delta.updated.map(\.filename).joined(separator: ","), privacy: .public)")
                        observer.didUpdate(delta.updated)
                    }
                    if !delta.deleted.isEmpty {
                        logger.info("Delta \(target.rawValue, privacy: .public): deleted \(delta.deleted.count)")
                        observer.didDeleteItems(withIdentifiers: delta.deleted)
                    }
                }
                observer.finishEnumeratingChanges(upTo: metadata.version(), moreComing: false)
                await requestUpdates(for: downloads)
            } catch {
                logger.error("Changes failed: \(error.localizedDescription, privacy: .public)")
                observer.finishEnumeratingWithError(mapToProviderError(error))
            }
        }
    }

    func currentSyncAnchor(
        completionHandler: @escaping (NSFileProviderSyncAnchor?) -> Void
    ) {
        let anchor = metadata.version()
        logger.debug("Anchor requested for \(self.container.rawValue, privacy: .public) -> \(String(data: anchor.rawValue, encoding: .utf8) ?? "?", privacy: .public)")
        completionHandler(anchor)
    }

    private func list(_ directory: String) async throws -> [FileProviderItem] {
        try await adapter.ls(path: directory, viewerRequest: true).map { entry in
            let isDirectory = entry.kind == .directory
            let path = FileProviderPath.child(of: directory, name: entry.name, isDirectory: isDirectory)
            return FileProviderItem(path: path, entry: entry)
        }
    }

    private func downloadedFiles(in items: [FileProviderItem]) async -> [NSFileProviderItemIdentifier] {
        var downloaded: [NSFileProviderItemIdentifier] = []
        for item in items where item.contentType != .folder {
            do {
                let url = try await manager.getUserVisibleURL(for: item.itemIdentifier)
                let values = try url.resourceValues(forKeys: [.ubiquitousItemDownloadingStatusKey])
                switch values.ubiquitousItemDownloadingStatus {
                case .current, .downloaded:
                    downloaded.append(item.itemIdentifier)
                default:
                    break
                }
            } catch {
                logger.debug("Download state unavailable for \(item.itemIdentifier.rawValue, privacy: .public): \(error.localizedDescription, privacy: .public)")
            }
        }
        return downloaded
    }

    private func requestUpdates(for identifiers: Set<NSFileProviderItemIdentifier>) async {
        for identifier in identifiers {
            do {
                guard try await waitForRemoteUpdate(identifier) else { continue }
                logger.info("Request content update \(identifier.rawValue, privacy: .public)")
                try await manager.requestDownloadForItem(withIdentifier: identifier)
            } catch {
                logger.error("Request content update \(identifier.rawValue, privacy: .public) failed: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    private func waitForRemoteUpdate(_ identifier: NSFileProviderItemIdentifier) async throws -> Bool {
        var url = try await manager.getUserVisibleURL(for: identifier)
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: .seconds(2))
        while clock.now < deadline {
            url.removeAllCachedResourceValues()
            let values = try url.resourceValues(forKeys: [.ubiquitousItemDownloadingStatusKey])
            switch values.ubiquitousItemDownloadingStatus {
            case .downloaded, .notDownloaded:
                return true
            case .current:
                try await Task.sleep(for: .milliseconds(50))
            default:
                return false
            }
        }
        logger.debug("Content still current after enumeration \(identifier.rawValue, privacy: .public)")
        return false
    }
}
