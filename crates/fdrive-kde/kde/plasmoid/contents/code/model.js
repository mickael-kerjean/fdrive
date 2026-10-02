.pragma library

var stopped = { phase: "stopped", phaseText: "Off", server: "", host: "", mount: "", lastError: "", transfers: [], sparkline: "", rate: "" }

var phaseIcons = {
  ok: "folder-cloud",
  syncing: "folder-sync",
  connecting: "folder-sync",
  error: "dialog-warning",
  loggedOut: "network-offline"
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

function phaseIcon(phase) {
  return phaseIcons[phase] || "network-offline"
}

function directionIcon(direction) {
  return direction === "up" ? "cloud-upload" : "cloud-download"
}

function firstErrorLine(text) {
  return text.trim().split("\n")[0].replace(/^fdrive: /, "")
}

function shellQuote(value) {
  return "'" + String(value).replace(/'/g, "'\\''") + "'"
}
