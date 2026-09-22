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
                RecentActivityView(
                    transfers: activity.transfers,
                    completedCount: activity.completedCount,
                    clear: activity.clear
                )
            }
            .frame(width: 340)
            .task { await activity.run() }

            Divider()

            HStack {
                if #available(macOS 14.0, *) {
                    ButtonSettings()
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

@available(macOS 14.0, *)
private struct ButtonSettings: View {
    @Environment(\.openSettings) private var openSettings
    var body: some View {
        Button("Settings") {
            NSApp.activate(ignoringOtherApps: true)
            openSettings()
        }
    }
}

#Preview {
    ConnectedView()
        .environmentObject(AppState())
}
