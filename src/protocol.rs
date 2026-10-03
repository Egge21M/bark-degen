use anyhow::{Context, Result, bail, ensure};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const RULES: &str = "bark-dice/v1";

pub fn target(game: &str) -> Result<u64> {
    match game {
        "lt5000" => Ok(5000),
        "lt2500" => Ok(2500),
        "lt1000" => Ok(1000),
        "lt0200" => Ok(200),
        _ => bail!("unsupported game {game}"),
    }
}

pub fn payout(stake: u64, target: u64) -> Result<u64> {
    ensure!(stake > 0 && stake <= 2_100_000_000_000_000, "invalid stake");
    ensure!(target > 0 && target < 10000, "invalid target");
    u64::try_from(u128::from(stake) * 9850 / u128::from(target)).context("payout overflow")
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub network: String,
    pub rules_version: String,
    pub quotes_open: bool,
    pub closed_reason: Option<String>,
    pub min_stake_sat: u64,
    pub max_stake_sat: u64,
    pub max_payout_sat: u64,
    pub quote_ttl_secs: u64,
    pub roll_range: u64,
    pub house_edge_bps: u64,
    pub games: Vec<Game>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Game {
    pub id: String,
    pub target: u64,
    pub max_stake_sat: u64,
    pub max_payout_sat: u64,
}

impl Config {
    pub fn validate(&self, network: &str, game: &str, stake: u64) -> Result<()> {
        ensure!(
            self.network == network,
            "Barkdice uses {}, wallet uses {network}",
            self.network
        );
        ensure!(
            self.rules_version == RULES && self.roll_range == 10000 && self.house_edge_bps == 150,
            "unsupported Barkdice rules"
        );
        ensure!(
            self.quotes_open,
            "house closed: {}",
            self.closed_reason.as_deref().unwrap_or("unavailable")
        );
        let g = self
            .games
            .iter()
            .find(|g| g.id == game)
            .context("game unavailable")?;
        ensure!(g.target == target(game)?, "server changed game target");
        ensure!(
            stake >= self.min_stake_sat && stake <= self.max_stake_sat && stake <= g.max_stake_sat,
            "stake outside limits (minimum {}, game maximum {})",
            self.min_stake_sat,
            g.max_stake_sat
        );
        let pay = payout(stake, g.target)?;
        ensure!(
            pay <= self.max_payout_sat && pay <= g.max_payout_sat,
            "payout exceeds house limit"
        );
        ensure!(
            self.quote_ttl_secs > 0 && self.quote_ttl_secs <= 86400,
            "invalid quote lifetime"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Draft {
    pub draft_token: String,
    pub commitment: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BetRequest {
    pub draft_token: String,
    pub game: String,
    pub stake_sat: u64,
    pub payout_address: String,
    pub client_seed: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Terms {
    pub rules_version: String,
    pub bet_id: String,
    pub game: String,
    pub target: u64,
    pub stake_sat: u64,
    pub payout_sat: u64,
    pub payout_address: String,
    pub pay_to: String,
    pub expires_at: i64,
    pub server_seed_commitment: String,
    pub client_seed: String,
}

impl Terms {
    pub fn canonical(&self) -> String {
        format!(
            "rules_version={}\nbet_id={}\ngame={}\ntarget={}\nstake_sat={}\npayout_sat={}\npayout_address={}\npay_to={}\nexpires_at={}\nserver_seed_commitment={}\nclient_seed={}\n",
            self.rules_version,
            self.bet_id,
            self.game,
            self.target,
            self.stake_sat,
            self.payout_sat,
            self.payout_address,
            self.pay_to,
            self.expires_at,
            self.server_seed_commitment,
            self.client_seed
        )
    }

    pub fn hash(&self) -> String {
        hex::encode(Sha256::digest(self.canonical().as_bytes()))
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.rules_version == RULES, "unsupported receipt rules");
        ensure!(target(&self.game)? == self.target, "game/target mismatch");
        ensure!(
            payout(self.stake_sat, self.target)? == self.payout_sat,
            "incorrect payout formula"
        );
        for value in [
            &self.bet_id,
            &self.payout_address,
            &self.pay_to,
            &self.client_seed,
        ] {
            ensure!(
                !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control),
                "invalid canonical term"
            );
        }
        ensure!(self.client_seed.len() <= 64, "client seed too long");
        hash_bytes(&self.server_seed_commitment)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Receipt {
    pub terms: Terms,
    pub terms_hash: String,
    pub server_seed: String,
    pub roll: u64,
    pub win: bool,
    pub payout_sat: u64,
}

impl Receipt {
    pub fn verify(&self, accepted: Option<&Terms>) -> Result<()> {
        self.terms.validate()?;
        if let Some(accepted) = accepted {
            ensure!(
                &self.terms == accepted,
                "receipt differs from the locally accepted quote"
            );
        }
        let seed = hash_bytes(&self.server_seed)?;
        ensure!(
            hex::encode(Sha256::digest(seed)) == self.terms.server_seed_commitment,
            "revealed seed does not match commitment"
        );
        ensure!(
            self.terms.hash() == self.terms_hash,
            "receipt terms hash mismatch"
        );
        let hash = hash_bytes(&self.terms_hash)?;
        let roll = compute_roll(&seed, &hash)?;
        ensure!(roll == self.roll, "incorrect roll");
        let win = roll < self.terms.target;
        ensure!(self.win == win, "incorrect outcome");
        ensure!(
            self.payout_sat == if win { self.terms.payout_sat } else { 0 },
            "incorrect receipt payout"
        );
        Ok(())
    }
}

pub fn hash_bytes(value: &str) -> Result<[u8; 32]> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "expected 32-byte lowercase hex value"
    );
    hex::decode(value)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid hash"))
}

fn compute_roll(seed: &[u8; 32], terms_hash: &[u8; 32]) -> Result<u64> {
    // A finite guard protects against malformed/unexpected protocol behavior.
    for counter in 0u32..1000 {
        let mut mac = Hmac::<Sha256>::new_from_slice(seed)?;
        mac.update(b"bark-dice/v1/roll");
        mac.update(terms_hash);
        mac.update(&counter.to_be_bytes());
        for bytes in mac.finalize().into_bytes().as_chunks::<4>().0 {
            let sample = u32::from_be_bytes(*bytes);
            if sample < 4_294_960_000 {
                return Ok(u64::from(sample % 10000));
            }
        }
    }
    bail!("roll rejection-sampling limit exceeded")
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Obligation {
    pub kind: String,
    pub state: String,
    pub amount_sat: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Bet {
    pub id: String,
    pub status: String,
    pub terms: Terms,
    pub terms_hash: String,
    pub terms_canonical: String,
    pub now: i64,
    pub receipt: Option<Receipt>,
    pub server_seed: Option<String>,
    pub roll: Option<u64>,
    pub win: Option<bool>,
    pub payments: Vec<serde_json::Value>,
    pub obligations: Vec<Obligation>,
}

impl Bet {
    pub fn validate_terms(&self, accepted: &Terms) -> Result<()> {
        self.terms.validate()?;
        ensure!(
            &self.terms == accepted && self.id == accepted.bet_id,
            "server changed accepted bet terms"
        );
        ensure!(
            self.terms_hash == accepted.hash(),
            "incorrect quote terms hash"
        );
        ensure!(
            self.terms_canonical == accepted.canonical(),
            "incorrect canonical quote terms"
        );
        ensure!(
            [
                "awaiting_payment",
                "expired",
                "lost",
                "payout_pending",
                "payout_review",
                "paid"
            ]
            .contains(&self.status.as_str()),
            "unrecognized bet status"
        );
        if let Some(r) = &self.receipt {
            r.verify(Some(accepted))?;
            ensure!(
                self.roll == Some(r.roll) && self.win == Some(r.win),
                "bet result differs from receipt"
            );
            ensure!(
                self.server_seed.as_deref() == Some(r.server_seed.as_str()),
                "bet seed differs from receipt"
            );
        }
        Ok(())
    }

    pub fn validate_quote(
        &self,
        draft: &Draft,
        request: &BetRequest,
        cfg: &Config,
        now: i64,
    ) -> Result<()> {
        self.validate_terms(&self.terms)?;
        let t = &self.terms;
        ensure!(
            t.server_seed_commitment == draft.commitment,
            "house changed original commitment"
        );
        ensure!(
            t.game == request.game
                && t.stake_sat == request.stake_sat
                && t.payout_address == request.payout_address
                && t.client_seed == request.client_seed,
            "quote differs from requested bet"
        );
        ensure!(
            self.status == "awaiting_payment"
                && self.receipt.is_none()
                && self.server_seed.is_none()
                && self.roll.is_none()
                && self.win.is_none()
                && self.payments.is_empty()
                && self.obligations.is_empty(),
            "quote has already been used or revealed"
        );
        ensure!(
            self.now.abs_diff(now) <= 120,
            "server/local clock differs by more than two minutes"
        );
        ensure!(
            t.expires_at > self.now.saturating_add(20) && t.expires_at > now.saturating_add(20),
            "quote expired or too close to expiry"
        );
        ensure!(
            t.expires_at <= self.now.saturating_add(cfg.quote_ttl_secs as i64 + 60),
            "unexpected quote expiry"
        );
        ensure!(
            t.pay_to != t.payout_address,
            "stake destination equals payout address"
        );
        Ok(())
    }

    pub fn settled(&self) -> Result<bool> {
        if self.obligations.iter().any(|o| o.state == "manual_review")
            || self.status == "payout_review"
        {
            bail!("payout/refund requires house operator review; claim is saved locally");
        }
        if self.obligations.iter().any(|o| o.state != "sent") {
            return Ok(false);
        }
        match self.status.as_str() {
            "paid" => {
                let r = self
                    .receipt
                    .as_ref()
                    .context("paid bet is missing its receipt")?;
                ensure!(r.win, "paid bet has losing receipt");
                ensure!(
                    self.obligations
                        .iter()
                        .any(|o| o.kind == "payout" && o.amount_sat == self.terms.payout_sat),
                    "paid bet has no matching payout obligation"
                );
                Ok(true)
            }
            "lost" => {
                ensure!(
                    !self
                        .receipt
                        .as_ref()
                        .context("lost bet is missing its receipt")?
                        .win,
                    "lost bet has winning receipt"
                );
                Ok(true)
            }
            "expired" => Ok(true),
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vectors() -> Vec<Receipt> {
        serde_json::from_str(include_str!("../tests/fixtures/test-vectors.json")).unwrap()
    }

    #[test]
    fn published_vectors_and_tampering() {
        for r in vectors() {
            r.verify(None).unwrap();
            let mut changed = r.clone();
            changed.roll = (r.roll + 1) % 10000;
            assert!(changed.verify(None).is_err());
            let mut changed = r.clone();
            changed.terms.payout_address.push('x');
            assert!(changed.verify(None).is_err());
            let mut changed = r.clone();
            changed.win = !changed.win;
            assert!(changed.verify(None).is_err());
            let mut changed = r.clone();
            changed.server_seed = "ff".repeat(32);
            assert!(changed.verify(None).is_err());
            let mut accepted = r.terms.clone();
            accepted.client_seed.push('x');
            assert!(r.verify(Some(&accepted)).is_err());
        }
    }

    pub fn quote_fixture() -> (Bet, Draft, BetRequest, Config) {
        let r = vectors().remove(0);
        let t = r.terms;
        let draft = Draft {
            draft_token: "draft".into(),
            commitment: t.server_seed_commitment.clone(),
        };
        let req = BetRequest {
            draft_token: draft.draft_token.clone(),
            game: t.game.clone(),
            stake_sat: t.stake_sat,
            payout_address: t.payout_address.clone(),
            client_seed: t.client_seed.clone(),
        };
        let cfg = Config {
            network: "signet".into(),
            rules_version: RULES.into(),
            quotes_open: true,
            closed_reason: None,
            min_stake_sat: 1000,
            max_stake_sat: 5000,
            max_payout_sat: 50000,
            quote_ttl_secs: 600,
            roll_range: 10000,
            house_edge_bps: 150,
            games: vec![Game {
                id: "lt5000".into(),
                target: 5000,
                max_stake_sat: 5000,
                max_payout_sat: 9850,
            }],
        };
        let bet = Bet {
            id: t.bet_id.clone(),
            status: "awaiting_payment".into(),
            terms_hash: t.hash(),
            terms_canonical: t.canonical(),
            now: t.expires_at - 600,
            terms: t,
            receipt: None,
            server_seed: None,
            roll: None,
            win: None,
            payments: vec![],
            obligations: vec![],
        };
        (bet, draft, req, cfg)
    }

    #[test]
    fn rejects_changed_quotes_and_expiry_before_spending() {
        let (bet, draft, req, cfg) = quote_fixture();
        bet.validate_quote(&draft, &req, &cfg, bet.now).unwrap();
        assert!(
            bet.validate_quote(&draft, &req, &cfg, bet.terms.expires_at)
                .is_err()
        );
        let mut wrong = draft.clone();
        wrong.commitment = "00".repeat(32);
        assert!(bet.validate_quote(&wrong, &req, &cfg, bet.now).is_err());
        let mut wrong = req.clone();
        wrong.stake_sat += 1;
        assert!(bet.validate_quote(&draft, &wrong, &cfg, bet.now).is_err());
        let mut wrong = bet.clone();
        wrong.terms_canonical.push('\n');
        assert!(wrong.validate_quote(&draft, &req, &cfg, bet.now).is_err());
        let mut wrong = bet.clone();
        wrong.server_seed = Some("00".repeat(32));
        assert!(wrong.validate_quote(&draft, &req, &cfg, bet.now).is_err());
        assert!(cfg.validate("mainnet", "lt5000", 1000).is_err());
        assert!(cfg.validate("signet", "lt5000", 999).is_err());
    }

    #[test]
    fn malformed_terms_cannot_panic_or_inject_lines() {
        let mut r = vectors().remove(0);
        r.terms.target = 0;
        assert!(r.verify(None).is_err());
        r.terms.target = 5000;
        r.terms.client_seed = "x\nstake_sat=1".into();
        assert!(r.verify(None).is_err());
        assert!(payout(u64::MAX, 5000).is_err());
    }

    #[test]
    fn win_does_not_mean_payout_is_settled() {
        let (mut bet, _, _, _) = quote_fixture();
        let receipt = vectors().remove(0);
        bet.roll = Some(receipt.roll);
        bet.win = Some(true);
        bet.server_seed = Some(receipt.server_seed.clone());
        bet.receipt = Some(receipt);
        bet.status = "payout_pending".into();
        bet.obligations.push(Obligation {
            kind: "payout".into(),
            state: "pending".into(),
            amount_sat: 1970,
        });
        bet.validate_terms(&bet.terms).unwrap();
        assert!(!bet.settled().unwrap());
        bet.status = "paid".into();
        assert!(!bet.settled().unwrap());
        bet.obligations[0].state = "manual_review".into();
        assert!(bet.settled().is_err());
        bet.obligations[0].state = "sent".into();
        assert!(bet.settled().unwrap());
        bet.obligations[0].amount_sat -= 1;
        assert!(bet.settled().is_err());
        bet.obligations[0].amount_sat = 1970;
        bet.status = "lost".into();
        assert!(bet.settled().is_err());
    }
}
