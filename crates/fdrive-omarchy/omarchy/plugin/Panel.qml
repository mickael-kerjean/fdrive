import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import Quickshell
import qs.Commons
import qs.Ui
import "Model.js" as Model

Panel {
  id: root
  moduleName: "filestash.fdrive"
  manageIpc: false

  property var anchorItem: null
  property var hostWidget: null
  property alias service: drive

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family
  readonly property bool healthy: drive.phase === "ok" || drive.phase === "syncing"
  readonly property bool moving: drive.sparkline.trim() !== ""

  function switchPanel(direction) {
    if (root.bar && typeof root.bar.switchPanelFrom === "function")
      return root.bar.switchPanelFrom(root.hostWidget || root, direction)
    return false
  }

  onOpenedChanged: if (opened) {
    drive.refresh()
    if (serverField.text === "") serverField.text = drive.server
  }

  Service {
    id: drive
    fast: root.opened
    onLoginPageOpened: tokenField.forceActiveFocus()
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root.hostWidget || root
    bar: root.bar
    open: root.opened
    focusTarget: drive.running && !drive.signedIn ? serverField : keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(column.implicitHeight)

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Column {
        id: column
        width: parent.width
        spacing: Style.space(12)

        PanelHero {
          width: parent.width
          title: root.moving ? drive.sparkline.replace(/ /g, "▁") : "Filestash Drive"
          meta: Model.phaseText(drive.phase) + (root.moving ? " · " + drive.rate : "")
          foreground: root.foreground
          fontFamily: root.fontFamily
          iconOpacity: root.healthy ? 1.0 : 0.6
          trailingControl: Component {
            ToggleSwitch {
              checked: drive.running
              busy: drive.busy
              foreground: root.foreground
              onToggled: drive.setActive(!drive.running)
            }
          }
          iconComponent: Component {
            Text {
              text: Model.phaseGlyph(drive.phase)
              color: drive.phase === "error" ? root.urgent : root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.display
            }
          }
        }

        Text {
          visible: text !== ""
          width: parent.width
          textFormat: Text.PlainText
          text: drive.actionStatus !== "" ? drive.actionStatus : drive.lastError
          color: drive.actionStatus === "" && drive.lastError !== "" ? root.urgent : root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.bodySmall
          wrapMode: Text.WordWrap
        }

        Column {
          visible: drive.running && !drive.signedIn
          width: parent.width
          spacing: Style.spacing.labelGap

          Label { text: "Server" }
          TextField {
            id: serverField
            width: parent.width
            placeholderText: "files.example.com"
            foreground: root.foreground
            font.family: root.fontFamily
            enabled: !drive.busy
            onAccepted: drive.openLogin(text)
            Keys.onTabPressed: tokenField.forceActiveFocus()
          }
          Button {
            text: "Open sign-in page"
            iconText: String.fromCodePoint(0xF03CC)
            foreground: root.foreground
            fontFamily: root.fontFamily
            bordered: true
            enabled: serverField.text.trim() !== "" && !drive.busy
            onClicked: drive.openLogin(serverField.text)
          }

          Item { width: 1; height: Style.space(4) }

          Label { text: "Token" }
          TextField {
            id: tokenField
            width: parent.width
            password: true
            placeholderText: "Paste the token shown after signing in"
            foreground: root.foreground
            font.family: root.fontFamily
            enabled: !drive.busy
            onAccepted: connectButton.clicked()
            Keys.onBacktabPressed: serverField.forceActiveFocus()
          }
          Button {
            id: connectButton
            text: "Connect"
            iconText: String.fromCodePoint(0xF0342)
            foreground: root.foreground
            fontFamily: root.fontFamily
            bordered: true
            enabled: serverField.text.trim() !== "" && tokenField.text.trim() !== "" && !drive.busy
            onClicked: {
              drive.login(serverField.text, tokenField.text)
              tokenField.text = ""
            }
          }
        }

        Column {
          visible: drive.signedIn
          width: parent.width
          spacing: Style.space(2)

          RowLayout {
            width: parent.width
            spacing: Style.space(8)

            Label { text: "Server" }
            Value {
              Layout.fillWidth: true
              text: Model.hostOf(drive.server)
            }
            Action {
              text: "[logout]"
              hoverColor: root.urgent
              enabled: !drive.busy
              onClicked: drive.logout()
            }
          }

          RowLayout {
            width: parent.width
            spacing: Style.space(8)

            Label { text: "Folder" }
            Value {
              Layout.fillWidth: true
              text: drive.mount
            }
            Action {
              text: "[open]"
              onClicked: { drive.openFolder(); root.close() }
            }
          }

        }

        PanelSeparator {
          visible: drive.signedIn && drive.transfers.length > 0
          foreground: root.foreground
        }

        Column {
          visible: drive.signedIn && drive.transfers.length > 0
          width: parent.width
          spacing: Style.space(6)

          RowLayout {
            width: parent.width

            PanelSectionHeader {
              Layout.fillWidth: true
              text: "RECENT TRANSFERS"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }
            Action {
              text: "[clear]"
              enabled: !drive.busy
              onClicked: drive.clear()
            }
          }

          ListView {
            width: parent.width
            height: Math.min(contentHeight, Style.space(240))
            clip: true
            spacing: Style.space(6)
            boundsBehavior: Flickable.StopAtBounds
            model: drive.transfers
            delegate: TransferRow {
              required property var modelData
              width: ListView.view.width
              transfer: modelData
            }
            Controls.ScrollBar.vertical: Controls.ScrollBar { policy: Controls.ScrollBar.AsNeeded }
          }
        }
      }
    }
  }

  component TransferRow: CursorSurface {
    id: row
    property var transfer: null

    hasCursor: mouse.containsMouse
    foreground: root.foreground
    implicitHeight: content.implicitHeight + Style.spacing.rowPaddingX

    MouseArea {
      id: mouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: drive.openTransfer(row.transfer)
    }

    RowLayout {
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      spacing: Style.space(8)

      Text {
        text: Model.directionGlyph(row.transfer ? row.transfer.direction : "")
        color: row.transfer && row.transfer.outcome === "failed" ? root.urgent : root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.icon
        Layout.alignment: Qt.AlignVCenter
      }

      ColumnLayout {
        id: content
        Layout.fillWidth: true
        spacing: Style.space(1)

        Text {
          Layout.fillWidth: true
          textFormat: Text.PlainText
          text: row.transfer ? row.transfer.path.split("/").pop() : ""
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          elide: Text.ElideMiddle
        }

        Text {
          Layout.fillWidth: true
          textFormat: Text.PlainText
          text: Model.transferMeta(row.transfer)
          color: row.transfer && row.transfer.outcome === "failed" ? root.urgent : root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          elide: Text.ElideRight
        }
      }
    }
  }

  component Label: Text {
    textFormat: Text.PlainText
    color: root.foreground
    opacity: 0.6
    font.family: root.fontFamily
    font.pixelSize: Style.font.bodySmall
  }

  component Value: Text {
    textFormat: Text.PlainText
    color: root.foreground
    font.family: root.fontFamily
    font.pixelSize: Style.font.bodySmall
    elide: Text.ElideLeft
  }

  component Action: Text {
    id: action
    signal clicked()
    property color hoverColor: root.foreground

    textFormat: Text.PlainText
    color: !enabled ? root.dim : (actionMouse.containsMouse ? hoverColor : root.foreground)
    font.family: root.fontFamily
    font.pixelSize: Style.font.bodySmall
    font.underline: actionMouse.containsMouse

    MouseArea {
      id: actionMouse
      anchors.fill: parent
      hoverEnabled: true
      enabled: action.enabled
      cursorShape: Qt.PointingHandCursor
      onClicked: action.clicked()
    }
  }
}
