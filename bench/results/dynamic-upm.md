# UPM dynamic derivations

Measured on 2026-11-08 with Determinate Nix 3.24.2 (Nix 2.35.3),
iets's worker-output fix, and drvgo's pinned Go toolchain. The machine was
x86_64 Linux on an Intel Xeon Platinum 8481C, limited to 4 CPUs and 12 GiB.
Both clients used the same daemon. The first compilation trials inadvertently
used different scheduling: iets's native build path used four jobs and all
cores per job, while the Nix client used two jobs and two cores per job.
Nix sandboxing was unavailable in this VM; the daemon used sandbox fallback.

The UPM source was the revision pinned by drvgo's UPM example. Both clients
received the same source store path for evaluation and cached-build trials.
iets used one evaluator worker with its tracing cache disabled. Determinate
Nix used its configured evaluator defaults. Measurements ran after setup
builds and Rust compilation finished, with alternating client order.

| Workload | iets median | Determinate median | Speedup |
|---|---:|---:|---:|
| Evaluation, five measured runs | 0.588 s | 1.373 s | 2.34× |
| Cached build, five measured runs | 0.603 s | 1.407 s | 2.33× |

The evaluation and cached-build scripts checked identical output paths.
Each fresh-source trial added an equal-length comment to local Go files.
These trials used a prewarmed store but rebuilt some external cgo dependencies
(including sqlite3), as well as local packages, the database and wrapper.
Every resulting UPM executable passed `--help`. This is end-to-end
source-change build time, not an isolated compiler or cold-store measurement.
Both clients use the same Go compiler and Nix build daemon.

Initial fresh-source samples in seconds (different budgets, not comparable):

| Trial | iets | Determinate |
|---|---:|---:|
| 1 | 89.087 | 88.911 |
| 2 | 84.074 | 89.803 |
| 3 | 81.079 | 89.691 |

No compilation speedup can be concluded from those samples. A new run with
both clients using four jobs and all cores completed the first iets build
in 76.186 seconds, but the Determinate build failed: its generated forwarding
derivation was killed with signal 9. The cgroup reported no OOM events or
OOM kills. The cause is unresolved. A similar forwarding-build failure
occurred during the initial iets setup; neither failure supports a claim
that one client is more reliable.

The matched run was excluded rather than retrying a partly cached build as
if it were fresh. Evaluation and warm-build results remain valid; all fresh
builds share the same compiler. A sandbox-capable host is recommended for a
reliable compilation comparison.

Reproduce with `bench/dynamic-upm.py` and `bench/dynamic-upm-compile.py`.
Raw samples and commands are in the adjacent JSON files.
