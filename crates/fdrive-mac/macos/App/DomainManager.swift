import AppKit
import FileProvider
import OSLog

enum DomainManager {
    private static let logger = Logger(subsystem: "app.filestash.mac", category: "Refresh")
    private static let domain = NSFileProviderDomain(
        identifier: NSFileProviderDomainIdentifier(rawValue: "filestash"),
        displayName: "Filestash"
    )

    static func add() async throws {
        try? await NSFileProviderManager.remove(domain)
        try await NSFileProviderManager.add(domain)
    }

    static func remove() async throws {
        try await NSFileProviderManager.remove(domain)
    }

    static func refresh() async {
        guard let manager = NSFileProviderManager(for: domain) else {
            logger.error("File Provider manager is unavailable")
            return
        }
        for container in [NSFileProviderItemIdentifier.rootContainer, .workingSet] {
            do {
                try await manager.signalEnumerator(for: container)
                logger.debug("Refreshed \(container.rawValue, privacy: .public)")
            } catch {
                logger.error("Could not refresh \(container.rawValue, privacy: .public): \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    static func connectActivity() async throws -> NSXPCConnection {
        guard let manager = NSFileProviderManager(for: domain) else {
            throw CocoaError(.fileNoSuchFile)
        }
        return try await ActivityService.connect(using: manager)
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
