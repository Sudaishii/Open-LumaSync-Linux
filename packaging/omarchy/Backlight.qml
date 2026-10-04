import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

BarWidget {
    id: root
    moduleName: "snzhy.backlight"
    property bool popupOpen: false
    readonly property bool opened: popupOpen
    property var controllerState: ({available: false, activity: "Checking controller…"})
    property string actionError: ""
    readonly property string helper: Quickshell.env("HOME") + "/.local/bin/snzhy-backlight"
    function open() { popupOpen = true; refresh() }
    function close() { popupOpen = false }
    function refresh() { if (!poll.running) poll.running = true }
    function control(command) {
        if (action.running) return
        actionError = ""
        action.command = [helper, command]
        action.running = true
    }
    implicitWidth: button.implicitWidth
    implicitHeight: button.implicitHeight

    BarIconButton {
        id: button
        anchors.fill: parent
        bar: root.bar
        tooltipText: "Backlight · " + (root.controllerState.activity || "Controller stopped")
        onPressed: function(mouseButton) { if (root.popupOpen) root.close(); else root.open() }
        iconComponent: Component {
            Canvas {
                property color ink: root.bar.barForeground
                onInkChanged: requestPaint()
                onPaint: {
                    const c = getContext("2d")
                    c.reset(); c.strokeStyle = ink; c.lineWidth = 1.5
                    c.strokeRect(width * .14, height * .2, width * .72, height * .53)
                    c.beginPath(); c.moveTo(width * .5, height * .73); c.lineTo(width * .5, height * .85)
                    c.moveTo(width * .32, height * .86); c.lineTo(width * .68, height * .86); c.stroke()
                    c.beginPath(); c.moveTo(width * .04, height * .26); c.lineTo(width * .04, height * .64)
                    c.moveTo(width * .96, height * .26); c.lineTo(width * .96, height * .64); c.stroke()
                }
            }
        }
    }
    Process {
        id: poll
        command: [root.helper, "status"]
        stdout: StdioCollector {
            onStreamFinished: {
                try { root.controllerState = JSON.parse(text) }
                catch (_) { root.controllerState = {available: false, activity: "Status unavailable"} }
            }
        }
    }
    Process {
        id: action
        stdout: StdioCollector {
            onStreamFinished: {
                try { const reply = JSON.parse(text); root.actionError = reply.error || "" }
                catch (_) { root.actionError = "Controller command failed" }
            }
        }
        onExited: root.refresh()
    }
    Timer { interval: 2000; running: true; repeat: true; triggeredOnStart: true; onTriggered: root.refresh() }

    PopupCard {
        id: popup
        anchorItem: root
        bar: root.bar
        owner: root
        open: root.popupOpen
        contentWidth: popup.fittedContentWidth(Style.space(340))
        contentHeight: popup.fittedContentHeight(column.implicitHeight)
        Column {
            id: column
            width: parent.width
            spacing: Style.space(8)
            Text {
                text: "Backlight"
                color: root.bar.foreground
                font.family: root.bar.fontFamily
                font.pixelSize: Style.font.subtitle
                font.bold: true
            }
            Text {
                width: parent.width
                textFormat: Text.PlainText
                text: root.actionError || root.controllerState.activity || "Controller stopped"
                wrapMode: Text.Wrap
                color: root.bar.foreground
                font.family: root.bar.fontFamily
                font.pixelSize: Style.font.bodySmall
            }
            Button { text: "Open controller"; foreground: root.bar.foreground; focusable: true; enabled: !action.running; onClicked: root.control("show") }
            PanelSeparator { width: parent.width; foreground: root.bar.foreground }
            Repeater {
                model: [{label: "Lighting", command: "lighting"}, {label: "Screen sync", command: "screen"}, {label: "Audio sync", command: "audio"}]
                Button {
                    required property var modelData
                    width: column.width
                    text: modelData.label
                    leftAlign: true
                    focusable: true
                    foreground: root.bar.foreground
                    selected: !!root.controllerState.powered && root.controllerState.mode === modelData.command
                    enabled: !action.running && !root.controllerState.busy
                    onClicked: root.control(modelData.command)
                }
            }
            Row {
                spacing: Style.space(6)
                Button { text: "Stop sync"; foreground: root.bar.foreground; focusable: true; enabled: !action.running; onClicked: root.control("stop") }
                Button { text: root.controllerState.powered ? "Turn off" : "Turn on"; foreground: root.bar.foreground; focusable: true; enabled: !action.running; onClicked: root.control("toggle") }
            }
            Row {
                spacing: Style.space(6)
                Button { text: "−"; foreground: root.bar.foreground; focusable: true; enabled: !action.running; onClicked: root.control("brightness-down") }
                Text { text: "Brightness " + Math.round((root.controllerState.brightness || 0) / 255 * 100) + "%"; anchors.verticalCenter: parent.verticalCenter; color: root.bar.foreground; font.family: root.bar.fontFamily; font.pixelSize: Style.font.bodySmall }
                Button { text: "+"; foreground: root.bar.foreground; focusable: true; enabled: !action.running; onClicked: root.control("brightness-up") }
            }
            Text { text: "Screen sync uses your display’s colors."; color: root.bar.foreground; font.family: root.bar.fontFamily; font.pixelSize: Style.font.caption }
            Repeater {
                model: root.controllerState.displays || []
                Button {
                    required property string modelData
                    text: "Monitor · " + modelData
                    foreground: root.bar.foreground
                    focusable: true
                    selected: root.controllerState.output === modelData
                    enabled: !action.running
                    onClicked: root.control("display:" + modelData)
                }
            }
            Row {
                spacing: Style.space(6)
                Button { text: "Balanced"; foreground: root.bar.foreground; focusable: true; enabled: !action.running; onClicked: root.control("preset:screen-balanced") }
                Button { text: "Gaming"; foreground: root.bar.foreground; focusable: true; enabled: !action.running; onClicked: root.control("preset:screen-gaming") }
            }
            Button {
                text: "Resume at login · " + (root.controllerState.resumeEnabled ? "On" : "Off")
                foreground: root.bar.foreground
                focusable: true
                selected: !!root.controllerState.resumeEnabled
                enabled: !action.running
                onClicked: root.control("resume-toggle")
            }
        }
    }
}
