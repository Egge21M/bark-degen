use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use bark::{Config, OpenWalletArgs, Wallet, WalletSeed};
use bip39::Mnemonic;
use bitcoin::{Amount, Network};
use clap::ValueEnum;
use lightning_invoice::Bolt11Invoice;
use serde::{Deserialize, Serialize};

use crate::{api::endpoint, store};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    #[default]
    Mainnet,
    Signet,
}

impl Chain {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mainnet => "mainnet",
            Self::Signet => "signet",
        }
    }
    pub fn bitcoin(self) -> Network {
        match self {
            Self::Mainnet => Network::Bitcoin,
            Self::Signet => Network::Signet,
        }
    }
    fn endpoints(self) -> (&'static str, &'static str) {
        match self {
            Self::Mainnet => ("https://ark.second.tech", "https://mempool.second.tech/api"),
            Self::Signet => (
                "https://ark.signet.2nd.dev",
                "https://esplora.signet.2nd.dev",
            ),
        }
    }
}

// Intentionally no Debug: this structure holds the spending secret.
#[derive(Serialize, Deserialize)]
struct Identity {
    version: u8,
    network: Chain,
    ark_server: String,
    esplora: String,
    mnemonic: String,
}

fn identity(
    dir: &Path,
    chain: Chain,
    ark: Option<&str>,
    esplora: Option<&str>,
    allow_create: bool,
) -> Result<Identity> {
    let path = dir.join("identity.json");
    store::private_file(&path)?;
    let identity = if path.exists() {
        serde_json::from_slice::<Identity>(&fs::read(&path)?)
            .context("invalid wallet identity; restore its backup, do not delete it")?
    } else {
        ensure!(
            allow_create,
            "wallet does not exist; run `bark-degen fund` first"
        );
        ensure!(
            !dir.join("wallet").exists() && !dir.join("ready").exists(),
            "wallet identity missing but wallet data exists; restore identity.json from backup"
        );
        let (default_ark, default_esplora) = chain.endpoints();
        let ark_server = endpoint(ark.unwrap_or(default_ark))?.to_string();
        let esplora = endpoint(esplora.unwrap_or(default_esplora))?.to_string();
        let identity = Identity {
            version: 1,
            network: chain,
            ark_server,
            esplora,
            mnemonic: Mnemonic::generate(12)?.to_string(),
        };
        store::write_new(&path, &serde_json::to_vec_pretty(&identity)?)?;
        eprintln!(
            "Created {} wallet identity at {}. Back up this private file and the wallet directory.",
            chain.name(),
            path.display()
        );
        identity
    };
    ensure!(
        identity.version == 1 && identity.network == chain,
        "wallet identity version/network mismatch"
    );
    if let Some(ark) = ark {
        ensure!(
            endpoint(ark)?.as_str() == identity.ark_server,
            "Ark server differs from saved wallet configuration"
        );
    }
    if let Some(esplora) = esplora {
        ensure!(
            endpoint(esplora)?.as_str() == identity.esplora,
            "Esplora differs from saved wallet configuration"
        );
    }
    endpoint(&identity.ark_server)?;
    endpoint(&identity.esplora)?;
    Ok(identity)
}

pub fn data_dir(root: Option<&Path>, chain: Chain) -> Result<PathBuf> {
    let root = match root {
        Some(root) => root.to_owned(),
        None => {
            let base = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
                .context("set --data-dir when HOME and XDG_DATA_HOME are unavailable")?;
            base.join("bark-degen")
        }
    };
    Ok(root.join(chain.name()))
}

