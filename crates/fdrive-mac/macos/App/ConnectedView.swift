import AppKit
import SwiftUI

struct ConnectedView: View {
    @EnvironmentObject private var state: AppState

    var body: some View {
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 12) {
                BandwidthView()
                Divider()
                RecentActivityView()
            }
            .padding()
            .frame(width: 340)

            Divider()

            HStack {
                Button("Quit") {
                    NSApp.terminate(nil)
                }

                Button("Disconnect") {
                    state.disconnect()
                }

                Spacer()

                Button("Explore") {
                    NSWorkspace.shared.open(FileManager.default.homeDirectoryForCurrentUser)
                }
            }
            .padding()
        }
    }
}

#Preview {
    ConnectedView()
        .environmentObject(AppState())
}
