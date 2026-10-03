#!/usr/bin/env python3
"""Build and install the bar plugin without changing or copying wallet data."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="Use an already-built bark-degen binary")
    parser.add_argument("--dest", type=Path, default=Path.home() / ".config/omarchy/plugins/bark.degen")
    parser.add_argument("--no-enable", action="store_true", help="Install files without changing the live bar")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent
    if not args.no_enable and not shutil.which("omarchy"):
        parser.error("Omarchy is not installed; use --no-enable to stage the plugin elsewhere")
    binary = args.binary
    if binary is None:
        subprocess.run(["cargo", "build", "--locked", "--release"], cwd=repo, check=True)
        binary = Path(os.environ.get("CARGO_TARGET_DIR", repo / "target")) / "release/bark-degen"
        if not binary.is_absolute():
            binary = repo / binary
    binary = binary.resolve(strict=True)
    # Verify the machine protocol without opening or creating a wallet.
    help_text = subprocess.check_output([str(binary), "--help"], text=True)
    if "--json" not in help_text or "status" not in help_text:
        parser.error("binary lacks the desktop JSON protocol; rebuild it from this repository")
    dest = args.dest.expanduser().absolute()
    if dest == repo or repo in dest.parents and dest.name != "bark.degen":
        parser.error("choose a dedicated plugin destination")
    if dest.exists():
        manifest = dest / "manifest.json"
        if not manifest.exists() or json.loads(manifest.read_text()).get("id") != "bark.degen":
            parser.error("destination already exists and is not the Bark Dice plugin")
    dest.mkdir(parents=True, exist_ok=True)
    (dest / "bin").mkdir(exist_ok=True)
    # Atomic executable replacement also works while the old binary is running.
    staged = dest / "bin/bark-degen.new"
    shutil.copy2(binary, staged)
    staged.chmod(0o755)
    staged.replace(dest / "bin/bark-degen")
    shutil.copytree(repo / "omarchy", dest / "omarchy", dirs_exist_ok=True)
    shutil.copy2(repo / "manifest.json", dest / "manifest.json")
    print(f"Installed Bark Dice to {dest}")
    if not args.no_enable:
        subprocess.run(["omarchy-shell", "shell", "rescanPlugins"], check=True)
        subprocess.run(["omarchy", "plugin", "enable", "bark.degen"], check=True)


if __name__ == "__main__":
    main()
