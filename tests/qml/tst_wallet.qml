import QtQuick
import QtTest
import "../../omarchy" as Bark

Item {
    width: 400
    height: 580
    QtObject {
        id: backend
        property bool ready: true
        property bool initialized: true
        property bool busy: false
        property bool uncertain: false
        property bool canSpend: !busy && !uncertain
        property var balance: 12000
        property var config: ({stake: 1000, game: "lt5000", network: "signet"})
        property string message: "Ready"
        property string error: ""
        property string action: ""
        property string depositEstimate: ""
        property string invoice: ""
        property string address: ""
        property var operations: []
        property var calls: []
        function topUp(amount) { calls = calls.concat([["fund", amount]]); }
        function receiveArk() { calls = calls.concat([["address"]]); }
        function withdraw(destination, amount) { calls = calls.concat([["withdraw", destination, amount]]); return true; }
        function createWallet() { calls = calls.concat([["create"]]); }
        function refresh() { calls = calls.concat([["refresh"]]); }
        function resume(op) { calls = calls.concat([["resume", op.id]]); }
        function stopWaiting() { calls = calls.concat([["stop"]]); }
    }
    Bark.WalletPane {
        id: pane
        anchors.fill: parent
        backend: backend
        settings: ({id: "bark.degen", network: "signet", api: "https://example.com"})
    }
    SignalSpy { id: saved; target: pane; signalName: "saveSettings" }
    TestCase {
        name: "BarkWalletPane"
        when: windowShown
        function control(name) { var c = findChild(pane, name); verify(c !== null, name); return c; }
        function click(name) { var c = control(name); verify(c.visible); mouseClick(c); }
        function init() {
            backend.busy = false; backend.uncertain = false; backend.calls = [];
            backend.error = ""; backend.initialized = true; backend.operations = [];
            pane.page = "wallet"; saved.clear();
        }
        function test_topup_and_withdraw() {
            click("topUpButton"); compare(pane.page, "topup");
            control("depositAmount").text = "5000";
            click("invoiceButton"); compare(backend.calls[0], ["fund", "5000"]);
            click("arkAddressButton"); compare(backend.calls[1], ["address"]);
            click("walletTab"); click("withdrawButton"); compare(pane.page, "withdraw");
            control("destination").text = "lnbc1invoice";
            control("withdrawAmount").text = "";
            click("sendButton"); compare(backend.calls[2], ["withdraw", "lnbc1invoice", ""]);
            compare(control("destination").text, "");
            verify(!control("sendButton").enabled);
        }
        function test_settings_validate_and_preserve_connection() {
            click("settingsTab"); compare(pane.page, "settings");
            control("stake").text = "0";
            click("saveSettingsButton"); compare(saved.count, 0); verify(backend.error.length > 0);
            control("stake").text = "2500"; control("gameMode").currentIndex = 2;
            verify(control("notificationsToggle").checked);
            verify(control("confettiToggle").checked);
            control("notificationsToggle").checked = false;
            control("confettiToggle").checked = false;
            click("saveSettingsButton"); compare(saved.count, 1);
            compare(saved.signalArguments[0][0].stake, 2500);
            compare(saved.signalArguments[0][0].game, "lt1000");
            compare(saved.signalArguments[0][0].api, "https://example.com");
            compare(saved.signalArguments[0][0].network, "signet");
            compare(saved.signalArguments[0][0].notifications, false);
            compare(saved.signalArguments[0][0].confetti, false);
        }
        function test_busy_and_unknown_payment_disable_send() {
            click("withdrawButton"); control("destination").text = "ark1recipient";
            backend.busy = true; verify(!control("sendButton").enabled);
            backend.busy = false; backend.uncertain = true; verify(!control("sendButton").enabled);
        }
        function test_new_wallet_and_recovery() {
            backend.initialized = false; click("createWalletButton"); compare(backend.calls[0], ["create"]);
            backend.operations = [{id: "saved-deposit", kind: "fund", state: "awaiting_payment", resumable: true}];
            wait(30); click("resumeButton"); compare(backend.calls[1], ["resume", "saved-deposit"]);
            compare(pane.page, "topup");
        }
        function test_wallet_render() {
            wait(50);
            verify(pane.width > 300);
            verify(control("balanceLabel").text.indexOf("12") >= 0);
            grabImage(pane).save("/tmp/bark-wallet-pane.png");
        }
    }
}
