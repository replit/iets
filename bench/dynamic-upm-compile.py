#!/usr/bin/env python3
"""Measure UPM application recompilation with cached external dependencies.

Each trial gets a fresh source snapshot with an equal-length Go comment added
to every local Go file. This invalidates UPM packages without deleting store
objects. It is not a cold toolchain/dependency benchmark.
"""
import argparse
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--drvgo", type=pathlib.Path, required=True)
p.add_argument("--source", type=pathlib.Path, required=True)
p.add_argument("--iets", required=True)
p.add_argument("--runs", type=int, default=3)
p.add_argument("--output", type=pathlib.Path, required=True)
a = p.parse_args()
if a.runs < 1:
    p.error("--runs must be positive")
env = dict(os.environ, NIX_REMOTE="daemon", IETS_PROGRESS="false")
results = []
with tempfile.TemporaryDirectory(prefix="upm-compile-") as tmp:
    for trial in range(a.runs):
        clients = ["iets", "determinate"]
        if trial % 2:
            clients.reverse()
        for client in clients:
            src = pathlib.Path(tmp) / f"{trial}-{client}"
            shutil.copytree(a.source, src)
            for path in src.rglob("*.go"):
                path.chmod(0o644)
                with path.open("a") as f:
                    f.write(f"\n// benchmark {trial:03d} {client:11s}\n")
            expr = f'(import ./examples/upm/dynamic.nix {{ source = {src}; }}).package'
            cmd = ([a.iets, "build", "--impure", "--no-tracing-cache", "--workers", "1"]
                   if client == "iets" else ["nix-build", "--no-out-link"])
            cmd += ["-E", expr]
            start = time.perf_counter()
            r = subprocess.run(cmd, cwd=a.drvgo, env=env, text=True, capture_output=True)
            elapsed = time.perf_counter() - start
            log = a.output.with_suffix(f".{trial}-{client}.log")
            log.write_text(r.stderr)
            if r.returncode:
                raise RuntimeError(f"{cmd} failed: see {log}")
            output = r.stdout.strip()
            smoke = subprocess.run([output + "/bin/upm", "--help"],
                                   text=True, capture_output=True)
            if smoke.returncode:
                raise RuntimeError(f"UPM smoke test failed: {smoke.stderr}")
            results.append(dict(client=client, trial=trial, seconds=elapsed,
                                output=output, command=cmd, help_exit=smoke.returncode))
            a.output.write_text(json.dumps({
                "note": "Local UPM package recompilation plus planning/evaluation/database/wrapping; external dependencies cached.",
                "samples": results,
            }, indent=2) + "\n")
            print(client, trial, elapsed, flush=True)
