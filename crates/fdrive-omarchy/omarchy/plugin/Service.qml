import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model

Item {
  id: root

  property bool fast: false
  property string phase: "stopped"
  property string server: ""
  property string mount: ""
  property string lastError: ""
  property var transfers: []
  property string sparkline: ""
  property string rate: ""
  property string actionStatus: ""

  readonly property bool running: phase !== "stopped"
  readonly property bool signedIn: Model.signedIn(phase)
  readonly property bool busy: action.running

  signal loginPageOpened()

  function refresh() {
    if (!statusProcess.running) statusProcess.running = true
  }

  function openLogin(host) {
    root.actionStatus = "Checking " + host.trim() + "…"
    run(["fdrive-omarchy", "login-url", host.trim()], "", function(ok, out, err) {
      if (!ok) {
        root.actionStatus = err || "Not a Filestash server"
        return
      }
      Qt.openUrlExternally(out.trim())
      root.actionStatus = "Sign in in the browser, then paste the token here"
      root.loginPageOpened()
    })
  }

  function login(host, token) {
    root.actionStatus = "Signing in…"
    run(["fdrive-omarchy", "login", host.trim()], token.trim() + "\n", function(ok, out, err) {
      flash(ok ? "Signed in" : err || "Sign in failed")
    })
  }

  function logout() {
    root.actionStatus = "Signing out…"
    run(["fdrive-omarchy", "logout"], "", function(ok, out, err) {
      flash(ok ? "Signed out" : err || "Sign out failed")
    })
  }

  function clear() {
    run(["fdrive-omarchy", "clear"], "", function(ok, out, err) {
      if (ok) refresh()
      else flash(err || "Could not clear the list")
    })
  }

  function setActive(on) {
    run(["systemctl", "--user", on ? "start" : "stop", "fdrive-omarchy"], "", function(ok, out, err) {
      if (ok) refresh()
      else flash(err || "Could not " + (on ? "start" : "stop") + " fdrive")
    })
  }

  function openFolder() {
    Quickshell.execDetached(["uwsm-app", "--", "nautilus", "file://" + mount])
  }

  function openTransfer(transfer) {
    Quickshell.execDetached(["uwsm-app", "--", "nautilus", "--select", Model.fileUri(mount, transfer.path)])
  }

  function run(command, input, done) {
    action.input = input
    action.done = done
    action.command = command
    action.running = true
  }

  function flash(message) {
    root.actionStatus = message
    clearStatus.restart()
    refresh()
  }

  Timer {
    interval: (root.fast || root.phase === "syncing" || root.phase === "connecting" ? 2 : 10) * 1000
    repeat: true
    running: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  Timer {
    id: clearStatus
    interval: 3500
    onTriggered: root.actionStatus = ""
  }

  Process {
    id: statusProcess
    command: ["fdrive-omarchy", "status"]
    stdout: StdioCollector { id: statusOut; waitForEnd: true }
    onExited: {
      var status = Model.parseStatus(statusOut.text)
      root.phase = status.phase
      root.server = status.server
      root.mount = status.mount
      root.lastError = status.lastError
      root.transfers = status.transfers
      root.sparkline = status.sparkline
      root.rate = status.rate
    }
  }

  Process {
    id: action
    property string input: ""
    property var done: null
    stdinEnabled: true
    stdout: StdioCollector { id: actionOut; waitForEnd: true }
    stderr: StdioCollector { id: actionErr; waitForEnd: true }
    onStarted: {
      if (input !== "") write(input)
      input = ""
    }
    onExited: function(exitCode) {
      var err = actionErr.text.trim().split("\n")[0].replace(/^fdrive-omarchy: /, "")
      done(exitCode === 0, actionOut.text, err)
    }
  }
}
