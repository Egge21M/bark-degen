import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model

// One instance for the entire shell, shared by every monitor's widget.
Item {
    id: root
    property var shell: null
    property var config: null
    property bool busy: false
    property bool ready: false
    property bool initialized: false
    property var balance: null
    property string error: ""
    property string message: "Right-click to open your wallet."
    property string action: ""
    property string address: ""
    property string invoice: ""
    property string fundingId: ""
    property string depositEstimate: ""
    property var operations: []
    property var lastResult: null
    property bool receivedSnapshot: false
    property bool gotEvent: false
    property bool needsRefresh: false
    readonly property bool uncertain: Model.hasUncertainPayment(operations)
    readonly property bool canSpend: ready && initialized && !busy && !uncertain
    readonly property string bundledBinary: decodeURIComponent(String(Qt.resolvedUrl("../bin/bark-degen")).replace(/^file:\/\//, ""))

    function configure(settings) {
        try {
            var next = Model.config(settings, bundledBinary);
            if (JSON.stringify(next) === JSON.stringify(config)) return;
            if (busy) return; // A command always finishes against its original wallet.
            var walletChanged = !config || next.network !== config.network || next.dataDir !== config.dataDir
                || next.binary !== config.binary || next.api !== config.api || next.arkServer !== config.arkServer || next.esplora !== config.esplora;
            config = next;
            if (walletChanged) {
                ready = false; initialized = false; balance = null; operations = [];
                address = ""; invoice = ""; fundingId = ""; lastResult = null;
                error = ""; needsRefresh = true;
                start(["status"], "snapshot", true);
            }
        } catch (e) { error = String(e.message || e); }
    }

    function start(args, kind, preserveError) {
        if (busy || !config) return false;
        busy = true;
        action = kind;
        gotEvent = false;
        receivedSnapshot = false;
        if (!preserveError) error = "";
        process.command = Model.command(config, args);
        process.running = true;
        startupCheck.restart();
        return true;
    }

    function refresh() {
        if (busy) return;
        needsRefresh = true;
        start(["status"], "snapshot", false);
    }

    function createWallet() { start(["fund"], "address", false); }
    function receiveArk() {
        if (start(["fund"], "address", false)) { invoice = ""; fundingId = ""; depositEstimate = ""; }
    }
    function topUp(amount) {
        try {
            var n = Model.sats(amount);
            if (start(["fund", String(n)], "fund", false)) {
                invoice = ""; fundingId = ""; depositEstimate = "";
                message = "Creating a Lightning invoice…";
            }
        } catch (e) { error = String(e.message || e); }
    }
    function withdraw(destination, amount) {
        if (!canSpend) return false;
        try {
            if (start(Model.withdrawal(destination, amount), "withdraw", false)) {
                message = "Sending withdrawal…";
                return true;
            }
        } catch (e) { error = String(e.message || e); }
        return false;
    }
    function play() {
        if (!canSpend) return false;
        if (start(["play", String(config.stake), "--game", config.game], "play", false)) {
            lastResult = null;
            message = "Rolling · " + config.stake + " sats…";
            return true;
        }
        return false;
    }
    function resume(operation) {
        try {
            if (start(Model.resume(operation), operation.kind, false)) message = "Checking saved " + operation.kind + "…";
        } catch (e) { error = String(e.message || e); }
    }
    function stopWaiting() {
        if (busy && action === "fund") process.signal(2); // SIGINT: SDK closes cleanly; invoice is journaled.
    }

    function event(value) {
        gotEvent = true;
        if (value.balance_sat !== undefined) balance = value.balance_sat;
        switch (value.event) {
        case "snapshot":
            initialized = value.initialized;
            operations = value.operations;
            ready = true;
            receivedSnapshot = true;
            break;
        case "error": error = value.message; break;
        case "address": address = value.address; initialized = true; message = "Ark address ready."; break;
        case "deposit_estimate": depositEstimate = "Estimated net " + value.net_sat + " sats · fee " + value.fee_sat + " sats"; break;
        case "deposit": invoice = value.invoice; fundingId = value.id; message = "Waiting for your Lightning payment…"; break;
        case "funded": message = "Top up received."; invoice = ""; fundingId = ""; break;
        case "bet_status": message = "Bet: " + value.state; break;
        case "bet_result":
            lastResult = value;
            message = (value.win ? "WIN" : "LOSE") + " · roll " + String(value.roll).padStart(4, "0")
                + (value.win ? " · house reports " + value.payout_sat + " sats paid" : "") + " · verified";
            break;
        case "withdrawal_status": message = "Withdrawal: " + value.state + (value.result ? " · " + value.result : ""); break;
        }
    }

    function finished(code) {
        startupCheck.stop();
        var kind = action;
        if (code !== 0 && !error) error = "Wallet command failed. " + diagnostics.text.trim().slice(-1200);
        if (code === 0 && !gotEvent) error = "The wallet binary did not emit JSON. Rebuild and reinstall bark-degen.";
        busy = false;
        action = "";
        if (kind === "snapshot") {
            if (!receivedSnapshot) {
                ready = false;
                if (!error) error = "Wallet snapshot missing. Rebuild and reinstall bark-degen.";
            }
            if (receivedSnapshot && needsRefresh && initialized) {
                needsRefresh = false;
                Qt.callLater(function() { root.start(["fund", "--balance"], "balance", true); });
            } else needsRefresh = false;
        } else if (kind !== "balance") {
            // Recover from persisted state even if the child died before emitting its ID.
            ready = false;
            needsRefresh = kind === "withdraw" && code === 0;
            Qt.callLater(function() { root.start(["status"], "snapshot", true); });
        }
    }

    Process {
        id: process
        stdout: SplitParser {
            onRead: function(line) {
                try { root.event(JSON.parse(line)); }
                catch (e) { root.error = "Invalid response from wallet client: " + String(e); }
            }
        }
        stderr: StdioCollector { id: diagnostics; waitForEnd: true }
        onExited: function(code) { root.finished(code); }
    }

    Timer {
        id: startupCheck
        interval: 2000
        onTriggered: if (root.busy && !process.running) {
            root.error = "Cannot start bark-degen. Build/install the bundled binary or set the widget's binary path.";
            root.ready = false; root.busy = false; root.action = "";
        }
    }
    Timer {
        interval: 60000
        repeat: true
        running: root.ready && root.initialized
        onTriggered: if (!root.busy) root.start(["fund", "--balance"], "balance", true)
    }
}
