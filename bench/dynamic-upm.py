#!/usr/bin/env python3
"""Alternate UPM dynamic-derivation evaluation and warm builds.

Run after provisioning drvgo's pinned toolchain and building UPM once.
Compilation is deliberately not inferred from a cached realization.
"""
import argparse
import json
import os
import pathlib
import statistics
import subprocess
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--drvgo", required=True, type=pathlib.Path)
p.add_argument("--source", required=True, help="Pinned UPM source store path")
p.add_argument("--iets", required=True)
p.add_argument("--runs", type=int, default=5)
p.add_argument("--output", type=pathlib.Path, required=True)
a = p.parse_args()
if a.runs < 1:
    p.error("--runs must be positive")
expr = f'(import ./examples/upm/dynamic.nix {{ source = {a.source}; }}).package'
commands = {
    "eval": {
        "iets": [a.iets, "eval", "--impure", "--raw", "--no-tracing-cache",
                 "--workers", "1", "-E", expr + ".drvPath"],
        # Read-write mode is essential: outputOf reads its producer's .drv.
        "determinate": ["nix-instantiate", "--eval", "--strict", "--read-write-mode",
                        "-E", expr + ".drvPath"],
    },
    "warm-build": {
        "iets": [a.iets, "build", "--impure", "--no-tracing-cache",
                 "--workers", "1", "-E", expr],
        "determinate": ["nix-build", "--no-out-link", "-E", expr],
    },
}
env = dict(os.environ, NIX_REMOTE="daemon", IETS_PROGRESS="false")
records = []
for phase, clients in commands.items():
    expected = None
    for run in range(a.runs + 1):
        for client in (["iets", "determinate"] if run % 2 == 0
                       else ["determinate", "iets"]):
            start = time.perf_counter()
            r = subprocess.run(clients[client], cwd=a.drvgo, env=env,
                               text=True, capture_output=True)
            elapsed = time.perf_counter() - start
            if r.returncode:
                raise RuntimeError(f"{clients[client]}:\n{r.stderr}")
            result = r.stdout.strip().strip('"')
            if expected is None:
                expected = result
            if result != expected:
                raise RuntimeError(f"{phase}: results differ: {result} != {expected}")
            if run:
                records.append(dict(phase=phase, client=client, run=run,
                                    seconds=elapsed, result=result))
summary = {
    phase: {
        client: statistics.median(x["seconds"] for x in records
                                  if x["phase"] == phase and x["client"] == client)
        for client in clients
    } for phase, clients in commands.items()
}
report = dict(commands=commands, samples=records, medians_seconds=summary,
              source=a.source,
              nix_version=subprocess.check_output(["nix", "--version"], text=True).strip(),
              drvgo_revision=subprocess.check_output(
                  ["git", "-C", str(a.drvgo), "rev-parse", "HEAD"], text=True).strip(),
              note="Warm page/store caches; tracing cache disabled; compilation excluded.")
a.output.write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(summary, indent=2))
