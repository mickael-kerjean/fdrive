import SwiftUI

struct RecentActivityView: View {
    let transfers: [Transfer]

    var body: some View {
        Text("Activity").font(.headline)

        if transfers.isEmpty {
            Text("Nothing transferred yet")
                .font(.caption)
                .foregroundStyle(.secondary)
        }

        ForEach(transfers.prefix(4)) { transfer in
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
}
