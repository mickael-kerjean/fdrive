import OSLog
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
        RuntimeSessionStore.clear()
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
            try RuntimeSessionStore.save(.init(serverURL: serverURL, token: token))
            try await DomainManager.add()
            syncStatus = .upToDate
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
            RuntimeSessionStore.clear()
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
