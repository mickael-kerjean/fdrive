import FileProvider
import OSLog
import UniformTypeIdentifiers

@available(macOS 26.0, *)
final class FileProviderSearchEnumerator: NSObject, NSFileProviderSearchEnumerator {
    private let logger = Logger(subsystem: "app.filestash.mac.fileprovider", category: "Search")
    private let adapter: Adapter?
    private let query: String
    private var task: Task<Void, Never>?

    init(adapter: Adapter?, query: String) {
        self.adapter = adapter
        self.query = query
        super.init()
    }

    func invalidate() {
        task?.cancel()
    }

    func enumerateSearchResults(
        for observer: NSFileProviderSearchEnumerationObserver,
        startingAt page: NSFileProviderPage?
    ) {
        guard let adapter else {
            observer.finishEnumeratingWithError(NSFileProviderError(.notAuthenticated))
            return
        }
        task = Task { [adapter, query, logger] in
            do {
                let hits = try await adapter.search(query: query)
                guard !Task.isCancelled else { return }
                observer.didEnumerate(hits.prefix(observer.maximumNumberOfResultsPerPage).map(FileProviderSearchResult.init))
                observer.finishEnumerating(upTo: nil)
            } catch {
                logger.error("Search \(query, privacy: .public) failed: \(error.localizedDescription, privacy: .public)")
                observer.finishEnumeratingWithError(mapToProviderError(error))
            }
        }
    }
}

@available(macOS 26.0, *)
final class FileProviderSearchResult: NSObject, NSFileProviderSearchResult {
    let itemIdentifier: NSFileProviderItemIdentifier
    let filename: String
    let creationDate: Date? = nil
    let contentModificationDate: Date?
    let lastUsedDate: Date? = nil
    let contentType: UTType
    let documentSize: NSNumber?

    init(_ hit: SearchHit) {
        let item = FileProviderItem(path: hit.path, parent: FileProviderPath.parent(of: hit.path), entry: hit.entry)
        itemIdentifier = item.itemIdentifier
        filename = item.filename
        contentModificationDate = item.contentModificationDate
        contentType = item.contentType
        documentSize = item.documentSize
    }
}
