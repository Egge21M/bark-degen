# bark-degen

> [!WARNING]
> This is experimental software that has not been reviewed or independently audited. It may contain bugs that result in loss of funds. Proceed at your own risk, and only use funds you can afford to lose. The software is provided without warranty.

A Rust CLI with its own persistent Bark wallet and three commands: `fund`, `withdraw`, and `play`. It embeds the official `bark-wallet` SDK and uses SQLite. No separately installed Bark wallet or daemon is required.

Licensed under the [MIT License](LICENSE). Copyright (c) 2026 Egge21M.

## Build

Install a current stable Rust toolchain, a C compiler, and Protocol Buffers' `protoc` compiler. On Debian/Ubuntu the system packages are `build-essential protobuf-compiler`. SQLite is bundled with the build.

```sh
cargo build --locked --release
./target/release/bark-degen --help
# Optional: install into Cargo's bin directory
cargo install --locked --path .
```

The implementation pins Bark 0.7.1. The checked-in lockfile pins transitive dependencies. A recent stable Rust is needed for the SDK and its dependencies.

## Omarchy bar plugin

The repository is also an **Omarchy Quattro / Quickshell** plugin (`bark.degen`). It uses Omarchy's built-in bar and shared popup components; older Waybar-based Omarchy releases are not supported.

On your Omarchy desktop, build and install it with:

```sh
python3 scripts/install-omarchy.py
```

This builds the Rust client, copies the plugin and its binary into `~/.config/omarchy/plugins/bark.degen`, and enables the widget in the right bar section. To use an existing build, pass `--binary /path/to/bark-degen`. To stage files without enabling them, pass `--dest /path/to/bark.degen --no-enable`. Re-run the installer to update. Restart the shell after updating QML service code (`omarchy restart shell`); finish active payments first.

- **Left-click the dice:** place one bet with the saved stake and game. The icon animates while the bet runs. A verified result sends a desktop notification, even with the popup closed. The horizontal bar keeps a **WIN / LOSS** label until the next bet; the icon color and tooltip also reflect the result.
- **Wins:** a brief, click-through confetti shower appears on the monitor where you started the bet. It does not take keyboard focus and is skipped when Omarchy has reduced motion enabled.
- **Right-click:** open the Wallet / Settings popup. A new wallet has a **Create wallet** button. Existing CLI wallets are reused.
- **Wallet → Top up:** generate a Lightning invoice and copy it, or copy an Ark receiving address. Keep the plugin running to claim a Lightning deposit. If waiting is stopped or the shell restarts, use **Resume top up** for the saved invoice.
- **Wallet → Withdraw:** enter an Ark address, BOLT11 invoice, or Bitcoin address, plus an amount when required, and press **Send**. This sends immediately; fees may be additional.
- **Settings:** choose the 50%, 25%, 10%, or 2% game and the stake in whole sats. Notifications and confetti are enabled by default and can be toggled independently. **Save settings** persists them in the widget's `shell.json` entry (`notifications` and `confetti` are boolean fields).

Notifications use `notify-send` from `libnotify` and respect the desktop's notification settings. Results are announced once per bet for the active wallet within the running session, including a bet resolved with **Resume bet**. Pending or failed verification never triggers win/loss feedback.

Mainnet and a 1,000-sat stake are the defaults. Settings displays the active network. The shared service serializes actions across monitors and refreshes the wallet balance once a minute while enabled. Closing the popup leaves its active operation running. Incomplete bets and withdrawals appear with **Resume bet** / **Check withdrawal**; those actions inspect existing payments and never send again. An unresolved outgoing payment disables new spends until it is resolved; inspect an uncertain withdrawal with the CLI's `withdraw --status ID` and `fund --history` if its outcome remains unknown.

Connection overrides are optional inline fields on the `bark.degen` entry in `~/.config/omarchy/shell.json`. For example, use a separate signet wallet and a compatible signet dice service:

```json
{
  "id": "bark.degen",
  "game": "lt5000",
  "stake": 1000,
  "network": "signet",
  "api": "https://YOUR-SIGNET-BARKDICE-SERVER",
  "dataDir": "/absolute/path/to/wallet-data"
}
```

`binary`, `arkServer`, and `esplora` are also supported. `binary` defaults to the bundled `bin/bark-degen`. Use an absolute path for an override. Stop active operations before changing connection fields. The wallet data and backup requirements below apply equally to the plugin. There is no separate wallet daemon to install.

