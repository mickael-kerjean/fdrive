import FileProvider

final class MaterializedItemsObserver: NSObject, NSFileProviderEnumerationObserver {
    static func collect(
        from enumerator: NSFileProviderEnumerator,
        completion: @escaping ([NSFileProviderItemIdentifier]) -> Void
    ) {
        let observer = MaterializedItemsObserver(enumerator: enumerator, completion: completion)
        enumerator.enumerateItems(
            for: observer,
            startingAt: NSFileProviderPage(NSFileProviderPage.initialPageSortedByName as Data)
        )
    }

    private let enumerator: NSFileProviderEnumerator
    private let completion: ([NSFileProviderItemIdentifier]) -> Void
    private var identifiers: [NSFileProviderItemIdentifier] = []
    private var retained: MaterializedItemsObserver?

    private init(
        enumerator: NSFileProviderEnumerator,
        completion: @escaping ([NSFileProviderItemIdentifier]) -> Void
    ) {
        self.enumerator = enumerator
        self.completion = completion
        super.init()
        retained = self
    }

    func didEnumerate(_ updatedItems: [NSFileProviderItemProtocol]) {
        identifiers += updatedItems.map(\.itemIdentifier)
    }

    func finishEnumerating(upTo nextPage: NSFileProviderPage?) {
        if let nextPage {
            enumerator.enumerateItems(for: self, startingAt: nextPage)
        } else {
            completion(identifiers)
            retained = nil
        }
    }

    func finishEnumeratingWithError(_ error: Error) {
        completion(identifiers)
        retained = nil
    }
}
