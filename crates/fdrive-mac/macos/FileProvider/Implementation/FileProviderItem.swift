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
        let path = path.hasSuffix("/") ? String(path.dropLast()) : path
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

    convenience init(path: String, entry: Entry) {
        self.init(path: path, parent: FileProviderPath.parent(of: path), entry: entry)
    }

    var capabilities: NSFileProviderItemCapabilities {
        if itemIdentifier == .rootContainer {
            return [.allowsReading, .allowsWriting]
        }
        return [
            .allowsReading,
            .allowsWriting,
            .allowsReparenting,
            .allowsRenaming,
            .allowsDeleting,
        ]
    }

    var itemVersion: NSFileProviderItemVersion {
        let value = "\(documentSize?.uint64Value ?? 0):\(contentModificationDate?.timeIntervalSince1970 ?? 0)"
        let version = Data(value.utf8)
        return .init(contentVersion: version, metadataVersion: version)
    }
}

enum FileProviderPath {
    static func path(for identifier: NSFileProviderItemIdentifier) -> String {
        identifier == .rootContainer ? "/" : identifier.rawValue
    }

    static func child(of parent: String, name: String, isDirectory: Bool) -> String {
        parent + name + (isDirectory ? "/" : "")
    }

    static func parent(of path: String) -> NSFileProviderItemIdentifier {
        let path = path.hasSuffix("/") ? String(path.dropLast()) : path
        guard let slash = path.lastIndex(of: "/") else { return .rootContainer }
        let parent = String(path[...slash])
        return parent == "/" ? .rootContainer : .init(parent)
    }
}