For installation through `omarchy plugin add`, the repository root contains the manifest. After cloning, build the bundled binary inside that checkout before enabling:

```sh
cd ~/.config/omarchy/plugins/bark.degen
cargo build --locked --release
mkdir -p bin
cp target/release/bark-degen bin/bark-degen
omarchy plugin enable bark.degen
```

## Desktop protocol

`bark-degen --json COMMAND` emits flushed, newline-delimited JSON to stdout; diagnostics remain on stderr. Events include `snapshot`, `balance`, `address`, `deposit_estimate`, `deposit`, `funded`, `operation`, `bet_status`, `bet_result`, `withdrawal_status`, and `error`. Amounts use integer `*_sat` fields. The command exit code indicates success or failure. Command-line syntax errors still come from Clap on stderr.

`bark-degen --json status` inspects wallet existence and recoverable operations without connecting to a wallet server. It never emits a seed or bet access token. `play --resume` and `withdraw --status` retain their existing no-resend behavior. Human-readable output remains the default.

## Fund

```sh
# First run creates the wallet; prints an Ark receiving address and balance
bark-degen fund

# Generate a Lightning invoice for 20,000 sats and wait for it to settle
bark-degen fund 20000

# Wait for an incoming Ark deposit
bark-degen fund --wait

# Print a Lightning invoice without waiting; keep the ID for later
bark-degen fund 20000 --no-wait
bark-degen fund --resume FUNDING_ID

# Synchronize and inspect your funds
bark-degen fund --balance
bark-degen fund --history
```

Pay the printed invoice/address from another wallet. Lightning deposits may incur Ark server fees; the command shows the estimated net deposit. Leave the command running to claim a Lightning payment, or resume it promptly. `--timeout SECONDS` controls the polling period (default 600 seconds). An Ark deposit can be discovered on the next synchronization even if the CLI was closed when it arrived.

## Withdraw

```sh
bark-degen withdraw ark1... 5000
bark-degen withdraw lnbc...           # Uses the invoice's embedded amount
bark-degen withdraw lnbc... 5000      # Required for an amountless invoice
bark-degen withdraw bc1q... 5000     # Withdraw from Ark to on-chain Bitcoin

# Use a stable request ID when calling from a script
bark-degen withdraw ark1... 5000 --request-id cashout-1
bark-degen withdraw --status cashout-1
bark-degen withdraw --list
```

Invoking `withdraw` sends the requested amount; fees may be additional. Destinations must match the wallet network, and Ark addresses must belong to a compatible Ark server. BOLT12, Lightning addresses, and sweeping the whole balance are not supported in this version. A Bitcoin withdrawal reports a broadcast transaction ID, not chain confirmation.

A timeout or error can happen after a send was accepted. The CLI saves a withdrawal ID **before** calling the SDK. Repeating the same `--request-id` with the same details checks the existing request without submitting another payment. Use `--status` to inspect matching wallet movements; if an operation remains uncertain, investigate those movements before making a new request. SDK synchronization may finish its previously persisted send. An `unknown` journal entry deliberately does not claim the payment failed or that funds are safe to resend.

## Play

```sh
bark-degen play 1000                  # One bet, default 50% game
bark-degen play 1000 --game lt2500     # 25% game
bark-degen play 1000 --game lt1000     # 10% game
bark-degen play 1000 --game lt0200     # 2% game

bark-degen play --list
bark-degen play --resume BET_ID
```

Each invocation places **one** bet. It reads current limits, saves the house commitment before generating the client seed, validates the quote, sends the stake, monitors settlement, and verifies the receipt using an independent implementation of the published `bark-dice/v1` protocol. The original terms and receipt are saved in the application database. House-reported payout status and synchronized wallet balance are displayed separately.

`play --resume` only monitors and synchronizes: it never submits a second stake. If interrupted before payment, the unpaid quote is allowed to expire. If quote creation timed out before its token was returned, the attempt remains recorded but cannot be queried; no stake is sent. A new `play AMOUNT` invocation always means a new bet.

### Signet play loop

[`scripts/play_until.py`](scripts/play_until.py) repeatedly invokes the one-bet command against a signet deployment. It defaults to a 1,000-sat `lt5000` game and stops when the spendable balance reaches 50,000 sats or falls below the stake:

