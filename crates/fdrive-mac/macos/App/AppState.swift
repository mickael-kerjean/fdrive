import OSLog
import ServiceManagement
import SwiftUI

enum SyncStatus {
    case upToDate
    case error
}

@MainActor
final class AppState: ObservableObject {
    private let logger = Logger(subsystem: "app.filestash.mac", category: "App")

    @Published var syncStatus: SyncStatus?
    @Published var connectionError: String?
    @Published var server: String?

    @Published private var syncing = false
    private var beacon: NSKeyValueObservation?

    init() {
        let session = RuntimeSessionStore.load()
        if session.ok {
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
        syncing = Beacon.active
        beacon = Beacon.watch {
            Task { @MainActor in
                self.syncing = Beacon.active
            }
        }
    }

    var isConnected: Bool {
        syncStatus != nil
    }

    var systemImage: String {
        switch syncStatus {
        case .upToDate: syncing ? "arrow.triangle.2.circlepath.icloud" : "checkmark.icloud"
        case .error: "xmark.icloud"
        case nil: "icloud.slash"
        }
    }

    func connect(serverURL: String, token: String) async {
        connectionError = nil
        do {
            RuntimeSessionStore.save(url: serverURL, token: token, insecure: serverURL.hasPrefix("http://"))
            try await DomainManager.add()
            syncStatus = .upToDate
            try? SMAppService.mainApp.register()
            try? await DomainManager.open()
        } catch {
            RuntimeSessionStore.clear()
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
            if session.ok {
                Task.detached { try? logout(url: session.url, insecure: session.insecure, token: session.token) }
            }
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
