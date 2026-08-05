import Foundation

struct RuntimeSession: Codable {
    let serverURL: String
    let token: String
}

enum RuntimeSessionStore {
    private static let file = FileManager.default
        .containerURL(forSecurityApplicationGroupIdentifier: "group.app.filestash.sync")?
        .appendingPathComponent("session.json")

    static func save(_ session: RuntimeSession) throws {
        guard let file else { throw CocoaError(.fileNoSuchFile) }
        try JSONEncoder().encode(session).write(to: file, options: .atomic)
    }

    static func load() -> RuntimeSession? {
        guard let file, let data = try? Data(contentsOf: file) else { return nil }
        return try? JSONDecoder().decode(RuntimeSession.self, from: data)
    }

    static func clear() {
        guard let file else { return }
        try? FileManager.default.removeItem(at: file)
    }
}