pub async fn open(
    dir: &Path,
    chain: Chain,
    ark: Option<&str>,
    esplora: Option<&str>,
    allow_create: bool,
) -> Result<Wallet> {
    let id = identity(dir, chain, ark, esplora, allow_create)?;
    let ready = dir.join("ready");
    let wallet_dir = dir.join("wallet");
    // Never turn a lost database into a newly initialized, apparently empty wallet.
    if ready.exists() {
        ensure!(
            wallet_dir.join("db.sqlite").is_file(),
            "wallet database missing; restore the wallet backup"
        );
    }
    store::private_dir(&wallet_dir)?;
    store::private_file(&wallet_dir.join("db.sqlite"))?;
    let recovery_error = Arc::new(Mutex::new(None::<String>));
    let recovery_result = recovery_error.clone();
    let config = Config {
        server_address: id.ark_server,
        esplora_address: Some(id.esplora),
        user_agent: Some(concat!("bark-degen/", env!("CARGO_PKG_VERSION")).into()),
        ..Config::network_default(chain.bitcoin())
    };
    let wallet = Wallet::open(
        chain.bitcoin(),
        WalletSeed::new_from_mnemonic(chain.bitcoin(), &Mnemonic::from_str(&id.mnemonic)?),
        config,
        OpenWalletArgs {
            datadir: Some(wallet_dir.clone()),
            create_if_not_exists: allow_create && !ready.exists(),
            run_daemon: true,
            on_recovery_finished: Some(Box::new(move |status| {
                use bark::RecoveryStatus;
                let error = match status {
                    RecoveryStatus::Failed(_) => Some("wallet recovery failed".into()),
                    RecoveryStatus::Completed(report) if !report.is_complete() => {
                        Some("wallet recovery incomplete".into())
                    }
                    _ => None,
                };
                *recovery_result.lock().expect("recovery lock poisoned") = error;
            })),
            ..Default::default()
        },
    )
    .await
    .context("could not open Bark wallet")?;
    store::private_file(&wallet_dir.join("db.sqlite"))?;
    if let Some(error) = recovery_error
        .lock()
        .expect("recovery lock poisoned")
        .take()
    {
        // New identities have no existing funds, but never hide an SDK recovery error.
        wallet.stop_daemon();
        bail!(
            "{error}; wallet preserved at {}. Resolve recovery before spending.",
            dir.display()
        );
    }
    if !ready.exists() {
        store::write_new(&ready, b"1\n")?;
    }
    Ok(wallet)
}

pub async fn sync(wallet: &Wallet) -> Result<u64> {
    wallet
        .require_ark_info()
        .await
        .context("Ark server unavailable")?;
    // sync() logs failures internally; explicitly require mailbox sync to succeed.
    wallet.sync().await;
    wallet
        .sync_mailbox()
        .await
        .context("wallet mailbox sync failed")?;
    Ok(wallet.balance().await?.spendable.to_sat())
}

pub async fn close(wallet: &Wallet) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(20), wallet.stop_daemon_wait())
        .await
        .context("wallet shutdown timed out; pending actions remain in the wallet database")??;
    Ok(())
}

pub enum Destination {
    Ark(ark::Address),
    Lightning(Bolt11Invoice),
    Bitcoin(bitcoin::Address),
}

impl Destination {
    pub fn parse(value: &str, amount: Option<u64>, network: Network) -> Result<(Self, u64)> {
        let value = value.strip_prefix("lightning:").unwrap_or(value);
        if let Ok(address) = value.parse::<ark::Address>() {
            return Ok((Self::Ark(address), required_amount(amount)?));
        }
        if let Ok(invoice) = value.parse::<Bolt11Invoice>() {
            ensure!(
                invoice.network() == network,
                "Lightning invoice network mismatch"
            );
            let sats = match invoice.amount_milli_satoshis() {
                Some(msat) => {
                    ensure!(
                        msat % 1000 == 0,
                        "invoice uses fractional sats; unsupported by this CLI"
                    );
                    let sats = msat / 1000;
                    ensure!(
                        amount.is_none_or(|a| a == sats),
                        "amount does not match invoice"
                    );
                    required_amount(Some(sats))?
                }
                None => required_amount(amount)?,
            };
            return Ok((Self::Lightning(invoice), sats));
        }
        if let Ok(address) = value.parse::<bitcoin::Address<bitcoin::address::NetworkUnchecked>>() {
            return Ok((
                Self::Bitcoin(address.require_network(network)?),
                required_amount(amount)?,
            ));
        }
        bail!("destination must be an Ark address, BOLT11 invoice, or Bitcoin address")
    }

    pub fn text(&self) -> String {
        match self {
            Self::Ark(a) => a.to_string(),
            Self::Lightning(i) => i.to_string(),
            Self::Bitcoin(a) => a.to_string(),
        }
    }

    pub async fn validate(&self, wallet: &Wallet) -> Result<()> {
        if let Self::Ark(address) = self {
            wallet.validate_arkoor_address(address).await?;
        }
        if let Self::Lightning(invoice) = self {
            ensure!(!invoice.is_expired(), "Lightning invoice expired");
        }
        Ok(())
    }

    pub async fn send(&self, wallet: &Wallet, sats: u64) -> Result<String> {
        let amount = Amount::from_sat(sats);
        match self {
            Self::Ark(address) => {
                wallet.send_arkoor_payment(address, amount).await?;
                Ok("Ark payment completed".into())
            }
            Self::Lightning(invoice) => {
                wallet
                    .pay_lightning_invoice(
                        invoice.clone(),
                        if invoice.amount_milli_satoshis().is_none() {
                            Some(amount)
                        } else {
                            None
                        },
                        true,
                    )
                    .await?;
                let state = wallet
                    .lightning_send_state(invoice.payment_hash().to_string().parse()?)
                    .await?;
                ensure!(
                    matches!(
                        state,
                        bark::actions::lightning::pay::LightningSendState::Paid(_)
                    ),
                    "Lightning payment is not yet confirmed paid"
                );
                Ok("Lightning payment completed".into())
            }
            Self::Bitcoin(address) => {
                let txid = wallet.send_onchain(address.clone(), amount).await?;
                Ok(format!("Bitcoin withdrawal broadcast: {txid}"))
            }
        }
    }
}

