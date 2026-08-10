import AppKit
import SwiftUI

struct DisconnectView: View {
    @EnvironmentObject private var state: AppState
    @Environment(\.openWindow) private var openWindow
    @State private var serverURL = RuntimeSessionStore.load()?.url ?? ""
    @State private var probing = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            TextField("Server", text: $serverURL)

            if let error = state.connectionError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

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
        }
        .padding()
        .frame(width: 300)
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
