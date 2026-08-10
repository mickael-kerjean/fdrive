import SwiftUI

struct BandwidthView: View {
    let meter: [Sample]
    let received: Date

    private static let window = 24
    private static let average = 5

    var body: some View {
        HStack(alignment: .bottom, spacing: 12) {
            Sparkline(samples: samples, received: received)
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
        let window = meter.dropLast().suffix(Self.window + 1)
        let peak = window.map { $0.up + $0.down }.max() ?? 0
        guard peak > 0 else { return Array(repeating: 0, count: window.count) }
        return window.map { Double($0.up + $0.down) / Double(peak) }
    }

    private func rate(_ direction: KeyPath<Sample, UInt64>) -> String {
        let window = meter.dropLast().suffix(Self.average)
        guard !window.isEmpty else { return "\(formatBytes(0))/s" }
        let total = window.reduce(UInt64(0)) { $0 + $1[keyPath: direction] }
        return "\(formatBytes(total / UInt64(window.count)))/s"
    }
}

private struct Sparkline: View {
    let samples: [Double]
    let received: Date

    var body: some View {
        TimelineView(.animation) { timeline in
            GeometryReader { geometry in
                Path { path in
                    guard samples.count > 2 else { return }

                    let phase = CGFloat(min(1, timeline.date.timeIntervalSince(received)))
                    let step = geometry.size.width / CGFloat(samples.count - 2)
                    for (index, sample) in samples.enumerated() {
                        let x = step * (CGFloat(index) - phase)
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
            .clipped()
        }
        .accessibilityHidden(true)
    }
}

private extension Comparable {
    func clamped(to range: ClosedRange<Self>) -> Self {
        min(max(self, range.lowerBound), range.upperBound)
    }
}
