import SwiftUI

struct SettingsView: View {
    var body: some View {
        TabView {
            AccountSettingsView()
                .tabItem { Label("Account", systemImage: "person.crop.circle") }

            PinnedSettingsView()
                .tabItem { Label("Pinned", systemImage: "pin") }

            AboutSettingsView()
                .tabItem { Label("About", systemImage: "info.circle") }
        }
        .frame(width: 480)
    }
}
