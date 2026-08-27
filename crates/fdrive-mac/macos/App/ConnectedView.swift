import AppKit
import SwiftUI

struct ConnectedView: View {
    @EnvironmentObject private var state: AppState
    @StateObject private var activity = ActivityMonitor()

    var body: some View {
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 0) {
                BandwidthView(meter: activity.meter).padding()
                Divider()
                RecentActivityView(transfers: activity.transfers, clear: activity.clear)
            }
            .frame(width: 340)
            .task { await activity.run() }

            Divider()

            HStack {
                Button("Quit") {
                    Task {
                        try? await DomainManager.remove()
                        NSApp.terminate(nil)
                    }
                }

                Button("Disconnect") {
                    Task { await state.disconnect() }
                }

                Spacer()

                Button("Explore") {
                    Task { await state.explore() }
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding(.horizontal)
            .padding(.vertical, 8)
        }
    }
}

#Preview {
    ConnectedView()
        .environmentObject(AppState())
}
