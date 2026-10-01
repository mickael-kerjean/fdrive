var stopped = { phase: "stopped", server: "", mount: "", lastError: "", transfers: [], sparkline: "", rate: "" }

var phaseTexts = {
  ok: "Up to date",
  syncing: "Syncing",
  connecting: "Connecting",
  error: "Sync trouble",
  loggedOut: "Not signed in"
}

var phaseGlyphs = {
  ok: 0xF0160,
  syncing: 0xF063F,
  connecting: 0xF063F,
  error: 0xF09E0,
  loggedOut: 0xF0163
}

function parseStatus(text) {
  try {
    return Object.assign({}, stopped, JSON.parse(text))
  } catch (e) {
    return stopped
  }
}

function signedIn(phase) {
  return phase !== "stopped" && phase !== "loggedOut"
}

function phaseText(phase) {
  return phaseTexts[phase] || "Off"
}

function phaseGlyph(phase) {
  return String.fromCodePoint(phaseGlyphs[phase] || 0xF0164)
}

function directionGlyph(direction) {
  return String.fromCodePoint(direction === "up" ? 0xF0552 : 0xF01DA)
}

function formatBytes(bytes) {
  var units = ["B", "KB", "MB", "GB", "TB"]
  var i = 0
  while (bytes >= 1000 && i < units.length - 1) {
    bytes /= 1000
    i++
  }
  return (i === 0 ? bytes : bytes.toFixed(1)) + " " + units[i]
}

function hostOf(server) {
  return server.replace(/^[a-z]+:\/\//i, "").split("/")[0]
}

function transferMeta(transfer) {
  if (!transfer) return ""
  var folder = transfer.path.substring(0, transfer.path.lastIndexOf("/")) || "/"
  if (transfer.outcome === "failed") return "Failed · " + (transfer.error || folder)
  if (transfer.outcome === "running") {
    var percent = transfer.size > 0 ? Math.floor(100 * transfer.progress / transfer.size) : 0
    return percent + "% of " + formatBytes(transfer.size) + " · " + folder
  }
  return formatBytes(transfer.size) + " · " + folder
}

function fileUri(mount, path) {
  return "file://" + (mount + path).split("/").map(encodeURIComponent).join("/")
}
