import FileProvider
import Foundation

let arguments = CommandLine.arguments
guard arguments.count >= 2 else {
    usage()
}

let domain = NSFileProviderDomain(
    identifier: .init(rawValue: "filestash"),
    displayName: "Filestash"
)
guard let manager = NSFileProviderManager(for: domain) else {
    fputs("File Provider manager is unavailable\n", stderr)
    exit(1)
}

do {
    switch arguments[1] {
    case "download":
        try await manager.requestDownloadForItem(withIdentifier: identifier())
    case "evict":
        try await manager.evictItem(identifier: identifier())
    case "stabilize":
        guard arguments.count == 2 else { usage() }
        try await manager.waitForStabilization()
    case "is-downloaded":
        try await checkDownloadStatus(is: [.current, .downloaded])
    case "is-evicted":
        try await checkDownloadStatus(is: [.notDownloaded])
    case "is-outdated":
        try await checkDownloadStatus(is: [.downloaded])
    case "is-managed":
        let values = try await itemURL().resourceValues(forKeys: [.isUbiquitousItemKey])
        guard values.isUbiquitousItem == true else { exit(1) }
    default:
        usage("unknown command: \(arguments[1])")
    }
} catch {
    if !arguments[1].hasPrefix("is-") {
        fputs("\(error)\n", stderr)
    }
    exit(1)
}

func identifier() async throws -> NSFileProviderItemIdentifier {
    guard arguments.count == 3 else { usage() }
    let value = arguments[2]
    let root = try await manager.getUserVisibleURL(for: .rootContainer).path
    if value == root {
        return .rootContainer
    }
    if value.hasPrefix(root + "/") {
        return .init(rawValue: String(value.dropFirst(root.count)))
    }
    return .init(rawValue: value)
}

func itemURL() async throws -> URL {
    let value = arguments.count == 3 ? arguments[2] : ""
    let root = try await manager.getUserVisibleURL(for: .rootContainer).path
    if value == root || value.hasPrefix(root + "/") {
        return URL(fileURLWithPath: value)
    }
    return try await manager.getUserVisibleURL(for: identifier())
}

func checkDownloadStatus(is expected: Set<URLUbiquitousItemDownloadingStatus>) async throws {
    let values = try await itemURL().resourceValues(forKeys: [.ubiquitousItemDownloadingStatusKey])
    guard let status = values.ubiquitousItemDownloadingStatus, expected.contains(status) else {
        exit(1)
    }
}

func usage(_ error: String? = nil) -> Never {
    if let error { fputs("\(error)\n", stderr) }
    fputs("usage: FilestashTestKit <download|evict|is-downloaded|is-evicted|is-outdated|is-managed> <item identifier> | stabilize\n", stderr)
    exit(2)
}
