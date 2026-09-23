import SwiftUI

@main
struct FilestashApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @StateObject private var state = AppState.shared

    var body: some Scene {
        MenuBarExtra("Filestash", systemImage: state.systemImage) {
            if state.isConnected {
                ConnectedView().environmentObject(state)
            } else {
                DisconnectView().environmentObject(state)
            }
        }
        .menuBarExtraStyle(.window)

        Window("Sign In", id: "login") {
            LoginView().environmentObject(state)
        }
        .windowResizability(.contentSize)

        Settings {
            SettingsView().environmentObject(state)
        }
    }
}
