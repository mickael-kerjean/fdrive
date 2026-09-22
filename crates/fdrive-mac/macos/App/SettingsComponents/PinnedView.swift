import SwiftUI

struct PinnedSettingsView: View {
    @StateObject private var pins = PinnedStore()

    var body: some View {
        Group {
            if pins.unavailable {
                Notice(icon: "exclamationmark.icloud")
            } else if pins.roots.isEmpty {
                Notice(icon: "pin.slash")
            } else {
                List(pins.roots, children: \.subnodes) { node in
                    PinnedRow(node: node, busy: pins.busy) {
                        Task { await pins.unpin(node) }
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task { await pins.run() }
    }
}

private struct Notice: View {
    let icon: String

    var body: some View {
        Image(systemName: icon)
            .font(.system(size: 32))
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct PinnedRow: View {
    let node: PinNode
    let busy: Bool
    let unpin: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: node.isDirectory ? "folder" : "doc")
                .foregroundStyle(.secondary)

            Text(node.name)
                .lineLimit(1)
                .truncationMode(.middle)

            Spacer(minLength: 0)

            if node.materialized {
                Button("Unpin", action: unpin)
                    .controlSize(.small)
                    .disabled(busy)
            }
        }
        .padding(.vertical, 2)
    }
}
