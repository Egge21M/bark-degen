import QtQuick
import qs.Ui as Ui
import qs.Commons

Ui.Panel {
    id: root
    moduleName: "bark.degen"
    manageIpc: false
    readonly property var backend: bar && bar.shell ? bar.shell.serviceFor(moduleName) : null
    implicitWidth: button.implicitWidth
    implicitHeight: button.implicitHeight

    function configure() { if (backend) backend.configure(settings); }
    onBackendChanged: Qt.callLater(configure)
    onSettingsChanged: Qt.callLater(configure)
    onOpenedChanged: if (opened && backend) backend.refresh()
    Component.onCompleted: Qt.callLater(configure)
    Connections {
        target: root.backend
        function onBusyChanged() { if (!root.backend.busy) Qt.callLater(root.configure); }
    }

    Ui.WidgetButton {
        id: button
        objectName: "diceButton"
        anchors.fill: parent
        bar: root.bar
        labelVisible: false
        hasVisualContent: true
        fixedWidth: root.bar && root.bar.vertical ? root.bar.barSize : Style.space(32)
        DiceIcon {
            anchors.centerIn: parent
            width: Style.space(16)
            height: width
            rolling: root.backend && root.backend.busy && root.backend.action === "play" && !Style.reduceMotion
            color: !root.backend || root.backend.error ? Color.urgent
                : root.backend.lastResult && root.backend.lastResult.win ? Color.accent : button.foreground
        }
        tooltipText: root.backend && root.backend.config
            ? "Bark Dice · " + root.backend.config.network + "\nLeft-click: bet " + root.backend.config.stake
              + " sats · " + root.backend.config.game + "\nRight-click: wallet and settings\n"
              + (root.backend.error || root.backend.message)
            : "Bark Dice · service unavailable; use Omarchy's built-in bar"
        onPressed: function(mouseButton) {
            if (mouseButton === Qt.RightButton) root.toggle();
            else if (mouseButton === Qt.LeftButton && root.backend) {
                if (!root.backend.play()) root.open();
            }
        }
    }

    Ui.KeyboardPanel {
        id: popup
        anchorItem: button
        bar: root.bar
        owner: root
        open: root.opened
        focusTarget: pane
        contentWidth: fittedContentWidth(Style.space(380))
        contentHeight: fittedContentHeight(pane.implicitHeight, Style.space(520))

        WalletPane {
            id: pane
            objectName: "walletPane"
            anchors.fill: parent
            backend: root.backend
            settings: root.settings
            foreground: Color.popups.text
            background: Color.popups.background
            accent: Color.accent
            danger: Color.urgent
            fontFamily: Style.font.family
            fontSize: Style.font.body
            onDismiss: root.close()
            onSaveSettings: function(entry) {
                if (!root.bar || !root.bar.shell || !root.bar.shell.updateEntryInline(root.moduleName, entry)) {
                    if (root.backend) root.backend.error = "Could not save settings.";
                    return;
                }
                root.settings = entry;
                root.configure();
                root.backend.message = "Settings saved. Left-click the dice to play.";
                pane.page = "wallet";
            }
        }
    }
}
