#!/usr/bin/env python3
"""Play Barkdice on signet until a balance target or stake floor is reached.

The runner keeps a durable journal and resumes an in-flight bet by ID. It never
repeats a stake when a previous command has an uncertain outcome.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path
from typing import Callable


GAME_TARGETS = {"lt5000": 5000, "lt2500": 2500, "lt1000": 1000, "lt0200": 200}


def positive_int(value: str) -> int:
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be greater than zero")
    return number


def arguments() -> argparse.Namespace:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(
        description="Run verified Barkdice bets on signet until a target or stake floor."
    )
    parser.add_argument(
        "--api",
        required=True,
        help="Signet Barkdice API URL (its API config must identify itself as signet)",
    )
    parser.add_argument("--target", type=positive_int, default=50_000)
    parser.add_argument("--stake", type=positive_int, default=1_000)
    parser.add_argument("--game", choices=GAME_TARGETS, default="lt5000")
    parser.add_argument("--timeout", type=positive_int, default=120)
    parser.add_argument(
        "--binary",
        type=Path,
        default=root / "target" / "release" / "bark-degen",
    )
    parser.add_argument(
        "--run-dir",
        type=Path,
        default=root / ".bark-degen-runs" / "signet-play-until",
        help="Persistent run state and command logs",
    )
    parser.add_argument("--data-dir", type=Path)
    parser.add_argument("--ark-server")
    parser.add_argument("--esplora")
    parser.add_argument("--resume-attempts", type=positive_int, default=3)
    parser.add_argument("--settlement-checks", type=positive_int, default=10)
    parser.add_argument("--settlement-delay", type=positive_int, default=3)
    return parser.parse_args()


class Runner:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.run_dir = args.run_dir.resolve()
        self.state_path = self.run_dir / "state.json"
        self.events_path = self.run_dir / "events.jsonl"
        self.state: dict[str, object] = {}
        self.base = [
            str(args.binary.resolve()),
            "--network",
            "signet",
            "--api",
            args.api,
            "--timeout",
            str(args.timeout),
        ]
        if args.data_dir:
            self.base.extend(["--data-dir", str(args.data_dir.resolve())])
        if args.ark_server:
            self.base.extend(["--ark-server", args.ark_server])
        if args.esplora:
            self.base.extend(["--esplora", args.esplora])

    def config(self) -> dict[str, object]:
        return {
            "network": "signet",
            "api": self.args.api,
            "target": self.args.target,
            "stake": self.args.stake,
            "game": self.args.game,
            "binary": str(self.args.binary.resolve()),
            "data_dir": str(self.args.data_dir.resolve()) if self.args.data_dir else None,
            "ark_server": self.args.ark_server,
            "esplora": self.args.esplora,
        }

    def save(self) -> None:
        temporary = self.state_path.with_suffix(".tmp")
        with temporary.open("w", encoding="utf-8") as output:
            json.dump(self.state, output, indent=2, sort_keys=True)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        temporary.replace(self.state_path)

    def emit(self, event: str, **fields: object) -> None:
        item = {"event": event, **fields}
        encoded = json.dumps(item, sort_keys=True)
        print(encoded, flush=True)
        with self.events_path.open("a", encoding="utf-8") as output:
            output.write(encoded + "\n")
            output.flush()
            os.fsync(output.fileno())

    def call(
        self,
        command: list[str],
        label: str,
        on_line: Callable[[str], None] | None = None,
    ) -> tuple[int, str]:
        lines: list[str] = []
        with (self.run_dir / f"{label}.log").open("w", encoding="utf-8") as log:
            process = subprocess.Popen(
                self.base + command,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                bufsize=1,
            )
            assert process.stdout is not None
            try:
                with process.stdout:
                    for line in process.stdout:
                        lines.append(line)
                        log.write(line)
                        log.flush()
                        print(line, end="", flush=True)
                        if on_line:
                            on_line(line)
                return process.wait(), "".join(lines)
            except BaseException:
                if process.poll() is None:
                    process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                raise

    @staticmethod
    def balance_from(output: str) -> int:
        matches = re.findall(
            r"(?:Spendable:|synchronized wallet balance:) (\d+) sats", output
        )
        if not matches:
            raise RuntimeError("CLI output has no confirmed spendable balance")
        return int(matches[-1])

    def balance(self, label: str) -> int:
        code, output = self.call(["fund", "--balance"], label)
        if code:
            raise RuntimeError(f"wallet synchronization failed; inspect {label}.log")
        return self.balance_from(output)

    def summary(self) -> dict[str, object]:
        return {
            "target": self.args.target,
            "stake": self.args.stake,
            "game": self.args.game,
            "games": self.state.get("games", 0),
            "wins": self.state.get("wins", 0),
            "losses": self.state.get("losses", 0),
            "initial_balance": self.state.get("initial_balance"),
            "balance": self.state.get("balance"),
            "in_flight": self.state.get("in_flight"),
            "bet_id": self.state.get("bet_id"),
        }

    def initialize(self) -> None:
        self.run_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        os.chmod(self.run_dir, 0o700)
        expected = self.config()
        if self.state_path.exists():
            self.state = json.loads(self.state_path.read_text(encoding="utf-8"))
            if self.state.get("config") != expected:
                raise RuntimeError(
                    "run arguments differ from state.json; use the original arguments or a new --run-dir"
                )
            if self.state.get("status") == "finished":
                self.emit("already_finished", **self.summary())
                return
            self.state["status"] = "running"
            self.state.pop("last_error", None)
            self.save()
            self.emit("resumed_run", **self.summary())
            return

        initial = self.balance("initial-balance")
        self.state = {
            "config": expected,
            "status": "running",
            "initial_balance": initial,
            "balance": initial,
            "games": 0,
            "wins": 0,
            "losses": 0,
        }
        self.save()
        self.emit("started", **self.summary())

    def remember_bet_id(self, line: str) -> None:
        match = re.fullmatch(r"Bet ID: ([A-Za-z0-9_-]+)\s*", line)
        if not match:
            return
        bet_id = match.group(1)
        existing = self.state.get("bet_id")
        if existing and existing != bet_id:
            raise RuntimeError("CLI printed conflicting bet IDs")
        self.state["bet_id"] = bet_id
        self.save()

    def resolved_result(self, output: str) -> tuple[int, bool, int, int]:
        result = re.search(r"Verified roll: (\d+)\. (WIN|LOSE)\.", output)
        payout = re.search(r"Quoted winning payout: (\d+) sats", output)
        if not result or not payout:
            raise RuntimeError("bet command completed without a verified result")
        roll = int(result.group(1))
        won = result.group(2) == "WIN"
        if won != (roll < GAME_TARGETS[self.args.game]):
            raise RuntimeError("verified outcome does not match the selected game")
        return roll, won, int(payout.group(1)), self.balance_from(output)

    def reconcile(self, number: int, first: tuple[int, str] | None) -> str:
        bet_id = self.state.get("bet_id")
        if not isinstance(bet_id, str):
            raise RuntimeError(
                f"game {number} has no saved bet ID; no later stake will be submitted"
            )
        if first and first[0] == 0:
            return first[1]
        for attempt in range(1, self.args.resume_attempts + 1):
            self.emit("reconciling", game=number, bet_id=bet_id, attempt=attempt)
            if attempt > 1:
                time.sleep(2)
            code, output = self.call(
                ["play", "--resume", bet_id], f"game-{number}-resume-{attempt}"
            )
            if code == 0:
                return output
        raise RuntimeError(f"game {number} remains unresolved; no new stake was sent")

    def settle(self, number: int, output: str) -> None:
        roll, won, payout, actual = self.resolved_result(output)
        previous = int(self.state["balance"])
        expected = previous - self.args.stake + (payout if won else 0)
        for attempt in range(1, self.args.settlement_checks + 1):
            if actual >= expected:
                break
            self.emit(
                "awaiting_wallet_settlement",
                game=number,
                balance=actual,
                expected=expected,
            )
            time.sleep(self.args.settlement_delay)
            actual = self.balance(f"game-{number}-balance-{attempt}")
        if actual < expected:
            raise RuntimeError(
                f"wallet balance {actual} is below expected {expected}; stopped for reconciliation"
            )

        bet_id = self.state["bet_id"]
        self.state["games"] = number
        counter = "wins" if won else "losses"
        self.state[counter] = int(self.state[counter]) + 1
        self.state["balance"] = actual
        self.state.pop("in_flight", None)
        self.state.pop("bet_id", None)
        self.save()
        self.emit(
            "settled",
            game=number,
            bet_id=bet_id,
            roll=roll,
            result="win" if won else "loss",
            balance=actual,
            wins=self.state["wins"],
            losses=self.state["losses"],
        )

    def run(self) -> None:
        self.initialize()
        if self.state.get("status") == "finished":
            return

        while self.args.stake <= int(self.state["balance"]) < self.args.target:
            in_flight = self.state.get("in_flight")
            if in_flight is not None:
                number = int(in_flight)
                output = self.reconcile(number, None)
            else:
                number = int(self.state["games"]) + 1
                self.state["in_flight"] = number
                self.save()
                self.emit("placing", game=number, balance=self.state["balance"])
                first = self.call(
                    ["play", str(self.args.stake), "--game", self.args.game],
                    f"game-{number}",
                    self.remember_bet_id,
                )
                output = self.reconcile(number, first)
            self.settle(number, output)

        self.state["status"] = "finished"
        self.state["stop_reason"] = (
            "target_reached"
            if int(self.state["balance"]) >= self.args.target
            else "below_minimum_stake"
        )
        self.save()
        self.emit(
            "finished",
            **self.summary(),
            stop_reason=self.state["stop_reason"],
            net=int(self.state["balance"]) - int(self.state["initial_balance"]),
        )


def main() -> int:
    os.umask(0o077)
    args = arguments()
    if not args.binary.is_file():
        print(f"error: bark-degen binary not found: {args.binary}", file=sys.stderr)
        return 2
    runner = Runner(args)
    try:
        runner.run()
    except KeyboardInterrupt:
        runner.state["status"] = "paused"
        runner.state["last_error"] = "interrupted"
        runner.save()
        runner.emit("paused", **runner.summary())
        return 130
    except Exception as error:
        if runner.state:
            runner.state["status"] = "paused"
            runner.state["last_error"] = str(error)
            runner.save()
            runner.emit("stopped", error=str(error), **runner.summary())
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
