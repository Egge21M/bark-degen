#!/usr/bin/env python3
"""Exercise real Quickshell + Omarchy components using a wallet protocol fixture."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--quickshell", default="quickshell")
    parser.add_argument("--compositor", default="labwc", help="Headless Wayland compositor")
    parser.add_argument("--shell", type=Path, default=Path("/usr/share/omarchy/shell"), help="Omarchy shell source directory")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent
    with tempfile.TemporaryDirectory(prefix="bark-qml-") as folder:
        root = Path(folder)
        runtime = root / "runtime"
        runtime.mkdir(mode=0o700)
        shutil.copytree(repo / "omarchy", root / "omarchy")
        for part in ["Commons", "Ui"]:
            shutil.copytree(args.shell / part, root / part)
        state = root / "state"
        state.mkdir()
        mock_bin = root / "bin"
        mock_bin.mkdir()
        notifier = mock_bin / "notify-send"
        notifier.write_text('#!/usr/bin/env python3\nimport json, os, sys\nfrom pathlib import Path\n'
                            'with (Path(os.environ["BARK_TEST_STATE"]) / "notifications.jsonl").open("a") as f:\n'
                            '    f.write(json.dumps(sys.argv[1:]) + "\\n")\n')
        notifier.chmod(0o755)
        env = dict(os.environ, QT_QPA_PLATFORM="offscreen", QT_QUICK_BACKEND="software",
                   PATH=str(mock_bin) + os.pathsep + os.environ["PATH"],
                   XDG_RUNTIME_DIR=str(runtime), BARK_TEST_STATE=str(state),
                   BARK_TEST_BINARY=str(repo / "tests/quickshell/mock-wallet.py"))
        compositor = None
        try:
            for harness in ["shell.qml", "widget.qml"]:
                if harness == "widget.qml":
                    config = root / "labwc"
                    config.mkdir()
                    (config / "autostart").write_text("")
                    with (root / "compositor.log").open("w") as log:
                        compositor = subprocess.Popen([args.compositor, "-C", str(config)],
                            env=dict(env, WLR_BACKENDS="headless", WLR_RENDERER="pixman", WLR_HEADLESS_OUTPUTS="1"),
                            stdout=log, stderr=subprocess.STDOUT)
                    deadline = time.monotonic() + 10
                    while not list(runtime.glob("wayland-*.lock")):
                        if compositor.poll() is not None or time.monotonic() > deadline:
                            raise RuntimeError((root / "compositor.log").read_text())
                        time.sleep(0.05)
                    socket = list(runtime.glob("wayland-*.lock"))[0].with_suffix("")
                    env.update(QT_QPA_PLATFORM="wayland", WAYLAND_DISPLAY=str(socket))
                (root / "shell.qml").write_text((repo / "tests/quickshell" / harness).read_text().replace("../../omarchy", "omarchy"))
                (state / "calls.jsonl").unlink(missing_ok=True)
                (state / "notifications.jsonl").unlink(missing_ok=True)
                result = subprocess.run([args.quickshell, "-p", str(root / "shell.qml")], env=env,
                                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=40)
                print(result.stdout)
                if result.returncode != 0 or "PASS:" not in result.stdout or "FAIL:" in result.stdout or "ERROR:" in result.stdout:
                    raise SystemExit(f"Failed runtime harness: {harness}")
                calls = [json.loads(line) for line in (state / "calls.jsonl").read_text().splitlines()]
                plays = [call for call in calls if "play" in call and "--resume" not in call]
                assert len(plays) == (2 if harness == "shell.qml" else 1), calls
                assert all(call[call.index("--network") + 1] == "signet" for call in calls), calls
                expected_notifications = 2 if harness == "shell.qml" else 3
                deadline = time.monotonic() + 5
                while True:
                    notifications = [json.loads(line) for line in (state / "notifications.jsonl").read_text().splitlines()]
                    if len(notifications) >= expected_notifications or time.monotonic() > deadline:
                        break
                    time.sleep(0.05)  # Detached notifier processes can finish after the shell exits.
                assert len(notifications) == expected_notifications, notifications
                if harness == "widget.qml":
                    assert plays[0][-4:] == ["play", "3000", "--game", "lt2500"], plays
                    assert sorted(n[-2] for n in notifications) == ["Bark Dice — LOSS", "Bark Dice — WIN!", "Bark Dice — WIN!"], notifications
                    assert all("1970 sats paid" in n[-1] for n in notifications if n[-2].endswith("WIN!")), notifications
                    loss = next(n for n in notifications if n[-2].endswith("LOSS"))
                    assert "paid" not in loss[-1] and "9999" in loss[-1], notifications
                    assert all("signet" in n[-1] and "--" in n for n in notifications), notifications
        finally:
            if compositor is not None:
                compositor.terminate()
                compositor.wait(timeout=5)
    print("Runtime contracts passed; no real wallet or funds were used.")


if __name__ == "__main__":
    main()
