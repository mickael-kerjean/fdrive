import SwiftUI

struct RecentActivityView: View {
    let transfers: [Transfer]
    let clear: () -> Void
    @State private var height = CGFloat.zero
    @State private var scrolled = false

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                if transfers.isEmpty {
                    VStack(spacing: 4) {
                        Image(systemName: "tray")
                            .font(.title3)
                        Text("No transfer")
                            .font(.caption)
                    }
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 8)
                }
                ForEach(transfers) { transfer in
                    row(transfer)
                }
            }
            .padding(.horizontal).padding(.bottom, 8)
            .onGeometryChange(for: CGRect.self, of: { $0.frame(in: .named("activity")) }) { frame in
                height = frame.height
                withAnimation(.easeInOut(duration: 0.2)) { scrolled = frame.minY < -25 }
            }
        }
        .coordinateSpace(name: "activity")
        .frame(height: min(300, height))
        .safeAreaInset(edge: .top, spacing: 8) {
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
            .padding(.horizontal).padding(.top, 8)
            .opacity(scrolled ? 0 : 1)
        }
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
