import SwiftUI

struct RecentActivityView: View {
    let transfers: [Transfer]
    let clear: () -> Void
    @State private var height = CGFloat.zero

    var body: some View {
        HStack {
            Text("Activity").font(.headline)
            Spacer()
            if !transfers.isEmpty {
                Button("Clear", action: clear)
                    .buttonStyle(.plain)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }

        if transfers.isEmpty {
            Text("Nothing transferred yet")
                .font(.caption)
                .foregroundStyle(.secondary)
        }

        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                ForEach(transfers) { transfer in
                    row(transfer)
                }
            }
            .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { height = $0 }
        }
        .frame(height: min(300, height))
    }

    private func row(_ transfer: Transfer) -> some View {
        HStack(spacing: 10) {
            Image(systemName: transfer.systemImage)
                .font(.system(size: 19))
                .frame(width: 24)
                .foregroundStyle(transfer.state == .failed ? Color.red : Color.primary)

            VStack(alignment: .leading, spacing: 2) {
                Text(transfer.name)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(transfer.detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }

            Spacer(minLength: 0)
        }
    }
}
