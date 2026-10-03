import QtQuick
import Quickshell
import "../../omarchy" as Bark

ShellRoot {
    id: root
    property int step: 0
    property int ticks: 0
    property var settings: ({network: "signet", confetti: false, binary: Quickshell.env("BARK_TEST_BINARY")})
    property bool sawInvoice: false
    function check(condition, reason) {
        if (!condition) { console.error("FAIL: " + reason); Qt.exit(1); throw new Error(reason); }
    }
    Bark.Service { id: service }
    Connections {
        target: service
        function onInvoiceChanged() { if (service.invoice === "lnbc-test-invoice") root.sawInvoice = true; }
    }
    Component.onCompleted: service.configure(settings)
    Timer {
        interval: 30
        running: true
        repeat: true
        onTriggered: {
            root.ticks++;
            if (root.ticks > 600) { console.error("FAIL: timed out at step " + root.step + " " + service.error); Qt.exit(1); }
            if (service.busy) return;
            switch (root.step) {
            case 0:
                if (service.balance === null) return;
                root.check(service.canSpend, "initial wallet is ready");
                root.check(service.play(), "left click starts a bet");
                root.check(!service.play(), "second click must not send");
                root.step++; break;
            case 1:
                root.check(service.lastResult && service.lastResult.win, "verified result reaches UI");
                service.topUp("5000"); root.step++; break;
            case 2:
                root.check(root.sawInvoice, "invoice streams before child exits");
                root.check(service.invoice === "", "settled invoice clears");
                root.check(service.balance === 14900, "deposit updates balance");
                service.receiveArk(); root.step++; break;
            case 3:
                root.check(service.address === "ark-test-address", "Ark receiving address");
                root.check(service.withdraw("lnbc-test-destination", ""), "invoice withdrawal starts");
                root.check(!service.withdraw("lnbc-test-destination", ""), "duplicate withdrawal rejected");
                root.step++; break;
            case 4:
                root.check(service.message.indexOf("completed") >= 0, "withdrawal result");
                service.configure(Object.assign({}, root.settings, {game: "lt0200", stake: 2000}));
                service.play(); root.step++; break;
            case 5:
                root.check(service.uncertain, "interrupted outgoing payment recovered from journal");
                root.check(!service.play(), "unknown payment blocks a new bet");
                root.check(service.operations[0].id === "saved-bet", "resume ID is retained");
                service.resume(service.operations[0]); root.step++; break;
            case 6:
                root.check(!service.uncertain && service.canSpend, "resume reconciles instead of resending");
                service.configure(Object.assign({}, root.settings, {binary: "/nonexistent/bark-degen"}));
                root.step++; break;
            case 7:
                root.check(!service.ready && !service.canSpend && service.error.indexOf("Cannot start") >= 0, "missing binary is visible and does not leave busy stuck");
                console.log("PASS: real Quickshell service integration");
                Qt.quit();
            }
        }
    }
}
