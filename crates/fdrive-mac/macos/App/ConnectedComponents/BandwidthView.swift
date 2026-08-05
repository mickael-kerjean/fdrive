import SwiftUI

struct BandwidthView: View {
    private let downloadRate = "2.4 MB/s"
    private let uploadRate = "640 KB/s"
    private let samples = [0.12, 0.2, 0.16, 0.42, 0.34, 0.7, 0.52, 0.9, 0.62, 0.74, 0.38, 0.48]

    var body: some View {
        HStack(alignment: .bottom, spacing: 12) {
            Sparkline(samples: samples)
                .frame(maxWidth: .infinity)
                .frame(height: 44)

            VStack(alignment: .trailing, spacing: 4) {
                Label(downloadRate, systemImage: "arrow.down")
                Label(uploadRate, systemImage: "arrow.up")
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(.secondary)
        }
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
