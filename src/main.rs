mod api;
mod commands;
mod output;
mod protocol;
mod store;
mod wallet;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(
    version,
    about = "A persistent Bark wallet and Barkdice player. Amounts are whole sats.",
    after_help = "Mainnet is the default. Withdraw and play send funds when invoked. Wallet state is stored locally; no Barkdice account is needed."
)]
pub struct Cli {
    /// Emit newline-delimited JSON events for desktop clients
    #[arg(long, global = true)]
    json: bool,
    /// Wallet network; each network has a separate data directory
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "mainnet",
        env = "BARK_DEGEN_NETWORK"
    )]
    network: wallet::Chain,
    /// Parent data directory (defaults to $XDG_DATA_HOME/bark-degen)
    #[arg(long, global = true, env = "BARK_DEGEN_DATA_DIR")]
    data_dir: Option<PathBuf>,
    /// Ark server (saved at first fund; later calls must use the same server)
    #[arg(long, global = true, env = "BARK_DEGEN_ARK_SERVER")]
    ark_server: Option<String>,
    /// Esplora chain source (saved at first fund)
    #[arg(long, global = true, env = "BARK_DEGEN_ESPLORA")]
    esplora: Option<String>,
    /// Barkdice HTTP endpoint
    #[arg(
        long,
        global = true,
        default_value = "https://barkdice.com",
        env = "BARK_DEGEN_API"
    )]
    api: String,
    /// Maximum wait for incoming funds or a bet result, in seconds
    #[arg(long, global = true, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..=86400))]
    timeout: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Inspect wallet existence and saved operations without connecting or sending
    Status,
    /// Create/reopen the wallet; show an Ark address, or request a Lightning deposit
    Fund(FundArgs),
    /// Send wallet funds to an Ark address, BOLT11 invoice, or Bitcoin address
    Withdraw(WithdrawArgs),
    /// Quote, pay, and independently verify one Barkdice bet
    Play(PlayArgs),
}

#[derive(Args)]
pub struct FundArgs {
    /// Generate a Lightning invoice for this many sats; waits for payment by default
    #[arg(value_parser = positive_sats, conflicts_with_all = ["resume", "balance", "history"])]
    amount: Option<u64>,
    /// Wait for an Ark deposit or keep the wallet online to sync/maintain it
    #[arg(long, conflicts_with_all = ["no_wait", "balance", "history"])]
    wait: bool,
    /// Print the invoice and return; use --resume to claim/check it later
    #[arg(long, requires = "amount", conflicts_with = "resume")]
    no_wait: bool,
    /// Resume waiting for a previously requested Lightning deposit
    #[arg(long, conflicts_with_all = ["balance", "history"])]
    resume: Option<String>,
    /// Synchronize and show the existing wallet's balance
    #[arg(long, conflicts_with = "history")]
    balance: bool,
    /// Synchronize and print wallet movement history as JSON
    #[arg(long)]
    history: bool,
}

#[derive(Args)]
pub struct WithdrawArgs {
    /// Recipient address or BOLT11 invoice
    #[arg(required_unless_present_any = ["status", "list"], conflicts_with_all = ["status", "list"])]
    destination: Option<String>,
    /// Sats to send; omit for an invoice containing its own amount
    #[arg(value_parser = positive_sats, requires = "destination")]
    amount: Option<u64>,
    /// Stable local request ID; repeating it checks the existing payment and never resends
    #[arg(long, requires = "destination")]
    request_id: Option<String>,
    /// Inspect a withdrawal and matching wallet movements, without sending again
    #[arg(long, conflicts_with = "list")]
    status: Option<String>,
    /// List locally recorded withdrawals without opening the wallet
    #[arg(long)]
    list: bool,
}

#[derive(Args)]
pub struct PlayArgs {
    /// Stake in sats; places exactly one bet
    #[arg(value_parser = positive_sats, required_unless_present_any = ["resume", "list"], conflicts_with_all = ["resume", "list"])]
    stake: Option<u64>,
    /// Game: roll strictly below 5000, 2500, 1000, or 200
    #[arg(long, default_value = "lt5000", value_parser = ["lt5000", "lt2500", "lt1000", "lt0200"])]
    game: String,
    /// Resume monitoring a saved bet; never sends another stake
    #[arg(long, conflicts_with = "list")]
    resume: Option<String>,
    /// List local bet IDs and states without opening the wallet
    #[arg(long)]
    list: bool,
}

fn positive_sats(value: &str) -> std::result::Result<u64, String> {
    let n = value
        .parse::<u64>()
        .map_err(|_| "use a whole number of sats".to_owned())?;
    wallet::required_amount(Some(n)).map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let cli = Cli::parse();
    let out = output::Output { json: cli.json };
    if let Err(e) = run(cli, &out).await {
        let _ = out.event(serde_json::json!({"event": "error", "message": format!("{e:#}")}));
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli, out: &output::Output) -> Result<()> {
    let dir = wallet::data_dir(cli.data_dir.as_deref(), cli.network)?;
    store::private_dir(&dir)?;
    let _lock = store::lock(&dir)?;
    let store = store::Store::open(&dir)?;
    if matches!(cli.command, Command::Status) {
        return commands::status(&store, &dir, cli.network, out);
    }
    let list_kind = match &cli.command {
        Command::Play(a) if a.list => Some("play"),
        Command::Withdraw(a) if a.list => Some("withdraw"),
        _ => None,
    };
    if let Some(kind) = list_kind {
        for (id, operation, state) in store.list()? {
            if operation == kind {
                out.message(format_args!("{id}\t{state}"))?;
                out.event(serde_json::json!({"event":"operation", "id":id, "kind":operation, "state":state}))?;
            }
        }
        return Ok(());
    }
    let create =
        matches!(&cli.command, Command::Fund(a) if a.resume.is_none() && !a.balance && !a.history);
    eprintln!("Wallet: {} ({})", dir.display(), cli.network.name());
    let wallet = tokio::time::timeout(
        Duration::from_secs(90),
        wallet::open(
            &dir,
            cli.network,
            cli.ark_server.as_deref(),
            cli.esplora.as_deref(),
            create,
        ),
    )
    .await
    .context("wallet connection timed out; any existing state has been preserved")??;
    let result = tokio::select! {
        result = async {
            match &cli.command {
                Command::Fund(args) => commands::fund(&wallet, &store, args, cli.timeout, out).await,
                Command::Withdraw(args) => commands::withdraw(&wallet, &store, args, cli.network, out).await,
                Command::Play(args) => commands::play(&wallet, &store, args, &cli.api, cli.network, cli.timeout, out).await,
                Command::Status => unreachable!(),
            }
        } => result,
        _ = tokio::signal::ctrl_c() => Err(anyhow::anyhow!("interrupted; payment state is saved. Use the printed operation ID to resume/check before sending again")),
    };
    let shutdown = wallet::close(&wallet).await;
    result?;
    shutdown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_contract() {
        for args in [
            vec!["bark-degen", "fund"],
            vec!["bark-degen", "fund", "10000"],
            vec!["bark-degen", "withdraw", "lnbc..."],
            vec!["bark-degen", "play", "1000"],
            vec!["bark-degen", "play", "--resume", "id"],
            vec!["bark-degen", "withdraw", "--status", "id"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok());
        }
        for args in [
            vec!["bark-degen", "play"],
            vec!["bark-degen", "play", "0"],
            vec!["bark-degen", "fund", "-1"],
            vec!["bark-degen", "play", "1.5"],
            vec!["bark-degen", "withdraw"],
            vec!["bark-degen", "play", "1000", "--resume", "id"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }
}
