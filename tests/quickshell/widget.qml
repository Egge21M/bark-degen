import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import "omarchy" as Bark

ShellRoot {
    id: root
    property int step: 0
    property int ticks: 0
    property int effectEndTick: 0
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
                secondWidget.close();
                root.step++; break;
            case 2:
                root.check(backend.lastResult && backend.lastResult.roll === 42, "left click runs a verified game");
                root.check(button.tooltipText.indexOf("WIN") >= 0, "result shown in tooltip");
                root.check(!widget.opened && !secondWidget.opened, "both panels closed at settlement");
                root.check(root.named(widget, "resultLabel").text === "WIN", "win visible in bar");
                root.check(backend.celebrating, "closed-panel win starts confetti");
                var effect = root.named(backend, "celebrationLoader").item;
                root.check(effect !== null && effect.visible, "actual layer-shell confetti window visible");
                root.check(effect.screen.name === window.screen.name, "confetti on clicked monitor");
                root.check(effect.mask.width === 0 && effect.mask.height === 0, "confetti input region empty");
                root.check(effect.WlrLayershell.keyboardFocus === WlrKeyboardFocus.None, "confetti does not take keyboard focus");
                if (effect.progress < 0.4) return;
                root.named(effect.contentItem, "particles").grabToImage(function(image) { image.saveToFile("/tmp/bark-confetti.png"); });
                widget.grabToImage(function(image) { image.saveToFile("/tmp/bark-result-widget.png"); });
                backend.event(backend.lastResult);
                root.check(backend.announcedResults.length === 1, "duplicate receipt suppressed");
                root.effectEndTick = root.ticks + 70;
                root.step++; break;
            case 3:
                if (backend.celebrating) {
                    root.check(root.ticks < root.effectEndTick, "confetti must finish automatically");
                    return;
                }
                root.check(root.named(backend, "celebrationLoader").item === null, "confetti window unloaded");
                backend.event({event: "bet_result", id: "loss", win: false, roll: 9999, payout_sat: 1970, balance_sat: 9000});
                root.check(!backend.celebrating, "loss does not celebrate");
                root.check(root.named(widget, "resultLabel").text === "LOSS", "loss visible in bar");
                Style.reduceMotion = true;
                backend.event({event: "bet_result", id: "reduced", win: true, roll: 3, payout_sat: 1970});
                root.check(!backend.celebrating, "reduced motion disables confetti");
                Style.reduceMotion = false;
                pane = root.named(widget, "walletPane");
                pane.saveSettings(Object.assign({}, widget.settings, {notifications: false, confetti: false}));
                backend.event({event: "bet_result", id: "disabled", win: true, roll: 4, payout_sat: 1970});
                root.check(!backend.celebrating, "confetti setting respected");
                root.step++; break;
            case 4:
                console.log("PASS: widget with real Omarchy popup components");
                Qt.quit();
            }
        }
    }
}
