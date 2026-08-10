import FileProvider
import OSLog

final class FileProviderExtension: NSObject, NSFileProviderReplicatedExtension, NSFileProviderEnumerating {
    private let logger = Logger(subsystem: "app.filestash.mac.fileprovider", category: "Extension")
    let manager: NSFileProviderManager
    private let adapter: Adapter?
    private lazy var signals = SignalService(trigger: { [manager, logger] in
        manager.signalEnumerator(for: .workingSet) { error in
            if let error {
                logger.error("Sync request failed: \(error.localizedDescription, privacy: .public)")
            }
        }
    })
    private let metadata = MetadataService()
    private let activityService: ActivityService?

    required init(domain: NSFileProviderDomain) {
        manager = NSFileProviderManager(for: domain)!
        if
            let session = RuntimeSessionStore.load(), session.ok,
            let dataDirectory = RuntimeSessionStore.dataDirectory
        {
            adapter = try? Adapter(
                url: session.url,
                insecure: session.insecure,
                token: session.token,
                dataDir: dataDirectory.path
            )
        } else {
            adapter = nil
        }
        activityService = adapter.map(ActivityService.init(adapter:))
        super.init()
        logger.info("Started domain \(domain.identifier.rawValue, privacy: .public)")
    }

    func invalidate() {}

    func item(
        for identifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, Error?) -> Void
    ) -> Progress {
        logger.debug("Item \(identifier.rawValue, privacy: .public)")
        guard let adapter else {
            completionHandler(nil, NSFileProviderError(.notAuthenticated))
            return Progress()
        }
        if identifier == .rootContainer {
            completionHandler(FileProviderItem.root, nil)
            return Progress()
        }
        do {
            let path = FileProviderPath.path(for: identifier)
            completionHandler(FileProviderItem(path: path, entry: try adapter.stat(path: path)), nil)
        } catch {
            completionHandler(nil, mapToProviderError(error))
        }
        return Progress()
    }

    func fetchContents(
        for itemIdentifier: NSFileProviderItemIdentifier,
        version requestedVersion: NSFileProviderItemVersion?,
        request: NSFileProviderRequest,
        completionHandler: @escaping (URL?, NSFileProviderItem?, Error?) -> Void
    ) -> Progress {
        logger.debug("Fetch \(itemIdentifier.rawValue, privacy: .public)")
        guard let adapter else {
            completionHandler(nil, nil, NSFileProviderError(.notAuthenticated))
            return Progress()
        }

        do {
            let path = FileProviderPath.path(for: itemIdentifier)
            let localPath = try adapter.open(path: path)
            let item = FileProviderItem(path: path, entry: try adapter.stat(path: path))
            completionHandler(URL(fileURLWithPath: localPath), item, nil)
        } catch {
            completionHandler(nil, nil, mapToProviderError(error))
        }
        return Progress()
    }

    func createItem(
        basedOn itemTemplate: NSFileProviderItem,
        fields: NSFileProviderItemFields,
        contents url: URL?,
        options: NSFileProviderCreateItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void
    ) -> Progress {
        guard let adapter else {
            completionHandler(nil, [], false, NSFileProviderError(.notAuthenticated))
            return Progress()
        }
        let isDirectory = itemTemplate.contentType == .folder
        let path = FileProviderPath.child(
            of: FileProviderPath.path(for: itemTemplate.parentItemIdentifier),
            name: itemTemplate.filename,
            isDirectory: isDirectory
        )
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                if isDirectory {
                    try adapter.mkdir(path: path)
                } else {
                    let localPath = try adapter.create(path: path)
                    if let url {
                        try replace(at: localPath, with: url)
                    }
                    adapter.saved(path: path)
                    adapter.flush(timeoutMs: 30_000)
                }
                let entry = try adapter.stat(path: path)
                completionHandler(FileProviderItem(path: path, entry: entry), [], false, nil)
            } catch {
                completionHandler(nil, [], false, mapToProviderError(error))
            }
        }
        return Progress()
    }

    func modifyItem(
        _ item: NSFileProviderItem,
        baseVersion version: NSFileProviderItemVersion,
        changedFields: NSFileProviderItemFields,
        contents newContents: URL?,
        options: NSFileProviderModifyItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void
    ) -> Progress {
        guard let adapter else {
            completionHandler(nil, [], false, NSFileProviderError(.notAuthenticated))
            return Progress()
        }
        var path = FileProviderPath.path(for: item.itemIdentifier)
        let isDirectory = path.hasSuffix("/")
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                if changedFields.contains(.filename) || changedFields.contains(.parentItemIdentifier) {
                    let destination = FileProviderPath.child(
                        of: FileProviderPath.path(for: item.parentItemIdentifier),
                        name: item.filename,
                        isDirectory: isDirectory
                    )
                    try adapter.rename(from: path, to: destination)
                    path = destination
                }
                if changedFields.contains(.contents), let newContents {
                    let localPath = try adapter.open(path: path)
                    try replace(at: localPath, with: newContents)
                    adapter.saved(path: path)
                    adapter.flush(timeoutMs: 30_000)
                }
                let entry = try adapter.stat(path: path)
                completionHandler(FileProviderItem(path: path, entry: entry), [], false, nil)
            } catch {
                completionHandler(nil, [], false, mapToProviderError(error))
            }
        }
        return Progress()
    }

    func deleteItem(
        identifier: NSFileProviderItemIdentifier,
        baseVersion version: NSFileProviderItemVersion,
        options: NSFileProviderDeleteItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping (Error?) -> Void
    ) -> Progress {
        guard let adapter else {
            completionHandler(NSFileProviderError(.notAuthenticated))
            return Progress()
        }
        let path = FileProviderPath.path(for: identifier)
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                try adapter.delete(path: path)
                completionHandler(nil)
            } catch {
                completionHandler(mapToProviderError(error))
            }
        }
        return Progress()
    }

    func enumerator(
        for containerItemIdentifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest
    ) throws -> NSFileProviderEnumerator {
        logger.debug("Enumerate \(containerItemIdentifier.rawValue, privacy: .public) viewer=\(request.isFileViewerRequest) system=\(request.isSystemRequest)")
        guard let adapter else { throw NSFileProviderError(.notAuthenticated) }
        let track = containerItemIdentifier != .workingSet
            && containerItemIdentifier != .trashContainer
            && !request.isSystemRequest
        return FileProviderEnumerator(
            adapter: adapter,
            container: containerItemIdentifier,
            signals: signals,
            metadata: metadata,
            tracked: track
        )
    }
}

