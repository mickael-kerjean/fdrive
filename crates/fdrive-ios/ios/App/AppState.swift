import OSLog
import SwiftUI

@MainActor
final class AppState: ObservableObject {
    private let logger = Logger(subsystem: "app.filestash.ios", category: "App")

    @Published var connectionError: String?
    @Published var server: String?
    @Published private(set) var isConnected: Bool

    init() {
        isConnected = RuntimeSessionStore.load().ok
        if isConnected {
            Task {
                do {
                    try await DomainManager.add()
                } catch {
                    logger.error("Reattach failed: \(error.localizedDescription, privacy: .public)")
                }
            }
        }
    }

    func connect(serverURL: String, token: String) async {
        connectionError = nil
        RuntimeSessionStore.save(url: serverURL, token: token, insecure: serverURL.hasPrefix("http://"))
        // Domain registration is best-effort; the session stands on its own.
        try? await DomainManager.add()
        isConnected = true
    }

    func disconnect() async {
        try? await DomainManager.remove()
        let session = RuntimeSessionStore.load()
        RuntimeSessionStore.clear()
        if session.ok {
            Task.detached { try? logout(url: session.url, insecure: session.insecure, token: session.token) }
        }
        isConnected = false
    }
}
