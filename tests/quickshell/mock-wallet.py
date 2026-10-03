#!/usr/bin/env python3
"""Deterministic wallet protocol fixture. Never loads the real wallet SDK."""
import json
import os
from pathlib import Path
import sys
import time

state = Path(os.environ["BARK_TEST_STATE"])
args = sys.argv[1:]
with (state / "calls.jsonl").open("a") as log:
    log.write(json.dumps(args) + "\n")


def event(name, **fields):
    print(json.dumps({"event": name, **fields}), flush=True)


pending = state / "pending.json"
if "status" in args:
    event("snapshot", initialized=True, network="signet",
          operations=json.loads(pending.read_text()) if pending.exists() else [])
elif "play" in args:
    if "--resume" in args:
        pending.unlink(missing_ok=True)
        event("bet_result", id="saved-bet", roll=42, win=True, payout_sat=1970, balance_sat=10970)
    elif "lt0200" in args:
        pending.write_text(json.dumps([{"id": "saved-bet", "kind": "play", "state": "payment_unknown", "resumable": True}]))
        event("error", message="Simulated interruption after payment. Resume the saved bet.")
        sys.exit(1)
    else:
        event("operation", id="first-bet", kind="play", state="requesting_commitment")
        time.sleep(0.3)
        event("bet_result", id="first-bet", roll=42, win=True, payout_sat=1970, balance_sat=10970)
elif "withdraw" in args:
    event("operation", kind="withdraw", id="withdrawal", state="prepared")
    time.sleep(0.2)
    event("withdrawal_status", id="withdrawal", state="completed", result="Sent")
elif "--balance" in args:
    event("balance", balance_sat=10000)
elif "5000" in args:
    event("deposit_estimate", amount_sat=5000, net_sat=4900, fee_sat=100)
    event("deposit", id="deposit", invoice="lnbc-test-invoice")
    time.sleep(0.3)
    event("funded", id="deposit", balance_sat=14900)
else:
    event("balance", balance_sat=10000)
    event("address", address="ark-test-address")
