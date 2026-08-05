import AppKit
import SwiftUI

struct DisconnectView: View {
    @EnvironmentObject private var state: AppState

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            TextField("Server URL", text: $state.serverURL)

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

                Button("Connect") {
                    Task { await state.connect() }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(state.serverURL.isEmpty)
            }
        }
        .padding()
        .frame(width: 300)
    }
}
