import Foundation

enum RuntimeSessionStore {
    static let dataDirectory = FileManager.default
        .containerURL(forSecurityApplicationGroupIdentifier: "3736F8X9F9.group.app.filestash.sync")?
        .appendingPathComponent("data")

    static func save(url: String, token: String, insecure: Bool) {
        guard let dataDirectory else { return }
        sessionRemember(dataDir: dataDirectory.path, url: url, token: token, insecure: insecure)
    }

    static func load() -> Session {
        guard let dataDirectory else { return Session(url: "", token: "", insecure: false, ok: false) }
        return sessionRecall(dataDir: dataDirectory.path)
    }

    static func clear() {
        guard let dataDirectory else { return }
        sessionForget(dataDir: dataDirectory.path)
    }
}
