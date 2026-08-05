import SwiftUI

@main
struct FilestashApp: App {
    @StateObject private var state = AppState()

    var body: some Scene {
        WindowGroup {
            ContentView().environmentObject(state)
        }
        MenuBarExtra("Filestash", systemImage: state.systemImage) {
            TrayMenu().environmentObject(state)
        }
    }
}
