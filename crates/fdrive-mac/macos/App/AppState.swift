import OSLog
import ServiceManagement
import SwiftUI

enum SyncStatus {
    case upToDate
    case syncing
    case error

    var label: String {
        switch self {
        case .upToDate: "Everything is up to date"
        case .syncing: "Syncing"
        case .error: "Sync error"
        }
    }

    var systemImage: String {
        switch self {
        case .upToDate: "checkmark.icloud"
        case .syncing: "arrow.trianglehead.2.clockwise.rotate.90.icloud"
        case .error: "xmark.icloud"
        }
    }
}

@MainActor
final class AppState: ObservableObject {
    private let logger = Logger(subsystem: "app.filestash.mac", category: "App")

    @Published var syncStatus: SyncStatus?
    @Published var connectionError: String?

    private(set) var serverURL = ""
    private(set) var token = ""

    init() {
        if let session = RuntimeSessionStore.load() {
            serverURL = session.url
            token = session.token
            syncStatus = .upToDate
            try? SMAppService.mainApp.register()
            Task {
                do {
                    try await DomainManager.add()
                } catch {
                    logger.error("Reattach failed: \(error.localizedDescription, privacy: .public)")
                    syncStatus = .error
                }
            }
        }
    }

    var isConnected: Bool {
        syncStatus != nil
    }

    var systemImage: String {
        syncStatus?.systemImage ?? "icloud.slash"
    }

    func connect(serverURL: String, token: String) async {
        connectionError = nil
        self.serverURL = serverURL
        self.token = token

        do {
            RuntimeSessionStore.save(url: serverURL, token: token, insecure: serverURL.hasPrefix("http://"))
            try await DomainManager.add()
            syncStatus = .upToDate
            try? SMAppService.mainApp.register()
        } catch {
            RuntimeSessionStore.clear()
            self.serverURL = ""
            self.token = ""
            let error = error as NSError
            logger.error("Connect failed: \(error.domain, privacy: .public) \(error.code): \(error.localizedDescription, privacy: .public)")
            connectionError = error.localizedDescription
        }
    }

    func disconnect() async {
        do {
            try await DomainManager.remove()
            try? await SMAppService.mainApp.unregister()
            let session = RuntimeSessionStore.load()
            RuntimeSessionStore.clear()
            if let session {
                Task.detached { try? logout(url: session.url, insecure: session.insecure, token: session.token) }
            }
            serverURL = ""
            token = ""
            syncStatus = nil
        } catch {
            logger.error("Disconnect failed: \(error.localizedDescription, privacy: .public)")
            syncStatus = .error
        }
    }

    func explore() async {
        do {
            try await DomainManager.open()
        } catch {
            logger.error("Explore failed: \(error.localizedDescription, privacy: .public)")
            syncStatus = .error
        }
    }
}
