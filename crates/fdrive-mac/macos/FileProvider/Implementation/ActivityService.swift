import FileProvider

final class ActivityService: NSObject, NSFileProviderServiceSource, NSXPCListenerDelegate, ActivityProviding {
    let serviceName = activityServiceName
    private let adapter: Adapter
    private let listener = NSXPCListener.anonymous()

    init(adapter: Adapter) {
        self.adapter = adapter
        super.init()
        listener.delegate = self
        listener.resume()
    }

    func makeListenerEndpoint() throws -> NSXPCListenerEndpoint {
        listener.endpoint
    }

    func listener(_ listener: NSXPCListener, shouldAcceptNewConnection connection: NSXPCConnection) -> Bool {
        connection.exportedInterface = NSXPCInterface(with: ActivityProviding.self)
        connection.exportedObject = self
        connection.resume()
        return true
    }

    func snapshot(reply: @escaping (Data?) -> Void) {
        reply(Data(adapter.activityJson().utf8))
    }
}
