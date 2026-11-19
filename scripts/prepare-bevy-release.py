"""Select a flatbt-bevy release line without changing its Rust sources."""

import argparse
import re
import tomllib
from pathlib import Path


BEVY_VERSIONS = {"0.1": "0.19", "0.2": "0.20"}
BEVY_CRATES = ("bevy_app", "bevy_ecs", "bevy_utils", "bevy_time", "bevy_tasks")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="flatbt-bevy release version: 0.1.x or 0.2.x")
    args = parser.parse_args()
    if not re.fullmatch(r"0\.[12]\.(0|[1-9][0-9]*)", args.version):
        parser.error("supported release versions are 0.1.x (Bevy 0.19) and 0.2.x (Bevy 0.20)")
    bevy = BEVY_VERSIONS[args.version.rsplit(".", 1)[0]]
    manifest = Path(__file__).resolve().parents[1] / "crates/flatbt-bevy/Cargo.toml"
    source = manifest.read_text()
    updated, count = re.subn(
        r'^version = "[^"]+"$',
        f'version = "{args.version}"',
        source,
        flags=re.MULTILINE,
    )
    if count != 1:
        parser.error("expected one explicit package version in the flatbt-bevy manifest")
    for name in BEVY_CRATES:
        updated, count = re.subn(
            rf'^({name}\s*=\s*\{{\s*version\s*=\s*)"[^"]+"',
            lambda match: f'{match[1]}"{bevy}"',
            updated,
            flags=re.MULTILINE,
        )
        expected = 2 if name == "bevy_ecs" else 1
        if count != expected:
            parser.error(f"expected {expected} version entries for {name}")

    # Validate the complete result before writing either release configuration.
    config = tomllib.loads(updated)
    if config["package"]["name"] != "flatbt-bevy":
        parser.error("expected the flatbt-bevy package")
    for name in BEVY_CRATES:
        if config["dependencies"][name]["version"] != bevy:
            parser.error(f"incorrect Bevy version for {name}")
    if config["dev-dependencies"]["bevy_ecs"]["version"] != bevy:
        parser.error("incorrect Bevy version for the multithreaded ECS tests")

    manifest.write_text(updated)
    print(f"Prepared flatbt-bevy {args.version} for Bevy {bevy}.")
    print("Run cargo fetch to update Cargo.lock, then run the checks in CONTRIBUTING.md.")


if __name__ == "__main__":
    main()
