import FileProvider
import UniformTypeIdentifiers

final class FileProviderItem: NSObject, NSFileProviderItem {
    static let root = FileProviderItem()

    var itemIdentifier: NSFileProviderItemIdentifier { .rootContainer }
    var parentItemIdentifier: NSFileProviderItemIdentifier { .rootContainer }
    var filename: String { "Filestash" }
    var contentType: UTType { .folder }
    var capabilities: NSFileProviderItemCapabilities { [.allowsReading, .allowsContentEnumerating] }

    var itemVersion: NSFileProviderItemVersion {
        let version = Data([0])
        return NSFileProviderItemVersion(contentVersion: version, metadataVersion: version)
    }
}
