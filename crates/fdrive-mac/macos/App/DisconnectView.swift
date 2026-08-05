import AppKit
import SwiftUI

struct DisconnectView: View {
    @EnvironmentObject private var state: AppState

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            TextField("Server URL", text: $state.serverURL)

            Divider()

            HStack {
                Button("Quit") {
                    NSApp.terminate(nil)
                }

                Spacer()

                Button("Connect") {
                    state.connect()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(state.serverURL.isEmpty)
            }
        }
        .padding()
        .frame(width: 300)
    }
}
