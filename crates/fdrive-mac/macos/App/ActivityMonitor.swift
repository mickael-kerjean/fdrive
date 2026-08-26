import FileProvider
import OSLog
import SwiftUI

@MainActor
final class ActivityMonitor: ObservableObject {
    private let logger = Logger(subsystem: "app.filestash.mac", category: "Activity")

    @Published private(set) var transfers: [Transfer] = []
    @Published private(set) var meter: [Sample] = []
    private(set) var received = Date.distantPast

    private var connection: NSXPCConnection?
    private var version: UInt64?
    private var cleared: UInt64 = 0

    func clear() {
        cleared = transfers.first?.id ?? 0
        transfers = []
    }

    func run() async {
        defer { disconnect() }
        while !Task.isCancelled {
            await poll()
            try? await Task.sleep(for: .seconds(1))
        }
    }

    private func poll() async {
        guard let snapshot = await fetch() else { return }
        meter = snapshot.meter
        received = Date()
        guard snapshot.version != version else { return }
        version = snapshot.version
        transfers = snapshot.transfers.filter { $0.id > cleared }
    }

    private func fetch() async -> ActivitySnapshot? {
        guard let proxy = await proxy() else { return nil }
        let data: Data? = await withCheckedContinuation { continuation in
            proxy.snapshot { continuation.resume(returning: $0) }
        }
        guard let data else { return nil }
        return try? JSONDecoder().decode(ActivitySnapshot.self, from: data)
    }

    private func proxy() async -> ActivityProviding? {
        if connection == nil {
            do {
                connection = try await DomainManager.connectActivity()
            } catch {
                logger.error("Activity service unavailable: \(error.localizedDescription, privacy: .public)")
                return nil
            }
        }
        return connection?.remoteObjectProxyWithErrorHandler { [weak self] error in
            Task { @MainActor in
                self?.logger.error("Activity connection lost: \(error.localizedDescription, privacy: .public)")
                self?.disconnect()
            }
        } as? ActivityProviding
    }

    private func disconnect() {
        connection?.invalidate()
        connection = nil
    }
}