private func replace(at localPath: String, with contents: URL) throws {
    let destination = URL(fileURLWithPath: localPath)
    if FileManager.default.fileExists(atPath: localPath) {
        _ = try FileManager.default.replaceItemAt(destination, withItemAt: contents)
    } else {
        try FileManager.default.copyItem(at: contents, to: destination)
    }
}

extension FileProviderExtension: NSFileProviderServicing {
    func supportedServiceSources(
        for itemIdentifier: NSFileProviderItemIdentifier,
        completionHandler: @escaping ([NSFileProviderServiceSource]?, Error?) -> Void
    ) -> Progress {
        completionHandler(activityService.map { [$0] } ?? [], nil)
        return Progress()
    }
}

extension FileProviderExtension: NSFileProviderThumbnailing {
    func fetchThumbnails(
        for itemIdentifiers: [NSFileProviderItemIdentifier],
        requestedSize size: CGSize,
        perThumbnailCompletionHandler: @escaping (NSFileProviderItemIdentifier, Data?, Error?) -> Void,
        completionHandler: @escaping (Error?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: Int64(itemIdentifiers.count))
        guard let adapter else {
            completionHandler(NSFileProviderError(.notAuthenticated))
            return progress
        }

        DispatchQueue.global(qos: .utility).async {
            for identifier in itemIdentifiers {
                do {
                    let thumbnail = try adapter.thumbnail(path: identifier.rawValue)
                    perThumbnailCompletionHandler(identifier, thumbnail, nil)
                } catch {
                    perThumbnailCompletionHandler(identifier, nil, mapToProviderError(error))
                }
                progress.completedUnitCount += 1
            }
            completionHandler(nil)
        }
        return progress
    }
}

extension FileProviderExtension: NSFileProviderCustomAction {
    func performAction(
        identifier actionIdentifier: NSFileProviderExtensionActionIdentifier,
        onItemsWithIdentifiers itemIdentifiers: [NSFileProviderItemIdentifier],
        completionHandler: @escaping (Error?) -> Void
    ) -> Progress {
        logger.info("Action \(actionIdentifier.rawValue, privacy: .public) on \(itemIdentifiers.map(\.rawValue).joined(separator: ","), privacy: .public)")
        MaterializedItemsObserver.collect(from: manager.enumeratorForMaterializedItems()) { materialized in
            var targets = Set(itemIdentifiers)
            for candidate in materialized {
                let isFolder = candidate.rawValue.hasSuffix("/")
                let isUnderSelection = itemIdentifiers.contains { selected in
                    selected == .rootContainer || candidate.rawValue.hasPrefix(selected.rawValue)
                }
                if isFolder && isUnderSelection {
                    targets.insert(candidate)
                }
            }
            for target in targets {
                self.signals.add(target)
            }
            completionHandler(nil)
        }
        return Progress()
    }
}

func mapToProviderError(_ error: Error) -> Error {
    guard let error = error as? FsError else { return error }
    return switch error {
    case .NotAuthenticated, .PermissionDenied, .InvalidCredentials:
        NSFileProviderError(.notAuthenticated)
    case .NotFound:
        NSFileProviderError(.noSuchItem)
    case .Network:
        NSFileProviderError(.serverUnreachable)
    case .Other:
        error
    }
}
