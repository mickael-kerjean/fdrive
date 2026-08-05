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
                    Task {
                        await state.disconnect()
                        if !state.isConnected {
                            NSApp.terminate(nil)
                        }
                    }
                }

                Button("Disconnect") {
                    Task { await state.disconnect() }
                }

                Spacer()

                Button("Explore") {
                    Task { await state.explore() }
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