```sh
cargo build --locked --release
python3 scripts/play_until.py --api https://YOUR-SIGNET-BARKDICE-SERVER
```

The runner always passes `--network signet`; the server's `/api/config` must also report signet before the CLI will send a stake. State and per-command logs are stored under `.bark-degen-runs/signet-play-until/`. Re-run the same command after an interruption: an in-flight bet is monitored with `play --resume`, which cannot send its stake again. To start an unrelated run or change its settings, provide a different `--run-dir`. Options such as `--target`, `--stake`, `--game`, `--data-dir`, `--ark-server`, and `--esplora` are available through `--help`.

## Network and storage

**Mainnet is the default**, matching the observed `barkdice.com` deployment. `play` and `withdraw` execute payments without another confirmation prompt. Amounts are integer sats.

```sh
bark-degen --network signet fund
bark-degen --data-dir /path/to/my-wallet fund
# Configure a different compatible Ark server when creating a wallet:
bark-degen --ark-server https://my-ark.example --esplora https://my-explorer.example/api fund
```

Default endpoints come from [Second's connection details](https://second.tech/docs/connection-details). The CLI checks the house payment address against the selected Ark server before sending. Live compatibility with Barkdice still requires a funded integration test; no funds were moved during implementation.

Wallets are separated by network under `$XDG_DATA_HOME/bark-degen`, or `$HOME/.local/share/bark-degen` when XDG is unset:

```text
mainnet/
  identity.json       # Mnemonic, network, and saved server configuration
  wallet/db.sqlite    # Bark-managed wallet state
  bets.sqlite         # Deposits, withdrawals, commitments, quotes, receipts
  ready               # Prevents silently recreating a missing wallet database
  cli.lock            # Prevents concurrent CLI commands against the same wallet
```

The mnemonic in `identity.json` is **plaintext**, protected by a private directory and mode `0600` on Unix. It is never printed by the CLI. Preserve it and the entire wallet data directory; never commit wallet files. The SQLite databases contain financial history and bet access tokens. For a consistent manual backup, stop the CLI and copy the complete network directory to encrypted storage. For continuous use, arrange consistent backups after state changes. See [Bark's backup requirements](https://second.tech/docs/backups). Restoring the full directory reopens the existing wallet; this version does not provide mnemonic-only import or recovery commands.

Use the same `--network` and `--data-dir` on resume/status calls. Corresponding environment variables are `BARK_DEGEN_NETWORK`, `BARK_DEGEN_DATA_DIR`, `BARK_DEGEN_ARK_SERVER`, `BARK_DEGEN_ESPLORA`, and `BARK_DEGEN_API`. The latter overrides the dice service (HTTPS required except loopback HTTP for local tests). Bet resumes use the saved service URL.

Bark's maintenance worker runs only while a wallet command is active. A stored balance still needs periodic maintenance and VTXO refresh; do not leave a funded wallet indefinitely offline. `fund --wait --timeout 86400` keeps the wallet active until funds arrive or the timeout expires. The CLI does not install a background service or implement unilateral-exit management.

## Validation

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
node tests/test_omarchy_model.cjs
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software qmltestrunner -input tests/qml
```

The QML tests require Qt 6 Quick Controls and QtTest (on some distributions, `qmltestrunner` lives under `/usr/lib/qt6/bin`). They exercise the actual wallet/settings controls using a fake backend. Rust integration tests cover the JSON protocol and restart snapshots without creating a funded wallet.

With Quickshell, labwc, and the Omarchy shell sources installed, run the service and real popup integration tests in an isolated headless Wayland session:

```sh
python3 scripts/test-omarchy-runtime.py --shell /usr/share/omarchy/shell
```

This checks streaming invoices, one bet per click across widget instances, settings persistence through the host API, withdrawals, restart recovery, and a missing binary. It also checks closed-panel win/loss feedback, notification deduplication, reduced motion, and confetti window cleanup. Wallet and notification commands are deterministic fixtures; no funds are moved or desktop notifications sent. `--quickshell` and `--compositor` can select alternate executable paths.

Tests use local temporary databases, loopback mock HTTP/Esplora servers, and the five [published reference vectors](https://barkdice.com/test-vectors.json). They cover verification/tampering, quote validation, network and amount checks, reopening a real SDK SQLite wallet with the same keys, wallet locking, and refusing duplicate stake submissions after an uncertain send. They do not contact a live wallet server or spend funds. Verifying a roll does not guarantee payout.
