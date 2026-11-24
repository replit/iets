//! Opt-in adapter for go2nix's JSON resolver, supplied by the host rather
//! than a flake. Native Nix plugins cannot run inside this evaluator.

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use super::Registry;
use super::paths;
use super::store_hooks::{self, Follow};
use super::text::json;
use crate::eval::{Error, ErrorKind, EvalResult, Evaluator, Pos, bail};
use crate::print;
use crate::runtime::{Ctx, Word};
use crate::store::nar;

pub fn register(r: &mut Registry) {
    r.op("resolveGoPackages", 1, resolve);
}

fn resolve(ev: &mut Evaluator, args: &[Word], pos: Pos) -> EvalResult<Word> {
    ev.trace().mark_uncacheable();
    let fail = |e| Error::boxed(ErrorKind::Eval, format!("resolveGoPackages: {e}"), pos);
    let helper = ev
        .session()
        .env
        .non_empty("IETS_GO2NIX_RESOLVER")
        .expect("registered only with a host resolver");
    if !Path::new(&helper).is_absolute() {
        return Err(fail(
            "IETS_GO2NIX_RESOLVER must be an absolute executable path".to_string(),
        ));
    }
    ev.force_attrs(
        args[0],
        pos,
        "while evaluating the argument to builtins.resolveGoPackages",
    )?;
    let mut input = Vec::new();
    let mut context = Vec::new();
    print::print_json(ev, args[0], &mut input, &mut context, true, pos)?;
    if context
        .iter()
        .any(|c| matches!(c, Ctx::Built { .. } | Ctx::DrvDeep(_)))
    {
        bail!(
            ErrorKind::Unsupported,
            pos,
            "resolveGoPackages: derivation-backed arguments require import from derivation"
        );
    }
    let mut input: serde_json::Value =
        serde_json::from_slice(&input).map_err(|e| fail(e.to_string()))?;
    let src = input
        .get("src")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| fail("'src' must be a path or string".to_string()))?;
    let path = ev.mk_string(src.as_bytes());
    let path = paths::realise_path(ev, path, pos)?;
    store_hooks::confine(ev, &path, Follow::Full, pos)?;
    let source = store_hooks::check(ev, store_hooks::locate(ev, &path), &path, Follow::Full, pos)?;
    let dir = tempfile::tempdir().map_err(|e| fail(e.to_string()))?;
    let mut archive = tempfile::tempfile().map_err(|e| fail(e.to_string()))?;
    source.nar(&mut archive).map_err(|e| fail(e.to_string()))?;
    archive.rewind().map_err(|e| fail(e.to_string()))?;
    let materialized = dir.path().join("src");
    nar::restore_path(&mut archive, &materialized).map_err(|e| fail(e.to_string()))?;
    input["src"] = materialized.to_string_lossy().into_owned().into();

    // A file avoids pipe backpressure if the helper writes before reading.
    let mut request = tempfile::tempfile().map_err(|e| fail(e.to_string()))?;
    serde_json::to_writer(&mut request, &input).map_err(|e| fail(e.to_string()))?;
    request.flush().map_err(|e| fail(e.to_string()))?;
    request
        .seek(SeekFrom::Start(0))
        .map_err(|e| fail(e.to_string()))?;
    let result = Command::new(helper)
        .stdin(Stdio::from(request))
        .output()
        .map_err(|e| fail(e.to_string()))?;
    if !result.status.success() {
        return Err(fail(format!(
            "resolver exited with {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    let decoded: serde_json::Value =
        serde_json::from_slice(&result.stdout).map_err(|e| fail(e.to_string()))?;
    if !decoded.is_object() {
        return Err(fail("resolver must return a JSON object".to_string()));
    }
    let output = ev.mk_string(&result.stdout);
    json::from_json(ev, &[output], pos)
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use crate::env::Env;
    use crate::eval::{EvalConfig, Evaluator, Pos};
    use crate::runtime::Word;
    use crate::session::Session;
    use crate::sys;

    fn evaluator(helper: Option<&Path>) -> Evaluator {
        let env = match helper {
            Some(path) => Env::isolated().with([("IETS_GO2NIX_RESOLVER", path.as_os_str())]),
            None => Env::isolated(),
        };
        EvalConfig::new()
            .session(Session::with_env(env))
            .trace(true)
            .build()
    }

    fn helper(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("resolver");
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    fn absent_without_host_opt_in() {
        let mut ev = evaluator(None);
        let v = ev
            .eval_string("builtins ? resolveGoPackages", b"/")
            .unwrap();
        assert_eq!(ev.force(v).unwrap(), Word::FALSE);
        assert!(ev.trace().is_cacheable());
    }

    #[test]
    fn opt_in_disables_the_tracing_cache_before_any_lookup() {
        let ev = evaluator(Some(Path::new("/unused")));
        assert!(!ev.trace().is_cacheable());
    }

    #[test]
    fn helper_must_be_an_absolute_path() {
        let mut ev = evaluator(Some(Path::new("untrusted-path-lookup")));
        let error = ev
            .eval_string("builtins.resolveGoPackages {}", b"/")
            .and_then(|v| ev.force(v))
            .unwrap_err();
        assert!(
            error.msg.contains("absolute executable path"),
            "{}",
            error.msg
        );
    }

    #[test]
    fn helper_reads_only_the_filtered_virtual_source() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("hidden"), "not in the source").unwrap();
        fs::write(
            dir.path().join("result.json"),
            r#"{"modulePath":"example.test/fixture","packages":{}}"#,
        )
        .unwrap();
        let helper = helper(
            dir.path(),
            r#"input=$(cat)
src=$(printf '%s' "$input" | sed 's/.*"src":"\([^"]*\)".*/\1/')
test ! -e "$src/hidden"
cat "$src/result.json""#,
        );
        let mut ev = evaluator(Some(&helper));
        let src = format!(
            r#"(builtins.resolveGoPackages {{
                src = builtins.path {{
                    path = {};
                    filter = p: type: type == "directory" || builtins.baseNameOf p == "result.json";
                }};
            }}).modulePath"#,
            sys::internal_string(dir.path())
        );
        let v = ev.eval_string(&src, b"/").unwrap();
        assert_eq!(
            ev.force_string_no_ctx(v, Pos::NONE, "").unwrap(),
            b"example.test/fixture"
        );
    }

    #[test]
    fn rejects_derivation_arguments_before_running_helper() {
        let dir = tempfile::tempdir().unwrap();
        let helper = helper(dir.path(), "echo should-not-run >&2; exit 3");
        let mut ev = evaluator(Some(&helper));
        let v = ev
            .eval_string(
                r#"builtins.resolveGoPackages {
                    src = derivation { name = "source"; system = "test"; builder = "/not-run"; };
                }"#,
                b"/",
            )
            .and_then(|v| ev.force(v));
        let error = v.unwrap_err();
        assert!(
            error.msg.contains("derivation-backed arguments"),
            "{}",
            error.msg
        );
    }

    #[test]
    fn helper_failures_are_evaluation_errors() {
        let dir = tempfile::tempdir().unwrap();
        let helper = helper(dir.path(), "echo resolver-failed >&2; exit 7");
        let mut ev = evaluator(Some(&helper));
        let src = format!(
            "builtins.resolveGoPackages {{ src = {}; }}",
            sys::internal_string(dir.path())
        );
        let error = ev
            .eval_string(&src, b"/")
            .and_then(|v| ev.force(v))
            .unwrap_err();
        assert!(error.msg.contains("resolver-failed"), "{}", error.msg);
        assert!(error.msg.contains('7'), "{}", error.msg);
    }

    #[test]
    fn rejects_non_object_results() {
        let dir = tempfile::tempdir().unwrap();
        let helper = helper(dir.path(), "printf '[]'");
        let mut ev = evaluator(Some(&helper));
        let src = format!(
            "builtins.resolveGoPackages {{ src = {}; }}",
            sys::internal_string(dir.path())
        );
        let error = ev
            .eval_string(&src, b"/")
            .and_then(|v| ev.force(v))
            .unwrap_err();
        assert!(error.msg.contains("JSON object"), "{}", error.msg);
    }

    #[test]
    fn preserves_pure_source_access_checks() {
        let dir = tempfile::tempdir().unwrap();
        let helper = helper(dir.path(), "echo should-not-run >&2; exit 3");
        let env = Env::isolated().with([("IETS_GO2NIX_RESOLVER", helper.as_os_str())]);
        let mut ev = EvalConfig::new()
            .session(Session::with_env(env))
            .pure(true)
            .build();
        let expr = format!(
            "builtins.resolveGoPackages {{ src = \"{}\"; }}",
            sys::internal_string(dir.path())
        );
        let error = ev
            .eval_string(&expr, b"/")
            .and_then(|v| ev.force(v))
            .unwrap_err();
        assert!(error.msg.contains("forbidden"), "{}", error.msg);
    }
}
