import SwiftUI

@main
struct FilestashApp: App {
    @StateObject private var state = AppState()

    var body: some Scene {
        WindowGroup {
            if state.isConnected {
                ConnectedView().environmentObject(state)
            } else {
                DisconnectView().environmentObject(state)
            }
        }
    }
}