pub fn required_amount(amount: Option<u64>) -> Result<u64> {
    let amount = amount.context("provide an amount in whole sats")?;
    ensure!(
        amount > 0 && amount <= 2_100_000_000_000_000,
        "amount must be positive whole sats within Bitcoin's supply"
    );
    Ok(amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn real_sdk_sqlite_wallet_reopens_with_the_same_keys() {
        use std::io::{Read, Write};
        let dir = tempfile::tempdir().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut buffer = [0; 4096];
                let n = socket.read(&mut buffer).unwrap();
                assert!(String::from_utf8_lossy(&buffer[..n]).starts_with("GET /block-height/0 "));
                let body = bitcoin::blockdata::constants::genesis_block(Network::Signet)
                    .block_hash()
                    .to_string();
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let phrase = Mnemonic::generate(12).unwrap();
        let seed = WalletSeed::new_from_mnemonic(Network::Signet, &phrase);
        // Initialize real SDK persistence with test-only properties; no Ark server or
        // funded wallet is involved. Open still checks the mocked chain's network.
        {
            use bark::persist::BarkPersister;
            let db =
                bark::persist::sqlite::SqliteClient::open(dir.path().join("db.sqlite")).unwrap();
            db.init_wallet(&bark::WalletProperties {
                network: Network::Signet,
                fingerprint: seed.fingerprint(),
                server_pubkey: None,
                server_mailbox_pubkey: None,
            })
            .await
            .unwrap();
        }
        let cfg = Config {
            server_address: "http://127.0.0.1:1".into(),
            esplora_address: Some(format!("http://{address}")),
            ..Config::network_default(Network::Signet)
        };
        let options = || OpenWalletArgs {
            datadir: Some(dir.path().to_path_buf()),
            create_if_not_exists: false,
            run_daemon: false,
            ..Default::default()
        };
        let first = Wallet::open(Network::Signet, seed, cfg.clone(), options())
            .await
            .unwrap();
        let (key, index) = first.derive_store_next_keypair().await.unwrap();
        let pubkey = key.public_key();
        drop(first);
        let reopened = Wallet::open(
            Network::Signet,
            WalletSeed::new_from_mnemonic(Network::Signet, &phrase),
            cfg.clone(),
            options(),
        )
        .await
        .unwrap();
        assert_eq!(
            reopened.peek_keypair(index).await.unwrap().public_key(),
            pubkey
        );
        assert_eq!(
            reopened.derive_store_next_keypair().await.unwrap().1,
            index + 1
        );
        drop(reopened);
        let other = Mnemonic::generate(12).unwrap();
        assert!(
            Wallet::open(
                Network::Signet,
                WalletSeed::new_from_mnemonic(Network::Signet, &other),
                cfg,
                options()
            )
            .await
            .is_err()
        );
        server.join().unwrap();
    }

    #[test]
    fn identity_persists_without_replacing_seed_or_switching_network() {
        let dir = tempfile::tempdir().unwrap();
        assert!(identity(dir.path(), Chain::Signet, None, None, false).is_err());
        let first = identity(dir.path(), Chain::Signet, None, None, true).unwrap();
        let reopened = identity(dir.path(), Chain::Signet, None, None, false).unwrap();
        assert!(first.mnemonic == reopened.mnemonic);
        assert!(identity(dir.path(), Chain::Mainnet, None, None, true).is_err());
        assert!(
            identity(
                dir.path(),
                Chain::Signet,
                Some("https://different.example"),
                None,
                false
            )
            .is_err()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(dir.path().join("identity.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn missing_identity_cannot_replace_existing_wallet() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("wallet")).unwrap();
        assert!(identity(dir.path(), Chain::Signet, None, None, true).is_err());
    }

    #[test]
    fn invalid_amounts_and_wrong_network_withdrawals_fail_early() {
        assert!(required_amount(Some(0)).is_err());
        assert!(required_amount(Some(u64::MAX)).is_err());
        assert!(
            Destination::parse(
                "1BoatSLRHtKNngkdXEeobR76b53LETtpyT",
                Some(1000),
                Network::Signet
            )
            .is_err()
        );
        assert!(
            Destination::parse(
                "1BoatSLRHtKNngkdXEeobR76b53LETtpyT",
                Some(1000),
                Network::Bitcoin
            )
            .is_ok()
        );
    }
}
