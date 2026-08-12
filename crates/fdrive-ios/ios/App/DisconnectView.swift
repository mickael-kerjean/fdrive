import SwiftUI

struct DisconnectView: View {
    @EnvironmentObject private var state: AppState
    @State private var serverURL = RuntimeSessionStore.load().url
    @State private var probing = false
    @FocusState private var fieldFocused: Bool

    var body: some View {
        NavigationStack {
            if state.server != nil {
                LoginView()
            } else {
                connectForm
            }
        }
        .tint(.fsAccent)
    }

    private var connectForm: some View {
        VStack(spacing: 18) {
            Text("Filestash")
                .font(.largeTitle.bold())

            HStack(spacing: 10) {
                Image(systemName: "globe")
                    .foregroundStyle(.secondary)
                TextField("demo.filestash.app", text: $serverURL)
                    .textContentType(.URL)
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.go)
                    .focused($fieldFocused)
                    .onSubmit(login)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 14)
            .background(Color(.secondarySystemGroupedBackground))
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))

            if let error = state.connectionError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Button(action: login) {
                Text("Connect")
                    .fontWeight(.semibold)
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(serverURL.isEmpty || probing)
        }
        .padding(.horizontal, 24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(.systemGroupedBackground).ignoresSafeArea())
    }

    private func login() {
        state.connectionError = nil
        probing = true
        fieldFocused = false
        let base = normalizeServer(input: serverURL)
        Task {
            defer { probing = false }
            guard (try? await probe(url: base, insecure: base.hasPrefix("http://"))) != nil else {
                state.connectionError = "\(base) does not look like a Filestash server"
                return
            }
            state.server = base
        }
    }
}
