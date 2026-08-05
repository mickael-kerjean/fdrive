import FileProvider
import OSLog

final class FileProviderExtension: NSObject, NSFileProviderReplicatedExtension, NSFileProviderEnumerating {
    private let logger = Logger(subsystem: "app.filestash.mac.fileprovider", category: "Extension")
    private let adapter: Adapter?

    required init(domain: NSFileProviderDomain) {
        if
            let session = RuntimeSessionStore.load(),
            let dataDirectory = FileManager.default
                .containerURL(forSecurityApplicationGroupIdentifier: "group.app.filestash.sync")?
                .appendingPathComponent("data")
        {
            adapter = try? Adapter(
                url: session.serverURL,
                insecure: session.serverURL.hasPrefix("http://"),
                token: session.token,
                dataDir: dataDirectory.path
            )
        } else {
            adapter = nil
        }
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
            let entry = try adapter.stat(path: identifier.rawValue)
            let parentPath = (identifier.rawValue as NSString).deletingLastPathComponent
            let parent: NSFileProviderItemIdentifier = parentPath.isEmpty || parentPath == "/"
                ? .rootContainer
                : .init(parentPath + "/")
            completionHandler(
                FileProviderItem(path: identifier.rawValue.trimmingCharacters(in: CharacterSet(charactersIn: "/")), parent: parent, entry: entry),
                nil
            )
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
            let path = try adapter.open(path: itemIdentifier.rawValue)
            let entry = try adapter.stat(path: itemIdentifier.rawValue)
            let parentPath = (itemIdentifier.rawValue as NSString).deletingLastPathComponent
            let parent: NSFileProviderItemIdentifier = parentPath.isEmpty || parentPath == "/"
                ? .rootContainer
                : .init(parentPath + "/")
            let item = FileProviderItem(path: itemIdentifier.rawValue, parent: parent, entry: entry)
            completionHandler(URL(fileURLWithPath: path), item, nil)
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
        completionHandler(nil, [], false, CocoaError(.featureUnsupported))
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
        completionHandler(nil, [], false, CocoaError(.featureUnsupported))
        return Progress()
    }

    func deleteItem(
        identifier: NSFileProviderItemIdentifier,
        baseVersion version: NSFileProviderItemVersion,
        options: NSFileProviderDeleteItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping (Error?) -> Void
    ) -> Progress {
        completionHandler(CocoaError(.featureUnsupported))
        return Progress()
    }

    func enumerator(
        for containerItemIdentifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest
    ) throws -> NSFileProviderEnumerator {
        logger.debug("Enumerate \(containerItemIdentifier.rawValue, privacy: .public)")
        guard let adapter else { throw NSFileProviderError(.notAuthenticated) }
        return FileProviderEnumerator(adapter: adapter, container: containerItemIdentifier)
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
