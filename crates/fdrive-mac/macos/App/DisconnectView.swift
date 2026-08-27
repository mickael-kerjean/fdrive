import AppKit
import SwiftUI

struct DisconnectView: View {
    @EnvironmentObject private var state: AppState
    @Environment(\.openWindow) private var openWindow
    @State private var serverURL = RuntimeSessionStore.load().url
    @State private var probing = false

    var body: some View {
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 12) {
                TextField("Server", text: $serverURL)

                if let error = state.connectionError {
                    Text(error)
                        .font(.caption)
                        .foregroundStyle(.secondary)
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
            guard (try? await probe(url: base, insecure: base.hasPrefix("http://"))) != nil else {
                state.connectionError = "\(base) does not look like a Filestash server"
                return
            }
            state.server = base
            openWindow(id: "login")
            NSApp.activate(ignoringOtherApps: true)
        }
    }
}
