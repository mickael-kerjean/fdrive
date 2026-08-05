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

    var isConnected: Bool {
        syncStatus != nil
    }

    var systemImage: String {
        syncStatus?.systemImage ?? "icloud.slash"
    }

    func connect() {
        guard !serverURL.isEmpty else { return }
        syncStatus = .upToDate
    }

    func disconnect() {
        syncStatus = nil
    }
}
