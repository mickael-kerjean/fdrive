import FileProvider
import Foundation

let activityServiceName = NSFileProviderServiceName("app.filestash.activity")

@objc(FSActivityProviding)
protocol ActivityProviding {
    func snapshot(reply: @escaping (Data?) -> Void)
}

enum ActivityChannel {
    static func connect(using manager: NSFileProviderManager) async throws -> NSXPCConnection {
        let url = try await manager.getUserVisibleURL(for: .rootContainer)
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let services = try await FileManager.default.fileProviderServicesForItem(at: url)
        guard let source = services[activityServiceName] else {
            throw CocoaError(.featureUnsupported)
        }
        let connection = try await source.fileProviderConnection()
        connection.remoteObjectInterface = NSXPCInterface(with: ActivityProviding.self)
        connection.resume()
        return connection
    }
}

enum TransferDirection: String, Codable {
    case up
    case down
}

enum TransferMode: String, Codable {
    case full
    case delta
}

enum TransferState: String, Codable {
    case running
    case done
    case failed
}

struct Transfer: Codable, Identifiable {
    let id: UInt64
    let path: String
    let direction: TransferDirection
    let mode: TransferMode
    let size: UInt64
    let wire: UInt64
    let progress: UInt64
    let state: TransferState
    let error: String?
}

struct Sample: Codable {
    let up: UInt64
    let down: UInt64
}

struct ActivitySnapshot: Codable {
    let version: UInt64
    let transfers: [Transfer]
    let meter: [Sample]
}
