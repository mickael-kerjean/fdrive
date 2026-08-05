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
    @Published var serverURL = ""
    @Published var syncStatus: SyncStatus?
    @Published var connectionError: String?

    var isConnected: Bool {
        syncStatus != nil
    }

    var systemImage: String {
        syncStatus?.systemImage ?? "icloud.slash"
    }

    func connect() async {
        guard !serverURL.isEmpty else { return }
        connectionError = nil

        do {
            try await DomainManager.add()
            syncStatus = .upToDate
        } catch {
            connectionError = error.localizedDescription
        }
    }

    func disconnect() async {
        do {
            try await DomainManager.remove()
            syncStatus = nil
        } catch {
            syncStatus = .error
        }
    }

    func explore() async {
        do {
            try await DomainManager.open()
        } catch {
            syncStatus = .error
        }
    }
}
