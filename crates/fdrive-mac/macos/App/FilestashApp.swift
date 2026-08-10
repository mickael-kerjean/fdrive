import SwiftUI

@main
struct FilestashApp: App {
    @StateObject private var state = AppState()

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
    }
}
