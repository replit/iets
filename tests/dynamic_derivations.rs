//! Exercise a generated derivation through the real daemon, without IFD.
#![cfg(unix)]

mod e2e;
mod hermetic;

use e2e::classic::Farm;
use e2e::{Store, ok, stdout};
use std::fs;
use std::path::{Path, PathBuf};

fn store() -> Option<Store> {
    Store::start_with(&[
        (
            "extra-experimental-features",
            "ca-derivations dynamic-derivations",
        ),
        ("allow-import-from-derivation", "false"),
    ])
}

/// A text-addressed output containing a derivation; instantiation only,
/// never building, prepares the fixture's bytes.
fn producer(s: &Store, leaf: &Path, name: &str) -> PathBuf {
    let drv = stdout(&ok(s.nix("nix-instantiate").arg(leaf)));
    let drv_name = Path::new(&drv).file_name().unwrap().to_str().unwrap();
    let drv_name = drv_name.split_once('-').unwrap().1;
    s.write(&format!("{name}.aterm"), &fs::read_to_string(&drv).unwrap());
    s.write(
        &format!("{name}.nix"),
        &format!(
            r#"
      derivation {{
        name = "{drv_name}";
        system = "@SYSTEM@";
        builder = "@SHELL@";
        __contentAddressed = true;
        outputHashMode = "text";
        outputHashAlgo = "sha256";
        args = [ "-c" ''IFS= read -r aterm < "$source"; printf '%s' "$aterm" > "$out"'' ];
        source = ./{name}.aterm;
        references = (import {leaf}).source or "";
      }}
    "#,
            leaf = leaf.display()
        ),
    )
}

#[test]
fn direct_dynamic_output_installables_cache_link_and_match_nix() {
    let Some(s) = store() else { return };
    let leaf = s.write(
        "direct-leaf.nix",
        r#"
      derivation {
        name = "direct-leaf-@NONCE@";
        system = "@SYSTEM@";
        builder = "@SHELL@";
        outputs = [ "out" "dev" ];
        args = [ "-c" ''echo direct > "$out"; echo headers > "$dev"'' ];
      }
    "#,
    );
    producer(&s, &leaf, "producer");
    let file = s.write(
        "direct.nix",
        r#"
      builtins.outputOf (import ./producer.nix).outPath "dev"
    "#,
    );
    let result = stdout(&ok(s.iets().args(["build", "--impure"]).arg(&file)));
    assert_eq!(fs::read_to_string(&result).unwrap(), "headers\n");
    let warm = ok(s.iets().args(["build", "--impure", "--timings"]).arg(&file));
    assert_eq!(stdout(&warm), result);
    assert!(String::from_utf8_lossy(&warm.stderr).contains("eval: skipped"));
    let reference = stdout(&ok(s
        .nix("nix")
        .args([
            "build",
            "--impure",
            "--no-link",
            "--print-out-paths",
            "--file",
        ])
        .arg(&file)));
    assert_eq!(result, reference);
    let link = s.dir().join("direct-result");
    ok(s.iets()
        .args(["build", "--impure", "--out-link"])
        .arg(&link)
        .arg(&file));
    assert_eq!(
        fs::read_link(s.dir().join("direct-result-dev")).unwrap(),
        Path::new(&result)
    );
    let farm = Farm::new(&s);
    let modern = stdout(&ok(farm
        .ours("nix")
        .args([
            "build",
            "--impure",
            "--no-link",
            "--print-out-paths",
            "--file",
        ])
        .arg(&file)
        .env("IETS_NIX_FALLBACK", "never")));
    assert_eq!(modern, reference);

    let multiple = s.write(
        "multiple.nix",
        r#"
      let p = import ./producer.nix;
      in [ (builtins.outputOf p.outPath "out") (builtins.outputOf p.outPath "dev") ]
    "#,
    );
    let both = stdout(&ok(s.iets().args(["build", "--impure"]).arg(&multiple)));
    let values: Vec<_> = both
        .lines()
        .map(|p| fs::read_to_string(p).unwrap())
        .collect();
    assert_eq!(values, ["direct\n", "headers\n"]);
}

#[test]
fn nested_dynamic_installables_do_not_build_during_evaluation() {
    let Some(s) = store() else { return };
    let leaf = s.write(
        "nested-leaf.nix",
        r#"
      derivation {
        name = "nested-leaf-@NONCE@";
        system = "@SYSTEM@";
        builder = "@SHELL@";
        args = [ "-c" ''echo nested > "$out"'' ];
      }
    "#,
    );
    let first = producer(&s, &leaf, "inner");
    producer(&s, &first, "outer");
    let file = s.write(
        "nested.nix",
        r#"
      builtins.outputOf (builtins.outputOf (import ./outer.nix).outPath "out") "out"
    "#,
    );
    let leaf_drv = stdout(&ok(s.nix("nix-instantiate").arg(&leaf)));
    let expected = stdout(&ok(s.nix("nix-store").args([
        "--query",
        "--outputs",
        &leaf_drv,
    ])));
    ok(s.iets()
        .args(["build", "--impure", "--no-build"])
        .arg(&file));
    assert!(!Path::new(&expected).exists());
    ok(s.iets().args(["build", "--impure", "--dry-run"]).arg(&file));
    assert!(!Path::new(&expected).exists());
    let result = stdout(&ok(s.iets().args(["build", "--impure"]).arg(&file)));
    assert_eq!(fs::read_to_string(&result).unwrap(), "nested\n");
    assert_eq!(result, expected);
    let warm = stdout(&ok(s.iets().args(["build", "--impure"]).arg(&file)));
    assert_eq!(warm, result);
    let farm = Farm::new(&s);
    let modern = stdout(&ok(farm
        .ours("nix")
        .args([
            "build",
            "--impure",
            "--no-link",
            "--print-out-paths",
            "--file",
        ])
        .arg(&file)
        .env("IETS_NIX_FALLBACK", "never")));
    assert_eq!(modern, result);
    // Determinate 3.24.2 cannot parse its own nested result (`^out!out!out`).
    // Compare the resulting path to Nix building the concrete leaf instead.
    let reference = stdout(&ok(s
        .nix("nix")
        .args([
            "build",
            "--impure",
            "--no-link",
            "--print-out-paths",
            "--file",
        ])
        .arg(&leaf)));
    assert_eq!(result, reference);
}

#[test]
fn missing_dynamic_output_fails_without_reporting_a_producer_as_success() {
    let Some(s) = store() else { return };
    let leaf = s.write(
        "failure-leaf.nix",
        r#"
      derivation {
        name = "failure-leaf-@NONCE@";
        system = "@SYSTEM@";
        builder = "@SHELL@";
        args = [ "-c" ''echo good > "$out"'' ];
      }
    "#,
    );
    producer(&s, &leaf, "failure-producer");
    let file = s.write(
        "missing.nix",
        r#"
      builtins.outputOf (import ./failure-producer.nix).outPath "missing"
    "#,
    );
    let result = s
        .iets()
        .args(["build", "--impure"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
}

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
