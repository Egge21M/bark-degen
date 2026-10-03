use serde_json::{Value, json};
use std::process::Command;

fn status(dir: &std::path::Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_bark-degen"))
        .args(["--json", "--network", "signet", "--data-dir"])
        .arg(dir)
        .arg("status")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn desktop_snapshot_recovers_payments_without_exposing_secrets_or_opening_wallet() {
    let temp = tempfile::tempdir().unwrap();
    let initial = status(temp.path());
    assert_eq!(initial["event"], "snapshot");
    assert_eq!(initial["initialized"], false);
    assert!(!temp.path().join("signet/identity.json").exists());
    let db = rusqlite::Connection::open(temp.path().join("signet/bets.sqlite")).unwrap();
    let record = json!({"api":"https://example.com", "token":"SECRET_BET_TOKEN"});
    for (id, kind, state, payload) in [
        ("interrupted", "play", "sending", record.clone()),
        ("unpaid", "play", "requesting_quote", record),
        (
            "withdrawal",
            "withdraw",
            "unknown",
            json!({"destination":"address", "amount_sat":1000, "result":null}),
        ),
        ("completed", "withdraw", "completed", json!({})),
        ("funded", "fund", "settled", json!({"invoice":"unused"})),
    ] {
        db.execute(
            "INSERT INTO operations(id,kind,state,payload) VALUES(?1,?2,?3,?4)",
            rusqlite::params![id, kind, state, payload.to_string()],
        )
        .unwrap();
    }
    let snapshot = status(temp.path());
    assert_eq!(snapshot["operations"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["operations"][0]["id"], "withdrawal");
    assert_eq!(snapshot["operations"][1]["id"], "interrupted");
    assert!(!snapshot.to_string().contains("SECRET_BET_TOKEN"));
    assert!(!temp.path().join("signet/identity.json").exists());
}

#[test]
fn desktop_errors_are_json_and_do_not_create_a_spending_identity() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bark-degen"))
        .args(["--json", "--network", "signet", "--data-dir"])
        .arg(temp.path())
        .args(["play", "1000"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["event"], "error");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("wallet does not exist")
    );
    assert!(!temp.path().join("signet/identity.json").exists());
}
