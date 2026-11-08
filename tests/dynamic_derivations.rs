//! Exercise a generated derivation through the real daemon, without IFD.
#![cfg(unix)]

mod e2e;
mod hermetic;

use e2e::{Store, ok, stdout};
use std::fs;

#[test]
fn builds_generated_derivation_without_import_from_derivation() {
    let Some(s) = Store::start_with(&[
        (
            "extra-experimental-features",
            "ca-derivations dynamic-derivations",
        ),
        ("allow-import-from-derivation", "false"),
    ]) else {
        return;
    };
    let leaf = s.write(
        "leaf.nix",
        r#"
        derivation {
          name = "dynamic-leaf-@NONCE@";
          system = "@SYSTEM@";
          builder = "@SHELL@";
          args = [ "-c" "echo dynamic-success > $out" ];
        }
    "#,
    );
    let drv = stdout(&ok(s.nix("nix-instantiate").arg(&leaf)));
    // The producer copies the ATerm as text, not as a derivation dependency.
    s.write("leaf.aterm", &fs::read_to_string(&drv).unwrap());
    let file = s.write(
        "dynamic.nix",
        r#"
        let
          producer = derivation {
            name = "dynamic-leaf-@NONCE@.drv";
            system = "@SYSTEM@";
            builder = "@SHELL@";
            __contentAddressed = true;
            outputHashMode = "text";
            outputHashAlgo = "sha256";
            args = [ "-c" ''printf '%s' "$aterm" > "$out"'' ];
            aterm = builtins.readFile ./leaf.aterm;
          };
        in derivation {
          name = "dynamic-consumer-@NONCE@";
          system = "@SYSTEM@";
          builder = "@SHELL@";
          args = [ "-c" ''read value < "$input"; printf '%s\n' "$value" > "$out"'' ];
          input = builtins.outputOf producer.outPath "out";
        }
    "#,
    );
    let expected = stdout(&ok(s.nix("nix-instantiate").arg(&file)));
    // Leave no generated output for iets to accidentally reuse.
    let built = ok(s
        .iets()
        .args(["build", "--impure", "--no-tracing-cache"])
        .arg(&file));
    let result = stdout(&built);
    assert_eq!(
        fs::read_to_string(result.trim()).unwrap(),
        "dynamic-success\n"
    );
    let reference = stdout(&ok(s.nix("nix-build").arg(&file).arg("--no-out-link")));
    assert_eq!(result, reference);
    // AlreadyValid must carry the resolved path too, not only a fresh build.
    let warm = stdout(&ok(s.iets().args(["build", "--impure"]).arg(&file)));
    assert_eq!(warm, reference);
    let actual = stdout(&ok(s
        .iets()
        .args(["eval", "--impure", "--raw", "-E"])
        .arg(format!("(import {}).drvPath", file.display()))));
    assert_eq!(actual, expected);
}
