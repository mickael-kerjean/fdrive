import FileProvider

final class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {
    private let adapter: Adapter
    private let container: NSFileProviderItemIdentifier

    init(adapter: Adapter, container: NSFileProviderItemIdentifier) {
        self.adapter = adapter
        self.container = container
    }

    func invalidate() {}

    func enumerateItems(
        for observer: NSFileProviderEnumerationObserver,
        startingAt page: NSFileProviderPage
    ) {
        do {
            let directory = container == .rootContainer || container == .workingSet
                ? "/"
                : container.rawValue
            let parent = container == .workingSet ? .rootContainer : container
            let items = try adapter.ls(path: directory).map { entry in
                FileProviderItem(path: directory + entry.name, parent: parent, entry: entry)
            }
            observer.didEnumerate(items)
            observer.finishEnumerating(upTo: nil)
        } catch {
            observer.finishEnumeratingWithError(mapToProviderError(error))
        }
    }
}
