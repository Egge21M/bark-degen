import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import "Model.js" as Model

FocusScope {
    id: root
    property var backend: null
    property var settings: ({})
    property string screenName: ""
    property string page: "wallet"
    property color foreground: "#e5e7eb"
    property color background: "#16191f"
    property color accent: "#a3e635"
    property color danger: "#fb7185"
    property string fontFamily: "monospace"
    property real fontSize: 13
    signal saveSettings(var entry)
    signal dismiss()
    readonly property bool idle: backend && backend.ready && !backend.busy
    implicitHeight: header.implicitHeight + tabs.implicitHeight + body.implicitHeight + 30
    Keys.onEscapePressed: dismiss()
    Rectangle { anchors.fill: parent; color: root.background }

    component Label: Controls.Label {
        Layout.fillWidth: true
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: root.fontSize
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
    }
    component Button: Controls.Button {
        font.family: root.fontFamily
        font.pixelSize: root.fontSize
        palette.button: Qt.lighter(root.background, 1.4)
        palette.buttonText: root.foreground
        palette.highlight: root.accent
        palette.light: root.accent
    }
    component Field: Controls.TextField {
        Layout.fillWidth: true
        font.family: root.fontFamily
        font.pixelSize: root.fontSize
        color: root.foreground
        placeholderTextColor: Qt.darker(root.foreground, 1.4)
        selectByMouse: true
        background: Rectangle {
            color: Qt.lighter(root.background, 1.25)
            radius: 4
            border.color: parent.activeFocus ? root.accent : Qt.darker(root.foreground, 2.5)
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 10
        RowLayout {
            id: header
            DiceIcon { color: root.accent }
            Label { text: "Bark Dice"; font.bold: true; font.pixelSize: root.fontSize * 1.2 }
            Button { objectName: "closeButton"; text: "×"; onClicked: root.dismiss(); Accessible.name: "Close wallet" }
        }
        RowLayout {
            id: tabs
            Button { objectName: "walletTab"; text: "Wallet"; highlighted: root.page !== "settings"; onClicked: root.page = "wallet" }
            Button {
                objectName: "settingsTab"
                text: "Settings"
                highlighted: root.page === "settings"
                onClicked: {
                    if (root.backend && root.backend.config) {
                        stake.text = String(root.backend.config.stake);
                        game.currentIndex = Model.games.indexOf(root.backend.config.game);
                        notifications.checked = root.backend.config.notifications !== false;
                        confetti.checked = root.backend.config.confetti !== false;
                    }
                    root.page = "settings";
                }
            }
            Item { Layout.fillWidth: true }
            Label { Layout.fillWidth: false; text: root.backend && root.backend.config ? root.backend.config.network : ""; opacity: 0.7 }
        }

        Controls.ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            clip: true

            ColumnLayout {
                id: body
                width: parent.width
                spacing: 12
                Label { visible: !root.backend; text: "Wallet service unavailable. Enable Bark Dice under the built-in Omarchy bar." }
                Label {
                    objectName: "balanceLabel"
                    visible: root.page !== "settings"
                    text: root.backend && root.backend.balance !== null ? root.backend.balance.toLocaleString(Qt.locale(), "f", 0) + " sats" : "— sats"
                    font.pixelSize: root.fontSize * 2
                    font.bold: true
                }
                Label {
                    visible: root.page === "wallet" && root.backend && !root.backend.initialized
                    text: "Create your Bark wallet to top up and play."
                }
                Button {
                    objectName: "createWalletButton"
                    visible: root.page === "wallet" && root.backend && !root.backend.initialized
                    text: "Create wallet"
                    enabled: root.idle
                    onClicked: root.backend.createWallet()
                }
                RowLayout {
                    visible: root.page === "wallet" && root.backend && root.backend.initialized
                    Button { objectName: "topUpButton"; text: "Top up"; onClicked: root.page = "topup" }
                    Button { objectName: "withdrawButton"; text: "Withdraw"; onClicked: root.page = "withdraw" }
                    Button { objectName: "refreshButton"; text: "↻"; enabled: root.idle; onClicked: root.backend.refresh(); Accessible.name: "Refresh balance" }
                }
                Label {
                    visible: root.page === "wallet" && root.backend && root.backend.config
                    text: root.backend && root.backend.config ? "Left-click the bar icon to bet " + root.backend.config.stake + " sats · " + root.backend.config.game : ""
                }

                ColumnLayout {
                    visible: root.page === "topup"
                    Layout.fillWidth: true
                    Label { text: "Top up"; font.bold: true }
                    Field { id: depositAmount; objectName: "depositAmount"; placeholderText: "Amount in sats"; inputMethodHints: Qt.ImhDigitsOnly; enabled: root.idle }
                    RowLayout {
                        Button { objectName: "invoiceButton"; text: "Lightning invoice"; enabled: root.idle; onClicked: root.backend.topUp(depositAmount.text) }
                        Button { objectName: "arkAddressButton"; text: "Ark address"; enabled: root.idle; onClicked: root.backend.receiveArk() }
                    }
                    Label { visible: text !== ""; text: root.backend ? root.backend.depositEstimate : ""; opacity: 0.7 }
                    Controls.TextArea {
                        id: receivingText
                        objectName: "receivingText"
                        Layout.fillWidth: true
                        Layout.preferredHeight: 100
                        text: root.backend ? (root.backend.invoice || root.backend.address) : ""
                        visible: text !== ""
                        readOnly: true
                        selectByMouse: true
                        wrapMode: TextEdit.WrapAnywhere
                        textFormat: TextEdit.PlainText
                        color: root.foreground
                        font.family: root.fontFamily
                        font.pixelSize: root.fontSize
                        background: Rectangle { color: Qt.lighter(root.background, 1.3); radius: 4 }
                    }
                    Button { text: "Copy"; visible: receivingText.text !== ""; onClicked: { receivingText.selectAll(); receivingText.copy(); receivingText.deselect(); } }
                    Label { text: "Lightning top ups are claimed while this wallet is waiting. Ark deposits appear after a refresh."; opacity: 0.7 }
                    Button { objectName: "stopWaitingButton"; text: "Stop waiting"; visible: root.backend && root.backend.busy && root.backend.action === "fund"; onClicked: root.backend.stopWaiting() }
                }

                ColumnLayout {
                    visible: root.page === "withdraw"
                    Layout.fillWidth: true
                    Label { text: "Withdraw"; font.bold: true }
                    Field { id: destination; objectName: "destination"; placeholderText: "Ark / Bitcoin address or Lightning invoice"; enabled: root.idle }
                    Field { id: amount; objectName: "withdrawAmount"; placeholderText: "Sats (optional for an invoice with an amount)"; inputMethodHints: Qt.ImhDigitsOnly; enabled: root.idle }
                    Label { text: "Send pays this destination immediately. Fees may be additional."; opacity: 0.7 }
                    Button {
                        objectName: "sendButton"
                        text: "Send"
                        enabled: root.backend && root.backend.canSpend && destination.text.trim() !== ""
                        onClicked: if (root.backend.withdraw(destination.text, amount.text)) { destination.clear(); amount.clear(); }
                    }
                }

                ColumnLayout {
                    visible: root.page === "settings"
                    Layout.fillWidth: true
                    Label { text: "Game mode" }
                    Controls.ComboBox {
                        id: game
                        objectName: "gameMode"
                        Layout.fillWidth: true
                        model: Model.gameLabels
                        enabled: root.idle
                        palette.button: Qt.lighter(root.background, 1.4)
                        palette.buttonText: root.foreground
                        palette.text: root.foreground
                        palette.base: root.background
                        font.family: root.fontFamily
                        font.pixelSize: root.fontSize
                    }
                    Label { text: "Stake per click (sats)" }
                    Field { id: stake; objectName: "stake"; text: "1000"; inputMethodHints: Qt.ImhDigitsOnly; enabled: root.idle }
                    Label { text: "Each left-click places one bet using these settings."; opacity: 0.7 }
                    Controls.CheckBox {
                        id: notifications
                        objectName: "notificationsToggle"
                        text: "Win / loss notifications"
                        checked: true
                        enabled: root.idle
                        palette.windowText: root.foreground
                        font.family: root.fontFamily
                        font.pixelSize: root.fontSize
                    }
                    Controls.CheckBox {
                        id: confetti
                        objectName: "confettiToggle"
                        text: "Confetti on wins"
                        checked: true
                        enabled: root.idle
                        palette.windowText: root.foreground
                        font.family: root.fontFamily
                        font.pixelSize: root.fontSize
                    }
                    Label { text: "Confetti respects reduced motion and lets clicks pass through."; opacity: 0.7 }
                    Button {
                        objectName: "saveSettingsButton"
                        text: "Save settings"
                        enabled: root.idle
                        onClicked: {
                            try {
                                var entry = Object.assign({}, root.settings, {id: "bark.degen", game: Model.games[game.currentIndex], stake: Model.sats(stake.text),
                                    notifications: notifications.checked, confetti: confetti.checked});
                                root.saveSettings(entry);
                            } catch (e) { root.backend.error = String(e.message || e); }
                        }
                    }
                }

                Label { objectName: "statusLabel"; text: root.backend ? root.backend.message : ""; color: root.accent }
                Label { objectName: "errorLabel"; visible: text !== ""; text: root.backend ? root.backend.error : ""; color: root.danger }
                Button {
                    objectName: "retryButton"
                    text: "Retry / refresh"
                    visible: root.backend && root.backend.error !== ""
                    enabled: root.backend && !root.backend.busy
                    onClicked: root.backend.refresh()
                }
                Label { visible: root.backend && root.backend.uncertain; text: "A payment needs checking. Resume it below before placing another bet or withdrawal." }
                Repeater {
                    model: root.backend ? root.backend.operations : []
                    delegate: ColumnLayout {
                        id: operationRow
                        required property var modelData
                        Layout.fillWidth: true
                        Label { text: operationRow.modelData.kind + " · " + operationRow.modelData.state; font.bold: true }
                        Label { text: operationRow.modelData.id; font.pixelSize: root.fontSize * 0.8; opacity: 0.7 }
                        Button {
                            objectName: "resumeButton"
                            text: operationRow.modelData.kind === "withdraw" ? "Check withdrawal" : operationRow.modelData.kind === "play" ? "Resume bet" : "Resume top up"
                            enabled: root.idle && operationRow.modelData.resumable
                            onClicked: { if (operationRow.modelData.kind === "fund") root.page = "topup"; root.backend.resume(operationRow.modelData, root.screenName); }
                        }
                    }
                }
            }
        }
    }
}
