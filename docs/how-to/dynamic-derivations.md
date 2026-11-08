# Build generated derivations

iets evaluates `builtins.outputOf`, text-addressed derivation producers and
their consumers without import-from-derivation. Builds use a Nix daemon with
dynamic derivations enabled:

```nix
nix.settings.experimental-features = [
  "nix-command" "flakes" "ca-derivations" "dynamic-derivations"
  "recursive-nix"
];
nix.settings.system-features = [ "recursive-nix" "ca-derivations" ];
```

`recursive-nix` is needed when the producer runs a nested Nix evaluator,
as drvgo does. It is not needed for a producer that simply writes a `.drv`.
Enable these features on the build daemon, not just on the client.

A consumer can refer to a generated derivation's output:

```nix
pkgs.runCommand "consumer" {} ''
  ln -s ${builtins.outputOf planner.outPath "out"} "$out"
''
```

Build the consumer with `iets build --impure consumer.nix`. The daemon builds
the planner, loads the generated derivation and resolves the output before
building the consumer. iets uses the realized output paths returned by the
build operation; a deferred derivation need not have an output path available
through `QueryDerivationOutputMap`.

`nix derivation show` preserves the nested `dynamicOutputs` tree. The gRPC
build farm requires static input paths and rejects dynamic inputs explicitly;
use the local daemon or a worker-protocol remote builder instead.

Directly selecting a dynamic `outputOf` string as the installable is not yet
supported. Select a consumer derivation such as the example above. Evaluation
still never builds: `getContext` on a dynamic output and reading its contents
during evaluation remain unsupported.

## UPM with drvgo

drvgo's `examples/upm/dynamic.nix` builds UPM, its Python database generator,
the database and the executable wrapper. Pass a pinned source store path via
the example's `source` argument: its default fetch lacks the `narHash` that
iets requires. Both clients must receive the same argument.

`bench/dynamic-upm.py` alternates evaluation and warm realization runs, checks
that both clients return the same paths, and records every sample.
`bench/dynamic-upm-compile.py` measures application recompilation by adding a
fresh comment to local Go sources for each trial. External dependencies stay
cached. This includes planning, evaluation, local package compilation, database
generation and wrapping; it is not a cold dependency build or a measurement of
a different Go compiler.
