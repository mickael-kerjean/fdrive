import Foundation

func formatBytes(_ count: UInt64) -> String {
    let formatter = ByteCountFormatter()
    formatter.countStyle = .file
    formatter.allowsNonnumericFormatting = false
    return formatter.string(fromByteCount: Int64(count)).replacingOccurrences(of: " ", with: "")
}

extension Transfer {
    var name: String {
        path.hasPrefix("/") ? String(path.dropFirst()) : path
    }

    var systemImage: String {
        direction == .up ? "arrow.up.doc" : "arrow.down.doc"
    }

    var detail: String {
        if state == .failed {
            return "Failed · \(error ?? "unknown error")"
        }
        let status = switch (state, direction) {
        case (.running, .down):
            "Downloading"
        case (.running, .up):
            "Uploading"
        case (_, .down):
            "Downloaded"
        case (_, .up):
            "Uploaded"
        }
        let bytes = switch (mode, state) {
        case (.delta, _):
            "Δ\(formatBytes(wire)) of \(formatBytes(size))"
        case (.full, .running) where progress > 0:
            "\(formatBytes(progress)) / \(formatBytes(size))"
        default:
            formatBytes(size)
        }
        return "\(status) · \(bytes)"
    }
}
