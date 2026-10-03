import QtQuick

Item {
    id: root
    property color color: "white"
    property bool rolling: false
    implicitWidth: 18
    implicitHeight: 18
    Rectangle {
        anchors.fill: parent
        color: "transparent"
        radius: width * 0.18
        border.color: root.color
        border.width: 1.5
    }
    Repeater {
        model: [[0.28, 0.28], [0.72, 0.28], [0.5, 0.5], [0.28, 0.72], [0.72, 0.72]]
        Rectangle {
            required property var modelData
            width: root.width * 0.14
            height: width
            radius: width / 2
            x: root.width * modelData[0] - width / 2
            y: root.height * modelData[1] - height / 2
            color: root.color
        }
    }
    NumberAnimation on rotation {
        running: root.rolling
        from: 0; to: 360; duration: 1200; loops: Animation.Infinite
        onStopped: root.rotation = 0
    }
}
