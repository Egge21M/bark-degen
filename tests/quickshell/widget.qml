import QtQuick
import Quickshell
import "omarchy" as Bark

ShellRoot {
    id: root
    property int step: 0
    property int ticks: 0
    function check(condition, reason) {
        if (!condition) { console.error("FAIL: " + reason); Qt.exit(1); throw new Error(reason); }
    }
    function named(item, name) {
        if (item.objectName === name) return item;
        var children = item.data || item.children || [];
        for (var i = 0; i < children.length; ++i) {
            var result = named(children[i], name);
            if (result) return result;
        }
        return null;
    }
    Bark.Service { id: backend }
    QtObject {
        id: host
        property var saved: null
        function serviceFor(id) { return id === "bark.degen" ? backend : null; }
        function updateEntryInline(id, entry) { saved = entry; widget.settings = entry; return true; }
    }
    QtObject {
        id: bar
        property var shell: host
        property bool vertical: false
        property int barSize: 28
        property string position: "top"
        property string fontFamily: "monospace"
        property color barForeground: "#e5e7eb"
        property color foreground: barForeground
        property color background: "#16191f"
        property color urgent: "#fb7185"
        property bool foregroundAnimationEnabled: false
        property var activePopout: null
        property var clickTargets: []
        function registerClickTarget(target) { clickTargets = clickTargets.concat([target]); }
        function unregisterClickTarget(target) { clickTargets = clickTargets.filter(function(x) { return x !== target; }); }
        function requestPopout(owner) { activePopout = owner; }
        function releasePopout(owner) { if (activePopout === owner) activePopout = null; }
        function showTooltip(target, text) {}
        function hideTooltip(target) {}
    }
    FloatingWindow {
        id: window
        visible: true
        implicitWidth: 400
        implicitHeight: 28
        color: bar.background
        Bark.Widget {
            id: widget
            bar: bar
            width: implicitWidth
            height: implicitHeight
            settings: ({network: "signet", stake: 2500, game: "lt1000", binary: Quickshell.env("BARK_TEST_BINARY")})
        }
        Bark.Widget {
            id: secondWidget
            x: 40
            bar: bar
            width: implicitWidth
            height: implicitHeight
            settings: widget.settings
        }
    }
    Timer {
        interval: 50
        running: true
        repeat: true
        onTriggered: {
            root.ticks++;
            if (root.ticks > 300) { console.error("FAIL: widget timed out at " + root.step); Qt.exit(1); }
            if (!backend.ready || backend.busy || backend.balance === null) return;
            var button = root.named(widget, "diceButton");
            root.check(button !== null, "real widget loaded");
            switch (root.step) {
            case 0:
                root.check(backend.config.network === "signet" && backend.config.stake === 2500, "host settings arrive before client starts");
                button.triggerPress(Qt.RightButton);
                root.check(widget.opened, "right click opens wallet/settings popup");
                root.step++; break;
            case 1:
                root.check(bar.activePopout === widget, "popup registered with Omarchy bar");
                var pane = root.named(widget, "walletPane");
                root.check(pane !== null, "wallet/settings content loaded in real popup");
                pane.saveSettings(Object.assign({}, widget.settings, {game: "lt2500", stake: 3000}));
                root.check(host.saved.game === "lt2500" && backend.config.stake === 3000, "settings persist through scoped host API");
                widget.close();
                root.check(!widget.opened, "popup can close without destroying backend");
                button.triggerPress(Qt.LeftButton);
                root.named(secondWidget, "diceButton").triggerPress(Qt.LeftButton);
                root.step++; break;
            case 2:
                root.check(backend.lastResult && backend.lastResult.roll === 42, "left click runs a verified game");
                root.check(button.tooltipText.indexOf("WIN") >= 0, "result shown in tooltip");
                console.log("PASS: widget with real Omarchy popup components");
                Qt.quit();
            }
        }
    }
}
