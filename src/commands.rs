use std::{
    io::{self, Write},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::output::Output;
use anyhow::{Context, Result, bail, ensure};
use bark::{Wallet, actions::lightning::receive::LightningReceiveState};
use bitcoin::Amount;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::time::Instant;

use crate::{
    FundArgs, PlayArgs, WithdrawArgs,
    api::Api,
    protocol::{Bet, BetRequest, Draft, Terms, hash_bytes},
    store::Store,
    wallet::{self, Chain, Destination},
};

fn id() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn now() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs()
        .try_into()?)
}

fn flush() -> Result<()> {
    io::stdout().flush().context("could not flush output")
}

#[derive(Serialize, Deserialize)]
struct Deposit {
    invoice: String,
}

pub async fn fund(
    wallet: &Wallet,
    store: &Store,
    args: &FundArgs,
    timeout: u64,
    out: &Output,
) -> Result<()> {
    let before = wallet::sync(wallet).await?;
    out.event(json!({"event":"balance", "balance_sat":before}))?;
    out.message(format_args!("Spendable: {before} sats"))?;
    if args.balance {
        return Ok(());
    }
    if args.history {
        out.event(json!({"event":"history", "movements":wallet.history().await?}))?;
        out.message(format_args!(
            "{}",
            serde_json::to_string_pretty(&wallet.history().await?)?
        ))?;
        return Ok(());
    }
    let (operation, invoice) = if let Some(operation) = &args.resume {
        let (_, deposit) = store
            .get::<Deposit>(operation, "fund")?
            .context("funding request not found")?;
        (
            Some(operation.clone()),
            Some(
                deposit
                    .invoice
                    .parse::<lightning_invoice::Bolt11Invoice>()?,
            ),
        )
    } else if let Some(amount) = args.amount {
        let estimate = wallet
            .estimate_lightning_receive_fee(Amount::from_sat(amount))
            .await?;
        out.message(format_args!(
            "Lightning deposit: {amount} sats; estimated net {} sats (fee {} sats)",
            estimate.net_amount.to_sat(),
            estimate.fee.to_sat()
        ))?;
        out.event(json!({"event":"deposit_estimate", "amount_sat":amount, "net_sat":estimate.net_amount.to_sat(), "fee_sat":estimate.fee.to_sat()}))?;
        let invoice = wallet
            .bolt11_invoice(
                Amount::from_sat(amount),
                Some("bark-degen funding".into()),
                None,
            )
            .await?;
        let operation = id();
        store.insert(
            &operation,
            "fund",
            "awaiting_payment",
            &Deposit {
                invoice: invoice.to_string(),
            },
        )?;
        (Some(operation), Some(invoice))
    } else {
        (None, None)
    };
    if let Some(invoice) = &invoice {
        let operation = operation.as_deref().context("missing funding ID")?;
        out.event(json!({"event":"deposit", "id":operation, "invoice":invoice.to_string()}))?;
        out.message(format_args!("Funding ID: {operation}\n{invoice}"))?;
        out.message(format_args!("Resume: bark-degen fund --resume {operation}"))?;
    } else {
        let address = wallet.new_address().await?.to_string();
        out.event(json!({"event":"address", "address":address}))?;
        out.message(format_args!("Ark receiving address:\n{address}"))?;
    }
    flush()?;
    if args.no_wait || (invoice.is_none() && !args.wait) {
        return Ok(());
    }
    out.message(format_args!(
        "Waiting for funds; wallet maintenance remains active while this command runs…"
    ))?;
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        let balance = wallet::sync(wallet).await?;
        if let Some(invoice) = &invoice {
            let hash = invoice.payment_hash().to_string().parse()?;
            if matches!(
                wallet.lightning_receive_state(hash).await?,
                LightningReceiveState::Settled(_)
            ) {
                store.save(
                    operation.as_deref().context("missing funding ID")?,
                    "settled",
                    &Deposit {
                        invoice: invoice.to_string(),
                    },
                )?;
                out.event(json!({"event":"funded", "balance_sat":balance, "id":operation}))?;
                out.message(format_args!("Deposit settled. Spendable: {balance} sats"))?;
                return Ok(());
            }
        } else if balance > before {
            out.event(json!({"event":"funded", "balance_sat":balance}))?;
            out.message(format_args!("Funds received. Spendable: {balance} sats"))?;
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "funding wait timed out; invoice/wallet state is saved. Run fund --resume <ID> for Lightning, or fund --wait for Ark"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[derive(Serialize, Deserialize)]
struct Withdrawal {
    destination: String,
    amount_sat: u64,
    before_movement: Option<u32>,
    result: Option<String>,
}

async fn withdrawal_status(
    wallet: &Wallet,
    store: &Store,
    operation: &str,
    out: &Output,
) -> Result<()> {
    let (state, record) = store
        .get::<Withdrawal>(operation, "withdraw")?
        .context("withdrawal not found")?;
    let balance = wallet::sync(wallet).await?;
    out.event(json!({"event":"balance", "balance_sat":balance}))?;
    out.event(json!({"event":"withdrawal_status", "id":operation, "state":state, "amount_sat":record.amount_sat, "destination":record.destination, "result":record.result}))?;
    out.message(format_args!(
        "Withdrawal {operation}: {state}; {} sats to {}",
        record.amount_sat, record.destination
    ))?;
    if let Some(result) = record.result {
        out.message(format_args!("{result}"))?;
    }
    let history = wallet.history().await?;
    let matching: Vec<_> = history
        .iter()
        .filter(|m| {
            record.before_movement.is_none_or(|id| m.id.0 > id)
                && m.sent_to.iter().any(|d| {
                    d.destination.value_string() == record.destination
                        && d.amount.to_sat() == record.amount_sat
                })
        })
        .collect();
    out.message(format_args!(
        "Matching wallet movements:\n{}",
        serde_json::to_string_pretty(&matching)?
    ))?;
    if state != "completed" {
        out.message(format_args!(
            "No payment was resent. Pending SDK actions may continue during synchronization; inspect their status before making a new withdrawal."
        ))?;
    }
    Ok(())
}

pub async fn withdraw(
    wallet: &Wallet,
    store: &Store,
    args: &WithdrawArgs,
    chain: Chain,
    out: &Output,
) -> Result<()> {
    if let Some(operation) = &args.status {
        return withdrawal_status(wallet, store, operation, out).await;
    }
    let (destination, amount) = Destination::parse(
        args.destination
            .as_deref()
            .context("destination required")?,
        args.amount,
        chain.bitcoin(),
    )?;
    let operation = args.request_id.clone().unwrap_or_else(id);
    ensure!(
        !operation.is_empty()
            && operation.len() <= 128
            && operation
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "request ID must contain 1–128 letters, digits, underscores, or hyphens"
    );
    if let Some((_, existing)) = store.get::<Withdrawal>(&operation, "withdraw")? {
        ensure!(
            existing.destination == destination.text() && existing.amount_sat == amount,
            "request ID is already associated with different payment details"
        );
        return withdrawal_status(wallet, store, &operation, out).await;
    }
    let balance = wallet::sync(wallet).await?;
    ensure!(
        balance >= amount,
        "insufficient spendable balance: {balance} sats (fees may require additional funds)"
    );
    destination.validate(wallet).await?;
    let before_movement = wallet.history().await?.iter().map(|m| m.id.0).max();
    let mut record = Withdrawal {
        destination: destination.text(),
        amount_sat: amount,
        before_movement,
        result: None,
    };
    store.insert(&operation, "withdraw", "prepared", &record)?;
    out.event(json!({"event":"operation", "kind":"withdraw", "id":operation, "state":"prepared"}))?;
    out.message(format_args!(
        "Withdrawal ID: {operation}\nSending {amount} sats to {}. SDK fees may apply.\nCheck: bark-degen withdraw --status {operation}",
        record.destination
    ))?;
    flush()?;
    store.save(&operation, "sending", &record)?;
    let result =
        tokio::time::timeout(Duration::from_secs(120), destination.send(wallet, amount)).await;
    match result {
        Ok(Ok(message)) => {
            out.message(format_args!("{message}"))?;
            record.result = Some(message);
            store.save(&operation, "completed", &record)?;
            out.event(json!({"event":"withdrawal_status", "id":operation, "state":"completed", "result":record.result}))?;
        }
        other => {
            store.save(&operation, "unknown", &record)?;
            let reason = match other {
                Ok(Err(e)) => e.to_string(),
                _ => "wallet send timed out".into(),
            };
            bail!(
                "{reason}. Outcome may be pending; do not resend. Check withdrawal {operation} with --status"
            );
        }
    }
    Ok(())
}

#[derive(Default, Serialize, Deserialize)]
struct PlayRecord {
    api: String,
    draft: Option<Draft>,
    request: Option<BetRequest>,
    token: Option<String>,
    terms: Option<Terms>,
    latest: Option<Bet>,
}

pub fn status(store: &Store, dir: &std::path::Path, chain: Chain, out: &Output) -> Result<()> {
    let mut operations = Vec::new();
    for (id, kind, state) in store.list()? {
        let mut entry = json!({"id":id, "kind":kind, "state":state, "resumable":false});
        match kind.as_str() {
            "fund" => {
                if state == "settled" {
                    continue;
                }
                if let Some((_, deposit)) = store.get::<Deposit>(&id, "fund")? {
                    let invoice: lightning_invoice::Bolt11Invoice = deposit.invoice.parse()?;
                    if invoice.is_expired() {
                        continue;
                    }
                    entry["invoice"] = json!(deposit.invoice);
                    entry["resumable"] = json!(true);
                }
            }
            "play" => {
                if let Some((_, record)) = store.get::<PlayRecord>(&id, "play")? {
                    if let (Some(bet), Some(terms)) = (&record.latest, &record.terms)
                        && bet.validate_terms(terms).is_ok()
                        && bet.settled().unwrap_or(false)
                        && bet.status != "expired"
                    {
                        continue;
                    }
                    entry["resumable"] = json!(record.terms.is_some() && record.token.is_some());
                    // Before these states, the write-ahead journal proves no send was attempted.
                    if [
                        "requesting_commitment",
                        "committed",
                        "requesting_quote",
                        "fetching_quote",
                        "quoted",
                    ]
                    .contains(&state.as_str())
                    {
                        continue;
                    }
                }
            }
            "withdraw" => {
                if state == "completed" {
                    continue;
                }
                entry["resumable"] = json!(true);
            }
            _ => continue,
        }
        operations.push(entry);
    }
    let initialized = dir.join("ready").exists();
    out.event(json!({"event":"snapshot", "initialized":initialized, "network":chain.name(), "operations":operations}))?;
    out.message(format_args!(
        "Wallet initialized: {initialized}\n{}",
        serde_json::to_string_pretty(&operations)?
    ))
}

pub async fn play(
    wallet: &Wallet,
    store: &Store,
    args: &PlayArgs,
    base: &str,
    chain: Chain,
    timeout: u64,
    out: &Output,
) -> Result<()> {
    if let Some(operation) = &args.resume {
        let (_, record) = store
            .get::<PlayRecord>(operation, "play")?
            .context("bet not found")?;
        ensure!(
            record.terms.is_some(),
            "no validated quote was saved for this attempt; no stake was sent"
        );
        out.message(format_args!(
            "Resuming bet {operation}; no stake will be sent."
        ))?;
        return watch(wallet, store, operation, record, timeout, out).await;
    }
    let stake = args.stake.context("stake required")?;
    let api = Api::new(base)?;
    let cfg = api.config().await?;
    cfg.validate(chain.name(), &args.game, stake)?;
    let balance = wallet::sync(wallet).await?;
    ensure!(balance >= stake, "insufficient balance: {balance} sats");
    let payout_address = wallet.new_address().await?.to_string();
    let operation = id();
    let mut record = PlayRecord {
        api: base.into(),
        ..Default::default()
    };
    store.insert(&operation, "play", "requesting_commitment", &record)?;
    out.event(json!({"event":"operation", "kind":"play", "id":operation, "state":"requesting_commitment"}))?;
    out.message(format_args!(
        "Bet ID: {operation}\nResume: bark-degen play --resume {operation}"
    ))?;
    flush()?;
    let draft = api.commit().await?;
    hash_bytes(&draft.commitment)?;
    record.draft = Some(draft.clone());
    store.save(&operation, "committed", &record)?;
    let request = BetRequest {
        draft_token: draft.draft_token.clone(),
        game: args.game.clone(),
        stake_sat: stake,
        payout_address,
        client_seed: id(),
    };
    record.request = Some(request.clone());
    store.save(&operation, "requesting_quote", &record)?;
    let token = api.quote(&request).await?;
    record.token = Some(token.clone());
    store.save(&operation, "fetching_quote", &record)?;
    let quote = api.bet(&token).await?;
    quote.validate_quote(&draft, &request, &cfg, now()?)?;
    let address: ark::Address = quote
        .terms
        .pay_to
        .parse()
        .context("invalid house Ark address")?;
    wallet
        .validate_arkoor_address(&address)
        .await
        .context("house address is incompatible with this wallet's network/Ark server")?;
    record.terms = Some(quote.terms.clone());
    record.latest = Some(quote.clone());
    store.save(&operation, "quoted", &record)?;
    out.message(format_args!(
        "{}: stake {} sats, win if roll < {}, payout {} sats (stake included).",
        chain.name(),
        stake,
        quote.terms.target,
        quote.terms.payout_sat
    ))?;
    out.message(format_args!(
        "Commitment: {}\nPaying {}",
        draft.commitment, quote.terms.pay_to
    ))?;
    flush()?;
    ensure!(
        quote.terms.expires_at > now()?.saturating_add(20),
        "quote expired before payment"
    );
    let send_result = send_stake_once(store, &operation, &record, || async {
        tokio::time::timeout(
            Duration::from_secs(90),
            wallet.send_arkoor_payment(&address, Amount::from_sat(stake)),
        )
        .await
        .context("stake send timed out")?
    })
    .await;
    if let Err(e) = send_result {
        bail!(
            "{e:#}. Payment outcome is uncertain; use `play --resume {operation}`. Never resend this stake manually."
        );
    }
    watch(wallet, store, &operation, record, timeout, out).await
}

// Write-ahead boundary: only an untouched quote can send; a resumed or ambiguous
// operation must reconcile. The SDK separately persists its own payment checkpoint.
async fn send_stake_once<F, Fut>(
    store: &Store,
    operation: &str,
    record: &PlayRecord,
    send: F,
) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let (state, _) = store
        .get::<PlayRecord>(operation, "play")?
        .context("bet not found")?;
    ensure!(
        state == "quoted",
        "stake already attempted or quote unavailable; refusing another send"
    );
    store.save(operation, "sending", record)?;
    match send().await {
        Ok(()) => store.save(operation, "stake_sent", record),
        Err(e) => {
            store.save(operation, "payment_unknown", record)?;
            Err(e)
        }
    }
}

async fn watch(
    wallet: &Wallet,
    store: &Store,
    operation: &str,
    mut record: PlayRecord,
    timeout: u64,
    out: &Output,
) -> Result<()> {
    let api = Api::new(&record.api)?;
    let token = record
        .token
        .clone()
        .context("bet has no remote access token")?;
    let terms = record.terms.clone().context("bet has no accepted quote")?;
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let mut last_status = String::new();
    let mut failures = 0u32;
    loop {
        let bet = match api.bet(&token).await {
            Ok(bet) => {
                failures = 0;
                bet
            }
            Err(e) => {
                failures += 1;
                ensure!(
                    failures <= 5 && Instant::now() < deadline,
                    "{e}; state saved, resume bet {operation}"
                );
                eprintln!("Status temporarily unavailable; retrying a read…");
                tokio::time::sleep(Duration::from_secs(u64::from(failures) * 2)).await;
                continue;
            }
        };
        bet.validate_terms(&terms)?;
        if bet.status != last_status {
            out.message(format_args!("Bet status: {}", bet.status))?;
            out.event(json!({"event":"bet_status", "id":operation, "state":bet.status}))?;
            last_status = bet.status.clone();
        }
        record.latest = Some(bet.clone());
        store.save(operation, &bet.status, &record)?;
        let balance = wallet::sync(wallet).await?;
        if bet.settled()? {
            if bet.status == "expired" {
                bail!(
                    "quote expired. A late stake/refund may still be in flight; resume bet {operation} to reconcile it"
                );
            }
            let r = bet.receipt.context("resolved bet missing receipt")?;
            out.event(json!({"event":"bet_result", "id":operation, "roll":r.roll, "win":r.win, "payout_sat":terms.payout_sat, "balance_sat":balance}))?;
            out.message(format_args!(
                "Verified roll: {:04}. {}. Quoted winning payout: {} sats.",
                r.roll,
                if r.win { "WIN" } else { "LOSE" },
                terms.payout_sat
            ))?;
            if r.win {
                out.message(format_args!(
                    "House reports payout sent; synchronized wallet balance: {balance} sats."
                ))?;
            } else {
                out.message(format_args!("Spendable: {balance} sats"))?;
            }
            out.message(format_args!(
                "Original quote and verified receipt saved in bets.sqlite (bet {operation})."
            ))?;
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "waiting timed out; resume with `play --resume {operation}` (does not send again)"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn uncertain_payment_is_never_automatically_sent_again_after_restart() {
        let temp = tempfile::tempdir().unwrap();
        let record = PlayRecord::default();
        let count = AtomicUsize::new(0);
        {
            let store = Store::open(temp.path()).unwrap();
            store.insert("bet", "play", "quoted", &record).unwrap();
            assert!(
                send_stake_once(&store, "bet", &record, || async {
                    count.fetch_add(1, Ordering::SeqCst);
                    bail!("timeout after possible acceptance")
                })
                .await
                .is_err()
            );
        }
        let store = Store::open(temp.path()).unwrap();
        assert!(
            send_stake_once(&store, "bet", &record, || async {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await
            .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(
            store.get::<PlayRecord>("bet", "play").unwrap().unwrap().0,
            "payment_unknown"
        );
    }

    #[tokio::test]
    async fn completed_send_and_interrupted_sending_state_cannot_be_replayed() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path()).unwrap();
        let record = PlayRecord::default();
        for state in ["sending", "stake_sent", "paid", "awaiting_payment"] {
            store.insert(state, "play", state, &record).unwrap();
            assert!(
                send_stake_once(&store, state, &record, || async { panic!("must not send") })
                    .await
                    .is_err()
            );
        }
        store.insert("fresh", "play", "quoted", &record).unwrap();
        send_stake_once(&store, "fresh", &record, || async { Ok(()) })
            .await
            .unwrap();
        assert_eq!(
            store.get::<PlayRecord>("fresh", "play").unwrap().unwrap().0,
            "stake_sent"
        );
    }
}
