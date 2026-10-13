import AppKit
import SwiftUI

struct DisconnectView: View {
    @EnvironmentObject private var state: AppState
    @Environment(\.openWindow) private var openWindow
    @State private var serverURL = RuntimeSessionStore.load().url
    @State private var probing = false
    private let locked = RuntimeSessionStore.features.serverLocked && !RuntimeSessionStore.load().url.isEmpty

    var body: some View {
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 12) {
                if locked {
                    Text(serverURL)
                } else {
                    TextField("Server", text: $serverURL)
                }

                if let error = state.connectionError {
                    Text(error)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 10)
            .frame(width: 340)

            Divider()

            HStack {
                Button("Quit") {
                    NSApp.terminate(nil)
                }

                Spacer()

                Button("Login") {
                    login()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(serverURL.isEmpty || probing)
            }
            .padding(.horizontal)
            .padding(.vertical, 8)
        }
    }

    private func login() {
        state.connectionError = nil
        probing = true
        let base = normalizeServer(input: serverURL)
        Task {
            defer { probing = false }
            guard let dataDirectory = RuntimeSessionStore.dataDirectory else {
                state.connectionError = "Data directory unavailable"
                return
            }
            do {
                _ = try await probe(dataDir: dataDirectory.path, url: base)
            } catch let error as FsError {
                state.connectionError = switch error {
                case .NotAuthenticated: "Not authenticated"
                case .PermissionDenied: "Permission denied"
                case .NotFound: "Not found"
                case .Network(let msg), .Other(let msg): msg
                }
                return
            } catch {
                state.connectionError = error.localizedDescription
                return
            }
            if base.hasPrefix("sftp://") {
                await state.connect(serverURL: base, token: "n/a")
                return
            }
            state.server = base
            openWindow(id: "login")
            NSApp.activate(ignoringOtherApps: true)
        }
    }
}
