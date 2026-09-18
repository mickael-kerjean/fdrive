import SwiftUI

struct RecentActivityView: View {
    let transfers: [Transfer]
    let clear: () -> Void
    @State private var height = CGFloat.zero
    @State private var scrolled = false

    var body: some View {
        let ordered = transfers.filter { $0.state == .running }
            + transfers.filter { $0.state != .running }

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
                ForEach(ordered) { transfer in
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
                if transfers.contains(where: { $0.state != .running }) {
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
            TransferIcon(systemImage: transfer.systemImage, active: transfer.state == .running)
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

private struct TransferIcon: View {
    let systemImage: String
    let active: Bool

    var body: some View {
        ZStack {
            if active {
                TimelineView(.animation) { context in
                    Circle()
                        .stroke(Color.secondary.opacity(0.22), lineWidth: 1.5)
                        .overlay {
                            Circle()
                                .trim(from: 0, to: 0.28)
                                .stroke(Color.accentColor, style: StrokeStyle(lineWidth: 1.5, lineCap: .round))
                                .rotationEffect(.degrees(context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 0.9) / 0.9 * 360))
                        }
                }
            }
            Image(systemName: systemImage)
                .font(.system(size: active ? 12 : 19))
        }
        .frame(width: 24, height: 24)
    }
}
