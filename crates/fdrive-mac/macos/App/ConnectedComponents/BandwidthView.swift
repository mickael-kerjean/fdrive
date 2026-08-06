import SwiftUI

struct BandwidthView: View {
    let meter: [Sample]

    private static let window = 24
    private static let average = 5

    var body: some View {
        HStack(alignment: .bottom, spacing: 12) {
            Sparkline(samples: samples)
                .frame(maxWidth: .infinity)
                .frame(height: 44)

            VStack(alignment: .trailing, spacing: 4) {
                Label(rate(\.down), systemImage: "arrow.down")
                Label(rate(\.up), systemImage: "arrow.up")
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(.secondary)
        }
    }

    private var samples: [Double] {
        let window = meter.suffix(Self.window)
        let peak = window.map { $0.up + $0.down }.max() ?? 0
        guard peak > 0 else { return Array(repeating: 0, count: window.count) }
        return window.map { Double($0.up + $0.down) / Double(peak) }
    }

    private func rate(_ direction: KeyPath<Sample, UInt64>) -> String {
        let window = meter.suffix(Self.average)
        guard !window.isEmpty else { return "\(formatBytes(0))/s" }
        let total = window.reduce(UInt64(0)) { $0 + $1[keyPath: direction] }
        return "\(formatBytes(total / UInt64(window.count)))/s"
    }
}

private struct Sparkline: View {
    let samples: [Double]

    var body: some View {
        GeometryReader { geometry in
            Path { path in
                guard samples.count > 1 else { return }

                for (index, sample) in samples.enumerated() {
                    let x = geometry.size.width * CGFloat(index) / CGFloat(samples.count - 1)
                    let y = geometry.size.height * (1 - sample.clamped(to: 0...1))
                    let point = CGPoint(x: x, y: y)

                    if index == 0 {
                        path.move(to: point)
                    } else {
                        path.addLine(to: point)
                    }
                }
            }
            .stroke(.primary, style: StrokeStyle(lineWidth: 1.5, lineCap: .round, lineJoin: .round))
        }
        .accessibilityHidden(true)
    }
}

private extension Comparable {
    func clamped(to range: ClosedRange<Self>) -> Self {
        min(max(self, range.lowerBound), range.upperBound)
    }
}
