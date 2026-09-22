import AppKit
import SwiftUI

struct AboutSettingsView: View {
    var body: some View {
        VStack(spacing: 8) {
            Image(nsImage: NSApp.applicationIconImage)
                .resizable()
                .frame(width: 64, height: 64)

            Text("Filestash")
                .font(.title2)

            Text(version)
                .font(.callout)
                .foregroundStyle(.secondary)

            if let server {
                Text(server)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                    .padding(.top, 6)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(.vertical, 30)
        .padding(.horizontal, 20)
    }

    private var version: String {
        let short = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "?"
        let build = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "?"
        return "Version \(short) (\(build))"
    }

    private var server: String? {
        let session = RuntimeSessionStore.load()
        return session.ok ? session.url : nil
    }
}
