import FileProvider
import UniformTypeIdentifiers

final class FileProviderItem: NSObject, NSFileProviderItem {
    static let root = FileProviderItem(.rootContainer, parent: .rootContainer, name: "Filestash", type: .folder)
    static let documents = FileProviderItem("documents", parent: .rootContainer, name: "Documents", type: .folder)
    static let welcome = FileProviderItem(
        "welcome.txt",
        parent: .rootContainer,
        name: "Welcome.txt",
        contents: "Welcome to Filestash!\n"
    )
    static let notes = FileProviderItem(
        "documents/notes.txt",
        parent: documents.itemIdentifier,
        name: "Notes.txt",
        contents: "This file came from the Filestash File Provider.\n"
    )

    static let all = [root, documents, welcome, notes]

    let itemIdentifier: NSFileProviderItemIdentifier
    let parentItemIdentifier: NSFileProviderItemIdentifier
    let filename: String
    let contentType: UTType
    let contents: Data?

    init(
        _ identifier: NSFileProviderItemIdentifier,
        parent: NSFileProviderItemIdentifier,
        name: String,
        type: UTType = .plainText,
        contents: String? = nil
    ) {
        itemIdentifier = identifier
        parentItemIdentifier = parent
        filename = name
        contentType = type
        self.contents = contents?.data(using: .utf8)
    }

    convenience init(
        _ identifier: String,
        parent: NSFileProviderItemIdentifier,
        name: String,
        type: UTType = .plainText,
        contents: String? = nil
    ) {
        self.init(.init(identifier), parent: parent, name: name, type: type, contents: contents)
    }

    var capabilities: NSFileProviderItemCapabilities {
        contentType == .folder ? [.allowsReading, .allowsContentEnumerating] : [.allowsReading]
    }

    var documentSize: NSNumber? { contents.map { NSNumber(value: $0.count) } }

    var itemVersion: NSFileProviderItemVersion {
        let version = Data([0])
        return .init(contentVersion: version, metadataVersion: version)
    }
}
