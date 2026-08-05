import FileProvider
import UniformTypeIdentifiers

final class FileProviderItem: NSObject, NSFileProviderItem {
    static let root = FileProviderItem()

    let itemIdentifier: NSFileProviderItemIdentifier
    let parentItemIdentifier: NSFileProviderItemIdentifier
    let filename: String
    let contentType: UTType
    let documentSize: NSNumber?
    let contentModificationDate: Date?

    private override init() {
        itemIdentifier = .rootContainer
        parentItemIdentifier = .rootContainer
        filename = "Filestash"
        contentType = .folder
        documentSize = nil
        contentModificationDate = nil
    }

    init(path: String, parent: NSFileProviderItemIdentifier, entry: Entry) {
        let isDirectory = entry.kind == .directory
        let identifier = isDirectory ? path + "/" : path
        itemIdentifier = .init(identifier)
        parentItemIdentifier = parent
        filename = entry.name
        contentType = isDirectory
            ? .folder
            : UTType(filenameExtension: (entry.name as NSString).pathExtension) ?? .data
        documentSize = entry.size.map(NSNumber.init(value:))
        contentModificationDate = entry.mtimeMs.map { Date(timeIntervalSince1970: Double($0) / 1000) }
    }

    var capabilities: NSFileProviderItemCapabilities {
        contentType == .folder ? [.allowsReading, .allowsContentEnumerating] : [.allowsReading]
    }

    var itemVersion: NSFileProviderItemVersion {
        let value = "\(documentSize?.uint64Value ?? 0):\(contentModificationDate?.timeIntervalSince1970 ?? 0)"
        let version = Data(value.utf8)
        return .init(contentVersion: version, metadataVersion: version)
    }
}
