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
    private let breaker: Breaker
    private let activityService: ActivityService?

    required init(domain: NSFileProviderDomain) {
        manager = NSFileProviderManager(for: domain)!
        let session = RuntimeSessionStore.load()
        if session.ok, let dataDirectory = RuntimeSessionStore.dataDirectory {
            adapter = try? Adapter(
                url: session.url,
                insecure: session.insecure,
                token: session.token,
                dataDir: dataDirectory.path
            )
        } else {
            adapter = nil
        }
        breaker = Breaker()
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
        if breaker.isTripped && request.isSystemRequest {
            completionHandler(nil, CocoaError(.userCancelled))
            return Progress()
        }
        let forget: () -> Void = request.isSystemRequest ? breaker.load(Progress()) : {}
        Task {
            defer { forget() }
            do {
                let path = FileProviderPath.path(for: identifier)
                completionHandler(FileProviderItem(path: path, entry: try await adapter.stat(path: path)), nil)
            } catch {
                completionHandler(nil, mapToProviderError(error))
            }
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
        if breaker.isTripped && request.isSystemRequest {
            completionHandler(nil, nil, CocoaError(.userCancelled))
            return Progress()
        }

        let progress = Progress(totalUnitCount: 1)
        let path = FileProviderPath.path(for: itemIdentifier)
        progress.cancellationHandler = { [breaker] in
            adapter.cancel(path: path)
            breaker.trip()
        }
        let forget = breaker.load(progress)
        Task {
            defer { forget() }
            do {
                let localPath = try await adapter.open(path: path)
                let item = FileProviderItem(path: path, entry: try await adapter.stat(path: path))
                completionHandler(URL(fileURLWithPath: localPath), item, nil)
            } catch {
                completionHandler(nil, nil, progress.isCancelled ? CocoaError(.userCancelled) : mapToProviderError(error))
            }
        }
        return progress
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
        Task {
            do {
                if isDirectory {
                    try await adapter.mkdir(path: path)
                } else {
                    let localPath = try adapter.create(path: path)
                    if let url {
                        try await replace(at: localPath, with: url)
                    }
                    adapter.saved(path: path)
                    await adapter.flush(timeoutMs: 30_000)
                }
                let entry = try await adapter.stat(path: path)
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
        Task {
            do {
                if changedFields.contains(.filename) || changedFields.contains(.parentItemIdentifier) {
                    let destination = FileProviderPath.child(
                        of: FileProviderPath.path(for: item.parentItemIdentifier),
                        name: item.filename,
                        isDirectory: isDirectory
                    )
                    try await adapter.rename(from: path, to: destination)
                    path = destination
                }
                if changedFields.contains(.contents), let newContents {
                    let localPath = try await adapter.open(path: path)
                    try await replace(at: localPath, with: newContents)
                    adapter.saved(path: path)
                    await adapter.flush(timeoutMs: 30_000)
                }
                let entry = try await adapter.stat(path: path)
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
        Task {
            do {
                try await adapter.delete(path: path)
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
        let shouldWatch = containerItemIdentifier != .workingSet
            && containerItemIdentifier != .trashContainer
            && request.isFileViewerRequest
        return FileProviderEnumerator(
            adapter: adapter,
            container: containerItemIdentifier,
            signals: signals,
            metadata: metadata,
            shouldWatch: shouldWatch,
            breaker: breaker,
            viewerRequest: request.isFileViewerRequest
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

private func replace(at localPath: String, with contents: URL) async throws {
    try await withCheckedThrowingContinuation { continuation in
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                try replace(at: localPath, with: contents)
                continuation.resume()
            } catch {
                continuation.resume(throwing: error)
            }
        }
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

        Task {
            for identifier in itemIdentifiers {
                do {
                    let thumbnail = try await adapter.thumbnail(path: FileProviderPath.path(for: identifier))
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
        if breaker.underLoad {
            breaker.trip()
            completionHandler(nil)
            return Progress()
        }
        MaterializedItemsObserver.collect(from: manager.enumeratorForMaterializedItems()) { materialized in
            var targets = Set(itemIdentifiers.map { identifier in
                identifier == .rootContainer || identifier.rawValue.hasSuffix("/")
                    ? identifier
                    : FileProviderPath.parent(of: identifier.rawValue)
            })
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
