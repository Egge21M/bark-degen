import QtQuick
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: root
    objectName: "confettiWindow"
    signal finished()
    function burst() { animation.restart(); }
    visible: true
    color: "transparent"
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "bark-dice-confetti"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    // An empty input region lets clicks reach the application underneath.
    mask: Region {}

    property real progress: 0
    NumberAnimation {
        id: animation
        target: root
        property: "progress"
        from: 0
        to: 1
        duration: 2800
        onFinished: root.finished()
    }
    Item {
        objectName: "particles"
        anchors.fill: parent
        Repeater {
            model: 80
            Rectangle {
                required property int index
                readonly property real seed: (index * 37 % 83) / 83
                readonly property real t: Math.max(0, root.progress * 1.2 - seed * 0.2)
                width: 5 + seed * 5
                height: width * (index % 3 === 0 ? 1 : 0.5)
                radius: index % 3 === 0 ? width / 2 : 1
                color: ["#a3e635", "#fbbf24", "#38bdf8", "#fb7185", "#c4b5fd"][index % 5]
                x: root.width * ((index * 17 % 80) / 80) + Math.sin(t * 7 + index) * 55
                y: -30 + t * t * (root.height + 100)
                rotation: index * 29 + t * (index % 2 ? 640 : -640)
                opacity: Math.min(1, t * 18) * Math.max(0, Math.min(1, (1 - root.progress) * 5))
            }
        }
    }
}
