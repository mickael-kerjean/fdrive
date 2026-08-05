import FileProvider

final class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {
    private let items: [FileProviderItem]

    init(container: NSFileProviderItemIdentifier) {
        items = container == .workingSet
            ? Array(FileProviderItem.all.dropFirst())
            : FileProviderItem.all.filter { $0.parentItemIdentifier == container && $0.itemIdentifier != .rootContainer }
    }

    func invalidate() {}

    func enumerateItems(
        for observer: NSFileProviderEnumerationObserver,
        startingAt page: NSFileProviderPage
    ) {
        observer.didEnumerate(items)
        observer.finishEnumerating(upTo: nil)
    }
}
