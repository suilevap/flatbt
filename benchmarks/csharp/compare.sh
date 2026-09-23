#!/usr/bin/env bash
# FlatBT (Evaluate) against bt-tree, the C# library it descends from.
# Needs a .NET 10 SDK. Fetches bt-tree at a pinned commit into ./bt-tree.
# Pass --quick for shorter samples. Fails if any checksum differs.
set -euo pipefail
cd "$(dirname "$0")"
BT_TREE_COMMIT=90f5002
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1
[ -d bt-tree ] || git clone -q https://github.com/suilevap/bt-tree bt-tree
git -C bt-tree checkout -q "$BT_TREE_COMMIT"
dotnet build -c Release --nologo -v quiet >/dev/null
(cd .. && cargo build --release --quiet)
rs=$(../target/release/flatbt-compare evaluate "$@" 2>/dev/null)
cs=$(dotnet bin/Release/net10.0/BtTreeBench.dll "$@" 2>/dev/null)
python3 - "$rs" "$cs" <<'PY'
import sys
rows = [l.split("\t") for l in (sys.argv[1] + "\n" + sys.argv[2]).splitlines() if l]
ref = {r[1]: r for r in rows if r[0] == "flatbt-evaluate"}
ok = True
for scenario in dict.fromkeys(r[1] for r in rows):
    base = ref[scenario]
    print(f"\n### {scenario}\n")
    print("| library | ns/tick, 1 agent | ns/tick, 10k agents | bytes allocated/tick | bytes/agent | ns to build an agent | same result |")
    print("|---|--:|--:|--:|--:|--:|:-:|")
    for r in (r for r in rows if r[1] == scenario):
        same = r[7] == base[7]
        ok &= same
        x = lambda i: "" if r is base else f" ({float(r[i]) / float(base[i]):.1f}×)"
        print(f"| {r[0]} | {r[2]}{x(2)} | {r[3]}{x(3)} | {r[4]} | {r[5]} | {r[6]} | {'yes' if same else '**NO**'} |")
sys.exit(0 if ok else 1)
PY
