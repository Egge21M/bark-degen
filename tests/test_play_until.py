import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    "play_until", ROOT / "scripts" / "play_until.py"
)
assert SPEC and SPEC.loader
PLAY_UNTIL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PLAY_UNTIL)


class PlayUntilTests(unittest.TestCase):
    def test_failed_play_resumes_same_bet_without_sending_again(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            fake = directory / "fake-bark-degen"
            calls = directory / "calls.jsonl"
            fake.write_text(
                """#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

calls = Path(os.environ["FAKE_CALLS"])
with calls.open("a") as output:
    output.write(json.dumps(sys.argv[1:]) + "\\n")
if "fund" in sys.argv:
    print("Spendable: 1000 sats")
elif "--resume" in sys.argv:
    print("Verified roll: 0042. WIN. Quoted winning payout: 1970 sats.")
    print("House reports payout sent; synchronized wallet balance: 1970 sats.")
else:
    print("Bet ID: signet-bet-1", flush=True)
    raise SystemExit(1)
""",
                encoding="utf-8",
            )
            fake.chmod(0o700)
            previous = os.environ.get("FAKE_CALLS")
            os.environ["FAKE_CALLS"] = str(calls)
            try:
                args = SimpleNamespace(
                    api="https://signet.example",
                    target=1970,
                    stake=1000,
                    game="lt5000",
                    timeout=5,
                    binary=fake,
                    run_dir=directory / "run",
                    data_dir=None,
                    ark_server=None,
                    esplora=None,
                    resume_attempts=1,
                    settlement_checks=1,
                    settlement_delay=1,
                )
                runner = PLAY_UNTIL.Runner(args)
                self.assertEqual(runner.base[1:3], ["--network", "signet"])
                runner.run()
            finally:
                if previous is None:
                    os.environ.pop("FAKE_CALLS", None)
                else:
                    os.environ["FAKE_CALLS"] = previous

            commands = [json.loads(line) for line in calls.read_text().splitlines()]
            sends = [command for command in commands if "play" in command and "1000" in command]
            resumes = [command for command in commands if "--resume" in command]
            self.assertEqual(len(sends), 1)
            self.assertEqual(len(resumes), 1)
            state = json.loads((directory / "run" / "state.json").read_text())
            self.assertEqual(state["status"], "finished")
            self.assertEqual(state["stop_reason"], "target_reached")
            self.assertEqual(state["balance"], 1970)


if __name__ == "__main__":
    unittest.main()
