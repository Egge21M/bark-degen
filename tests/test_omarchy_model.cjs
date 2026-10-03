const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const model = vm.createContext({});
vm.runInContext(fs.readFileSync('omarchy/Model.js', 'utf8'), model);
const plain = value => JSON.parse(JSON.stringify(value));
const config = model.config({}, '/a path/bark-degen');
assert.equal(config.notifications, true);
assert.equal(config.confetti, true);
assert.equal(model.config({notifications: false, confetti: false}, 'bark').notifications, false);
assert.equal(model.config({notifications: false, confetti: false}, 'bark').confetti, false);
assert.equal(model.resultMessage({win: true, roll: 42, payout_sat: 1970}), 'WIN · roll 0042 · house reports 1970 sats paid · verified');
assert.equal(model.resultMessage({win: false, roll: 9999, payout_sat: 1970}), 'LOSS · roll 9999 · verified');
assert.deepEqual(plain(model.command(config, ['play', '1000', '--game', 'lt5000'])),
    ['/a path/bark-degen', '--json', '--network', 'mainnet', '--api', 'https://barkdice.com', 'play', '1000', '--game', 'lt5000']);
for (const input of ['', '0', '-1', '1.2', '1e3', 'Infinity', '1;echo x', '2100000000000001']) {
    assert.throws(() => model.sats(input));
}
assert.equal(model.sats('2100000000000000'), 2100000000000000);
assert.throws(() => model.config({game: 'unknown'}, 'bark'));
assert.throws(() => model.config({network: 'testnet'}, 'bark'));
assert.throws(() => model.withdrawal('--help', '10'));
assert.deepEqual(plain(model.withdrawal('lnbc1invoice', '')), ['withdraw', '--', 'lnbc1invoice']);
assert.deepEqual(plain(model.withdrawal('ark1address', '1000')), ['withdraw', '--', 'ark1address', '1000']);
for (const kind of ['play', 'withdraw', 'fund']) {
    const args = plain(model.resume({kind, id: 'saved-id', resumable: true}));
    assert.deepEqual(args, [kind, kind === 'withdraw' ? '--status' : '--resume', 'saved-id']);
    assert.equal(args.includes('1000'), false);
}
assert.equal(model.hasUncertainPayment([{kind:'fund'}]), false);
assert.equal(model.hasUncertainPayment([{kind:'play'}]), true);
assert.equal(model.hasUncertainPayment([{kind:'withdraw'}]), true);
console.log('Omarchy model contracts passed.');
