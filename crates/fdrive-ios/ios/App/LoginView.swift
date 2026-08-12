import SwiftUI
import WebKit

struct LoginView: View {
    @EnvironmentObject private var state: AppState

    var body: some View {
        if let server = state.server {
            LoginWebView(base: server) { token in
                Task { @MainActor in
                    await state.connect(serverURL: server, token: token)
                    state.server = nil
                }
            }
            .ignoresSafeArea(edges: .bottom)
            .navigationTitle("Sign In")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { state.server = nil }
                }
            }
        }
    }
}

struct LoginWebView: UIViewRepresentable {
    let base: String
    let onToken: (String) -> Void

    final class Coordinator { var watch: NSKeyValueObservation? }
    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration(); configuration.websiteDataStore = .nonPersistent()
        let web = WKWebView(frame: .zero, configuration: configuration)
        let host = URL(string: base)?.host
        var done = false
        context.coordinator.watch = web.observe(\.url) { web, _ in
            guard !done, web.url?.path.hasPrefix("/files") == true else { return }
            web.configuration.websiteDataStore.httpCookieStore.getAllCookies { cookies in
                let token = assembleToken(cookies: cookies
                    .filter { host == nil || $0.domain.contains(host!) }
                    .reduce(into: [:]) { $0[$1.name] = $1.value })
                if !done, !token.isEmpty {
                    done = true
                    onToken(token)
                }
            }
        }
        web.load(URLRequest(url: URL(string: "\(base)/login")!))
        return web
    }

    func updateUIView(_ web: WKWebView, context: Context) {}
}
