import SwiftUI

struct SettingsView: View {
    var body: some View {
        TabView {
            PinnedSettingsView()
                .tabItem { Label("Pinned", systemImage: "pin") }

            AboutSettingsView()
                .tabItem { Label("About", systemImage: "info.circle") }
        }
        .frame(width: 480)
    }
}
