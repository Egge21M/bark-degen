// Shared by the QML service and the command-contract tests. Never construct shell strings.
var games = ["lt5000", "lt2500", "lt1000", "lt0200"];
var gameLabels = ["50% · roll < 5000", "25% · roll < 2500", "10% · roll < 1000", "2% · roll < 200"];

function sats(value) {
    var text = String(value).trim();
    if (!/^[1-9][0-9]*$/.test(text) || Number(text) > 2100000000000000)
        throw new Error("Enter a positive whole number of sats.");
    return Number(text);
}

function config(settings, binary) {
    var s = settings || {};
    var game = s.game || "lt5000";
    var network = s.network || "mainnet";
    if (games.indexOf(game) < 0) throw new Error("Unknown game mode.");
    if (["mainnet", "signet"].indexOf(network) < 0) throw new Error("Unknown wallet network.");
    return {game: game, stake: sats(s.stake === undefined ? 1000 : s.stake), network: network,
        binary: s.binary || binary, dataDir: s.dataDir || "", api: s.api || "https://barkdice.com",
        arkServer: s.arkServer || "", esplora: s.esplora || "",
        notifications: s.notifications !== false, confetti: s.confetti !== false};
}

function resultMessage(result) {
    return (result.win ? "WIN" : "LOSS") + " · roll " + String(result.roll).padStart(4, "0")
        + (result.win ? " · house reports " + result.payout_sat + " sats paid" : "") + " · verified";
}

function command(c, args) {
    var result = [c.binary, "--json", "--network", c.network, "--api", c.api];
    if (c.dataDir) result.push("--data-dir", c.dataDir);
    if (c.arkServer) result.push("--ark-server", c.arkServer);
    if (c.esplora) result.push("--esplora", c.esplora);
    return result.concat(args);
}

function withdrawal(destination, amount) {
    var to = String(destination).trim();
    if (!to || /\s/.test(to) || to.charAt(0) === "-") throw new Error("Enter an Ark address, Lightning invoice, or Bitcoin address.");
    // `--` also prevents a destination from being interpreted as a CLI option.
    var args = ["withdraw", "--", to];
    if (String(amount).trim()) args.push(String(sats(amount)));
    return args;
}

function resume(operation) {
    if (!operation.resumable) throw new Error("This operation cannot be resumed.");
    if (operation.kind === "fund") return ["fund", "--resume", operation.id];
    if (operation.kind === "withdraw") return ["withdraw", "--status", operation.id];
    if (operation.kind === "play") return ["play", "--resume", operation.id];
    throw new Error("Unknown saved operation.");
}

function hasUncertainPayment(operations) {
    return operations.some(function(op) { return op.kind === "play" || op.kind === "withdraw"; });
}
