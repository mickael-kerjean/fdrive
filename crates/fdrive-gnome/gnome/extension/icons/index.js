import Gio from "gi://Gio";

export function phaseIcon(phase) {
    switch (phase) {
    case "ok": return _bundled("fdrive-ok-symbolic");
    case "syncing":
    case "connecting": return _bundled("fdrive-syncing-symbolic");
    case "error": return _themed("dialog-warning-symbolic");
    default: return _themed("network-offline-symbolic");
    }
}

export function transferIcon(transfer) {
    if (transfer.outcome === "failed") return _themed("dialog-warning-symbolic");
    return _bundled(transfer.direction === "up" ? "fdrive-upload-symbolic" : "fdrive-download-symbolic");
}

const _dir = Gio.File.new_for_uri(import.meta.url).get_parent();
const _bundled = name => new Gio.FileIcon({file: _dir.get_child(`${name}.svg`)});
const _themed = name => new Gio.ThemedIcon({name});
