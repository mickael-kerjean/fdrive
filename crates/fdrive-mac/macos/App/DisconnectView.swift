import AppKit
import SwiftUI

struct DisconnectView: View {
    @EnvironmentObject private var state: AppState
    @State private var serverURL = "http://debian.tailbcbddf.ts.net:8334/"
    @State private var token = "a8lBj6Z4ibBN6oNs9T7sg3UEe9OX2HafIfcde5FWtnSP8L6yk0uL6YnBzICoVBuehQdkQEyeFYvosPD6SugVI0Kqb3C1xKYNfW8MSjLmGIq7BfTfpLnwa6ycD0m3Gpy16ZbWMTJTMMA-6ScH4KYzY5dth-tFz-BSNMdO_DCwkGhnqh4_KhlBi-wLQ_oC"

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            TextField("Server URL", text: $serverURL)
            SecureField("Token", text: $token)

            if let error = state.connectionError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Divider()

            HStack {
                Button("Quit") {
                    NSApp.terminate(nil)
                }

                Spacer()

                Button("Connect") {
                    Task { await state.connect(serverURL: serverURL, token: token) }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(serverURL.isEmpty || token.isEmpty)
            }
        }
        .padding()
        .frame(width: 300)
    }
}
