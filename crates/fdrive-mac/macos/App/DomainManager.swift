import AppKit
import FileProvider

enum DomainManager {
    private static let domain = NSFileProviderDomain(
        identifier: NSFileProviderDomainIdentifier(rawValue: "filestash"),
        displayName: ""
    )

    static var manager: NSFileProviderManager? {
        NSFileProviderManager(for: domain)
    }

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
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            NSWorkspace.shared.open(url)
        }
    }

    static func reveal(_ path: String) async throws {
        guard let url = try await manager?.getUserVisibleURL(for: NSFileProviderItemIdentifier(path)) else { return }
        await MainActor.run { NSWorkspace.shared.activateFileViewerSelecting([url]) }
    }
}
