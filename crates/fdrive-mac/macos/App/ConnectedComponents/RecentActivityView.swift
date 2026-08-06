import SwiftUI

private enum TransferDirection {
    case upload
    case download

    var systemImage: String {
        switch self {
        case .upload: "arrow.up.doc"
        case .download: "arrow.down.doc"
        }
    }
}

private struct RecentTransfer: Identifiable {
    let id = UUID()
    let filename: String
    let detail: String
    let direction: TransferDirection
}

struct RecentActivityView: View {
    private let transfers = [
        RecentTransfer(filename: "Q3 product roadmap.pdf", detail: "8.4 MB · Just now", direction: .download),
        RecentTransfer(filename: "brand-assets.zip", detail: "24.1 MB · 2 min ago", direction: .upload),
        RecentTransfer(filename: "Team/meeting-notes.md", detail: "42 KB · 5 min ago", direction: .download),
        RecentTransfer(filename: "Photos/launch-day.jpg", detail: "3.7 MB · 12 min ago", direction: .upload)
    ]

    var body: some View {
        Text("Activity").font(.headline)

        ForEach(transfers) { transfer in
            HStack(spacing: 10) {
                Image(systemName: transfer.direction.systemImage)
                    .font(.system(size: 19))
                    .frame(width: 24)

                VStack(alignment: .leading, spacing: 2) {
                    Text(transfer.filename)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Text(transfer.detail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer(minLength: 0)
            }
        }
    }
}
