import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents3
import org.kde.plasma.extras as PlasmaExtras
import org.kde.plasma.plasma5support as P5Support
import org.kde.kirigami as Kirigami
import "../code/model.js" as Model

PlasmoidItem {
  id: root

  property var drive: Model.stopped
  property string actionStatus: ""
  property bool busy: false
  property bool statusRunning: false

  readonly property bool running: drive.phase !== "stopped"
  readonly property bool signedIn: Model.signedIn(drive.phase)
  readonly property bool moving: drive.sparkline.trim() !== ""

  signal loginPageOpened()

  Plasmoid.icon: Model.phaseIcon(drive.phase)
  Plasmoid.status: drive.phase === "error" ? PlasmaCore.Types.NeedsAttentionStatus : PlasmaCore.Types.ActiveStatus
  toolTipMainText: "Filestash Drive"
  toolTipSubText: drive.phaseText + (moving ? " · " + drive.rate : "")

  onExpandedChanged: if (root.expanded) refresh()

  function refresh() {
    if (statusRunning) return
    statusRunning = true
    shell.exec("fdrive status", function(ok, out, err) {
      statusRunning = false
      root.drive = Model.parseStatus(out)
    })
  }

  function run(command, pending, done, failed) {
    actionStatus = pending
    clearStatus.stop()
    busy = true
    shell.exec(command, function(ok, out, err) {
      busy = false
      flash(ok ? done : err || failed)
    })
  }

  function openLogin(host) {
    host = host.trim()
    if (host === "" || busy) return
    actionStatus = "Checking " + host + "…"
    clearStatus.stop()
    busy = true
    shell.exec("fdrive login-url " + Model.shellQuote(host), function(ok, out, err) {
      busy = false
      if (!ok) {
        actionStatus = err || "Not a Filestash server"
        return
      }
      Qt.openUrlExternally(out.trim())
      actionStatus = "Sign in in the browser, then paste the token here"
      root.loginPageOpened()
    })
  }

  function login(host, token) {
    host = host.trim()
    token = token.trim()
    if (host === "" || token === "" || busy) return
    run("printf '%s\\n' " + Model.shellQuote(token) + " | fdrive login " + Model.shellQuote(host), "Signing in…", "Signed in", "Sign in failed")
  }

  function logout() {
    run("fdrive logout", "Signing out…", "Signed out", "Sign out failed")
  }

  function clear() {
    run("fdrive clear", "", "", "Could not clear the list")
  }

  function setActive(on) {
    run("systemctl --user " + (on ? "start" : "stop") + " fdrive", "", "", "Could not " + (on ? "start" : "stop") + " fdrive")
  }

  function openFolder() {
    Qt.openUrlExternally("file://" + drive.mount)
    root.expanded = false
  }

  function openTransfer(transfer) {
    var uri = transfer.uri
    shell.exec("dbus-send --session --type=method_call --dest=org.freedesktop.FileManager1 /org/freedesktop/FileManager1 org.freedesktop.FileManager1.ShowItems array:string:" + Model.shellQuote(uri) + " string:''", function(ok) {
      if (!ok) Qt.openUrlExternally(uri.substring(0, uri.lastIndexOf("/")))
    })
    root.expanded = false
  }

  function flash(message) {
    actionStatus = message
    clearStatus.restart()
    refresh()
  }

  P5Support.DataSource {
    id: shell
    engine: "executable"
    connectedSources: []

    property var callbacks: ({})
    property int counter: 0

    function exec(command, done) {
      counter += 1
      var source = command + " #" + counter
      callbacks[source] = done
      connectSource(source)
    }

    onNewData: function(source, data) {
      var done = callbacks[source]
      delete callbacks[source]
      disconnectSource(source)
      if (done) done(data["exit code"] === 0, data["stdout"] || "", Model.firstErrorLine(data["stderr"] || ""))
    }
  }

  Timer {
    interval: (root.expanded || root.drive.phase === "syncing" || root.drive.phase === "connecting" ? 2 : 10) * 1000
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

  fullRepresentation: PlasmaExtras.Representation {
    Layout.minimumWidth: Kirigami.Units.gridUnit * 20
    Layout.preferredWidth: Kirigami.Units.gridUnit * 22
    Layout.minimumHeight: column.implicitHeight + Kirigami.Units.largeSpacing * 2
    Layout.preferredHeight: column.implicitHeight + Kirigami.Units.largeSpacing * 2
    Layout.maximumHeight: column.implicitHeight + Kirigami.Units.largeSpacing * 2
    collapseMarginsHint: true

    Connections {
      target: root
      function onLoginPageOpened() { tokenField.forceActiveFocus() }
      function onExpandedChanged() {
        if (root.expanded && serverField.text === "") serverField.text = root.drive.server
      }
    }

    ColumnLayout {
      id: column
      anchors.fill: parent
      anchors.margins: Kirigami.Units.largeSpacing
      spacing: Kirigami.Units.largeSpacing

      RowLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.largeSpacing

        Kirigami.Icon {
          source: Model.phaseIcon(root.drive.phase)
          Layout.preferredWidth: Kirigami.Units.iconSizes.large
          Layout.preferredHeight: Kirigami.Units.iconSizes.large
          opacity: root.drive.phase === "ok" || root.drive.phase === "syncing" ? 1.0 : 0.6
        }

        ColumnLayout {
          Layout.fillWidth: true
          spacing: 0

          PlasmaExtras.Heading {
            Layout.fillWidth: true
            level: 3
            text: root.moving ? root.drive.sparkline.replace(/ /g, "▁") : "Filestash Drive"
            elide: Text.ElideRight
          }
          PlasmaComponents3.Label {
            Layout.fillWidth: true
            text: root.drive.phaseText + (root.moving ? " · " + root.drive.rate : "")
            opacity: 0.7
            elide: Text.ElideRight
          }
        }

        PlasmaComponents3.Switch {
          checked: root.running
          enabled: !root.busy
          onToggled: root.setActive(checked)
        }
      }

      PlasmaComponents3.Label {
        Layout.fillWidth: true
        visible: text !== ""
        text: root.actionStatus !== "" ? root.actionStatus : root.drive.lastError
        color: root.actionStatus === "" && root.drive.lastError !== "" ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
        wrapMode: Text.WordWrap
        textFormat: Text.PlainText
      }

      ColumnLayout {
        Layout.fillWidth: true
        visible: root.running && !root.signedIn
        spacing: Kirigami.Units.smallSpacing

        PlasmaComponents3.Label { text: "Server"; opacity: 0.7 }
        PlasmaComponents3.TextField {
          id: serverField
          Layout.fillWidth: true
          placeholderText: "files.example.com"
          enabled: !root.busy
          onAccepted: root.openLogin(text)
        }
        PlasmaComponents3.Button {
          text: "Open sign-in page"
          icon.name: "internet-web-browser"
          enabled: serverField.text.trim() !== "" && !root.busy
          onClicked: root.openLogin(serverField.text)
        }

        PlasmaComponents3.Label { text: "Token"; opacity: 0.7 }
        PlasmaComponents3.TextField {
          id: tokenField
          Layout.fillWidth: true
          echoMode: TextInput.Password
          placeholderText: "Paste the token shown after signing in"
          enabled: !root.busy
          onAccepted: connectButton.clicked()
        }
        PlasmaComponents3.Button {
          id: connectButton
          text: "Connect"
          icon.name: "network-connect"
          enabled: serverField.text.trim() !== "" && tokenField.text.trim() !== "" && !root.busy
          onClicked: {
            root.login(serverField.text, tokenField.text)
            tokenField.text = ""
          }
        }
      }

      GridLayout {
        Layout.fillWidth: true
        visible: root.signedIn
        columns: 3
        columnSpacing: Kirigami.Units.largeSpacing
        rowSpacing: 0

        PlasmaComponents3.Label { text: "Server"; opacity: 0.7 }
        PlasmaComponents3.Label {
          Layout.fillWidth: true
          text: root.drive.host
          elide: Text.ElideLeft
          textFormat: Text.PlainText
        }
        PlasmaComponents3.ToolButton {
          text: "Sign out"
          enabled: !root.busy
          onClicked: root.logout()
        }

        PlasmaComponents3.Label { text: "Folder"; opacity: 0.7 }
        PlasmaComponents3.Label {
          Layout.fillWidth: true
          text: root.drive.mount
          elide: Text.ElideLeft
          textFormat: Text.PlainText
        }
        PlasmaComponents3.ToolButton {
          text: "Open"
          onClicked: root.openFolder()
        }
      }

      Kirigami.Separator {
        Layout.fillWidth: true
        visible: root.signedIn && root.drive.transfers.length > 0
      }

      RowLayout {
        Layout.fillWidth: true
        visible: root.signedIn && root.drive.transfers.length > 0

        PlasmaExtras.Heading {
          Layout.fillWidth: true
          level: 5
          text: "Recent transfers"
        }
        PlasmaComponents3.ToolButton {
          text: "Clear"
          enabled: !root.busy
          onClicked: root.clear()
        }
      }

      ListView {
        Layout.fillWidth: true
        Layout.fillHeight: true
        Layout.minimumHeight: Kirigami.Units.gridUnit * 3
        Layout.preferredHeight: Math.min(contentHeight, Kirigami.Units.gridUnit * 14)
        visible: root.signedIn && root.drive.transfers.length > 0
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        model: root.drive.transfers
        QQC2.ScrollBar.vertical: PlasmaComponents3.ScrollBar {}

        delegate: PlasmaComponents3.ItemDelegate {
          required property var modelData
          width: ListView.view.width
          onClicked: root.openTransfer(modelData)

          contentItem: RowLayout {
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
              source: Model.directionIcon(modelData.direction)
              Layout.preferredWidth: Kirigami.Units.iconSizes.small
              Layout.preferredHeight: Kirigami.Units.iconSizes.small
              color: modelData.outcome === "failed" ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
              isMask: true
            }

            ColumnLayout {
              Layout.fillWidth: true
              spacing: 0

              PlasmaComponents3.Label {
                Layout.fillWidth: true
                text: modelData.name
                elide: Text.ElideMiddle
                textFormat: Text.PlainText
              }
              PlasmaComponents3.Label {
                Layout.fillWidth: true
                text: modelData.detail
                color: modelData.outcome === "failed" ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                opacity: modelData.outcome === "failed" ? 1.0 : 0.7
                font: Kirigami.Theme.smallFont
                elide: Text.ElideRight
                textFormat: Text.PlainText
              }
            }
          }
        }
      }
    }
  }
}
