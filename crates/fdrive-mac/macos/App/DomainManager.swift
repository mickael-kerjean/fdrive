import AppKit
import FileProvider

enum DomainManager {
    private static let domain = NSFileProviderDomain(
        identifier: NSFileProviderDomainIdentifier(rawValue: "filestash"),
        displayName: ""
    )

    static func add() async throws {
        try? await NSFileProviderManager.remove(domain)
        try await NSFileProviderManager.add(domain)
    }

    static func remove() async throws {
        try await NSFileProviderManager.remove(domain)
    }

    static func connectActivity() async throws -> NSXPCConnection {
        guard let manager = NSFileProviderManager(for: domain) else {
            throw CocoaError(.fileNoSuchFile)
        }
        return try await ActivityChannel.connect(using: manager)
    }

    static func open() async throws {
        guard let manager = NSFileProviderManager(for: domain) else {
            throw CocoaError(.fileNoSuchFile)
        }
        let url = try await manager.getUserVisibleURL(for: .rootContainer)
        await MainActor.run {
            NSWorkspace.shared.open(url)
        }
    }
}
