import SwiftUI

struct ContentView: View {
    @EnvironmentObject private var state: AppState

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Filestash").font(.largeTitle.bold())
            if state.isConnected {
                Text(state.syncStatus?.label ?? "Not connected").foregroundStyle(.secondary)
                Text(state.serverURL).font(.footnote)
                Button("Disconnect", role: .destructive) { state.disconnect() }
            } else {
                TextField("Server URL", text: $state.serverURL)
                Button("Connect") { state.connect() }
                    .disabled(state.serverURL.isEmpty)
            }
        }
        .padding(24)
        .frame(width: 420)
    }
}
