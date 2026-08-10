import SwiftUI
import WebKit

struct LoginView: View {
    @EnvironmentObject private var state: AppState
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        if let server = state.server {
            LoginWebView(base: server) { token in
                Task { @MainActor in
                    await state.connect(serverURL: server, token: token)
                    dismiss()
                }
            }
            .frame(width: 480, height: 560)
            .onDisappear { state.server = nil }
        }
    }
}

struct LoginWebView: NSViewRepresentable {
    let base: String
    let onToken: (String) -> Void

    final class Coordinator { var watch: NSKeyValueObservation? }
    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> WKWebView {
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

    func updateNSView(_ web: WKWebView, context: Context) {}
}
