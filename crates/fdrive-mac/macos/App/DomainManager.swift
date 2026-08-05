import AppKit
import FileProvider

enum DomainManager {
    private static let domain = NSFileProviderDomain(
        identifier: NSFileProviderDomainIdentifier(rawValue: "filestash"),
        displayName: "Filestash"
    )

    static func add() async throws {
        try await NSFileProviderManager.add(domain)
    }

    static func remove() async throws {
        try await NSFileProviderManager.remove(domain)
    }

    @MainActor
    static func open() async throws {
        guard let manager = NSFileProviderManager(for: domain) else {
            throw CocoaError(.fileNoSuchFile)
        }
        let url = try await manager.getUserVisibleURL(for: .rootContainer)
        NSWorkspace.shared.open(url)
    }
}
