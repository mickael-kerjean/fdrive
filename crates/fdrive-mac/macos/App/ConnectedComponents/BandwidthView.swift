import SwiftUI

struct BandwidthView: View {
    let meter: [Sample]

    private static let window = 24
    private static let average = 5

    var body: some View {
        HStack(alignment: .bottom, spacing: 12) {
            GeometryReader { geometry in
                HStack(alignment: .bottom, spacing: 2) {
                    ForEach(samples.indices, id: \.self) { index in
                        RoundedRectangle(cornerRadius: 1)
                            .fill(.primary)
                            .frame(maxWidth: .infinity)
                            .frame(height: max(samples[index] > 0 ? 8 : 2, geometry.size.height * samples[index]))
                    }
                }
                .frame(maxHeight: .infinity, alignment: .bottom)
            }
            .frame(maxWidth: .infinity)
            .frame(height: 44)
            .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: 4) {
                Label(rate(\.down), systemImage: "arrow.down")
                Label(rate(\.up), systemImage: "arrow.up")
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(.secondary)
        }
    }

    private var samples: [Double] {
        let window = meter.dropLast().suffix(Self.window)
        let peak = window.map { $0.up + $0.down }.max() ?? 0
        guard peak > 0 else { return Array(repeating: 0, count: window.count) }
        let scale = Double(max(peak, 250_000))
        return window.map { min(1, Double($0.up + $0.down) / scale) }
    }

    private func rate(_ direction: KeyPath<Sample, UInt64>) -> String {
        let window = meter.dropLast().suffix(Self.average)
        guard !window.isEmpty else { return "\(formatBytes(0))/s" }
        let total = window.reduce(UInt64(0)) { $0 + $1[keyPath: direction] }
        return "\(formatBytes(total / UInt64(window.count)))/s"
    }
}
