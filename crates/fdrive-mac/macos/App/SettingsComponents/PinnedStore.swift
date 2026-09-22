import FileProvider
import OSLog
import SwiftUI

private let log = Logger(subsystem: "app.filestash.mac", category: "Pins")

private extension NSFileProviderItemIdentifier {
    var isFolder: Bool {
        rawValue.hasSuffix("/")
    }

    var relativePath: String {
        rawValue.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    }
}

final class PinNode: Identifiable {
    let path: String
    let name: String
    var isDirectory: Bool
    var materialized = false
    var children: [PinNode] = []

    var id: String { path }

    var subnodes: [PinNode]? {
        children.isEmpty ? nil : children
    }

    var identifier: NSFileProviderItemIdentifier {
        .init(rawValue: isDirectory ? "/\(path)/" : "/\(path)")
    }

    init(path: String, name: String, isDirectory: Bool) {
        self.path = path
        self.name = name
        self.isDirectory = isDirectory
    }

    static func tree(_ identifiers: [NSFileProviderItemIdentifier]) -> [PinNode] {
        let builder = TreeBuilder()
        identifiers.forEach(builder.add)
        return builder.roots.filter { $0.prune() }
    }

    private func prune() -> Bool {
        children.removeAll { !$0.prune() }
        children.sort { lhs, rhs in
            guard lhs.isDirectory == rhs.isDirectory else { return lhs.isDirectory }
            return lhs.name.localizedStandardCompare(rhs.name) == .orderedAscending
        }
        return !children.isEmpty || !isDirectory
    }
}

private final class TreeBuilder {
    private var nodes: [String: PinNode] = [:]
    private(set) var roots: [PinNode] = []

    func add(_ identifier: NSFileProviderItemIdentifier) {
        let components = identifier.relativePath.split(separator: "/")
        var parent: PinNode?
        var path = ""
        for (depth, component) in components.enumerated() {
            path = path.isEmpty ? String(component) : "\(path)/\(component)"
            let isLeaf = depth == components.count - 1
            let node = nodes[path] ?? insert(path: path, name: String(component), under: parent)
            if isLeaf {
                node.isDirectory = identifier.isFolder
                node.materialized = true
            }
            parent = node
        }
    }

    private func insert(path: String, name: String, under parent: PinNode?) -> PinNode {
        let node = PinNode(path: path, name: name, isDirectory: true)
        nodes[path] = node
        if let parent {
            parent.children.append(node)
        } else {
            roots.append(node)
        }
        return node
    }
}

@MainActor
final class PinnedStore: ObservableObject {
    @Published private(set) var roots: [PinNode] = []
    @Published private(set) var unavailable = false
    @Published private(set) var busy = false

    private static let containers: Set<NSFileProviderItemIdentifier> = [
        .rootContainer, .trashContainer, .workingSet,
    ]

    private var loaded: [NSFileProviderItemIdentifier] = []

    func run() async {
        while !Task.isCancelled {
            if !busy { await load() }
            try? await Task.sleep(for: .seconds(5))
        }
    }

    func load() async {
        guard let manager = DomainManager.manager else {
            roots = []
            loaded = []
            unavailable = true
            return
        }
        let materialized = await materializedItems(of: manager)
        unavailable = false
        guard materialized != loaded else { return }
        loaded = materialized
        roots = PinNode.tree(materialized)
    }

    func unpin(_ node: PinNode) async {
        guard let manager = DomainManager.manager else { return }
        busy = true
        await evict(node.identifier, using: manager)
        busy = false
        await load()
    }

    private func materializedItems(of manager: NSFileProviderManager) async -> [NSFileProviderItemIdentifier] {
        let identifiers = await withCheckedContinuation { continuation in
            MaterializedItemsObserver.collect(from: manager.enumeratorForMaterializedItems()) {
                continuation.resume(returning: $0)
            }
        }
        return identifiers
            .filter { !Self.containers.contains($0) }
            .sorted { $0.rawValue < $1.rawValue }
    }

    private func evict(_ identifier: NSFileProviderItemIdentifier, using manager: NSFileProviderManager) async {
        await withCheckedContinuation { continuation in
            manager.evictItem(identifier: identifier) { error in
                if let error = error as NSError? {
                    log.error("evict failed: \(Self.describe(error), privacy: .public)")
                }
                continuation.resume()
            }
        }
    }

    private nonisolated static func describe(_ error: NSError) -> String {
        let blocked = error.userInfo[NSFileProviderErrorItemKey] as? NSFileProviderItemIdentifier
        let reasons = error.underlyingErrors.map(\.localizedDescription).joined(separator: " | ")
        return "code=\(error.code) blocked=\(blocked?.rawValue ?? "-") reasons=\(reasons.isEmpty ? error.localizedDescription : reasons)"
    }
}
