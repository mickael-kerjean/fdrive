import AppKit
import SwiftUI

struct AccountSettingsView: View {
    @EnvironmentObject private var state: AppState
    @State private var working = false

    var body: some View {
        VStack(spacing: 0) {
            Form {
                LabeledContent("Server") {
                    Text(server ?? "Not connected")
                        .foregroundStyle(server == nil ? .secondary : .primary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                }

                LabeledContent("Status") {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(status.color)
                            .frame(width: 8, height: 8)

                        Text(status.label)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .formStyle(.grouped)

            Divider()

            HStack {
                Button("Quit") {
                    NSApp.terminate(nil)
                }

                Button("Logout") {
                    Task {
                        working = true
                        await state.disconnect()
                        working = false
                    }
                }
                .disabled(!state.isConnected || working)

                Spacer()

                Button("Admin Console") {
                    adminConsole()
                }
                .disabled(server == nil)
            }
            .padding(.horizontal)
            .padding(.vertical, 8)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var server: String? {
        let session = RuntimeSessionStore.load()
        return session.ok ? session.url : nil
    }

    private var status: (label: String, color: Color) {
        guard state.isConnected else { return ("Disconnected", .red) }
        guard state.syncStatus != .error else { return ("Sync error", .red) }
        return state.online ? ("Connected", .green) : ("Offline", .red)
    }

    private func adminConsole() {
        guard let server, let url = URL(string: "\(server)/admin") else { return }
        NSWorkspace.shared.open(url)
    }
}
