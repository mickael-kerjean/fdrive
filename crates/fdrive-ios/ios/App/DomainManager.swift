import FileProvider
import UIKit

enum DomainManager {
    private static let domain = NSFileProviderDomain(
        identifier: NSFileProviderDomainIdentifier(rawValue: "filestash"),
        displayName: "Filestash"
    )

    static func add() async throws {
        let existing = try await NSFileProviderManager.domains()
        if existing.contains(where: { $0.identifier == domain.identifier }) {
            return
        }
        try await NSFileProviderManager.add(domain)
    }

    static func remove() async throws {
        try await NSFileProviderManager.remove(domain)
    }

    @MainActor
    static func open() async {
        guard let manager = NSFileProviderManager(for: domain),
              let root = try? await manager.getUserVisibleURL(for: .rootContainer),
              var components = URLComponents(url: root, resolvingAgainstBaseURL: false)
        else { return }
        components.scheme = "shareddocuments"
        if let url = components.url {
            await UIApplication.shared.open(url)
        }
    }
}
