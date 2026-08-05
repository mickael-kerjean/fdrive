import AppKit
import SwiftUI

struct TrayMenu: View {
    @EnvironmentObject private var state: AppState

    var body: some View {
        if state.isConnected {
            Button("Explore") { NSApp.activate(ignoringOtherApps: true) }
            Button("Disconnect", role: .destructive) { state.disconnect() }
        } else {
            Button("Connect") { NSApp.activate(ignoringOtherApps: true) }
        }
        Divider()
        Button("Quit Filestash") { NSApp.terminate(nil) }
    }
}
