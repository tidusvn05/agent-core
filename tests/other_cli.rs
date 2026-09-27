#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use agent_core::{AgentCli, Error, Provider, RunRequest};

fn fake_cli(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo 1.0.0; exit 0; fi\n{body}\n"),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    // Some filesystems briefly report ETXTBSY immediately after creating an
    // executable shell script while tests run in parallel.
    std::thread::sleep(Duration::from_millis(10));
    path
}

fn request(dir: &Path) -> RunRequest {
    RunRequest {
        prompt: "hello\nworld".into(),
        cwd: dir.into(),
        model: None,
        timeout: Duration::from_secs(2),
        env_remove: vec![],
        json_schema: None,
    }
}

#[tokio::test]
async fn claude_parses_structured_output_and_usage() {
    let dir = tempfile::tempdir().unwrap();
    let binary = fake_cli(
        dir.path(),
        "claude",
        r#"
[ "$1" = -p ] || exit 2
[ "$(cat)" = 'hello
world' ] || exit 3
printf '%s\n' '{"is_error":false,"structured_output":{"ok":true},"session_id":"s1","usage":{"input_tokens":2,"output_tokens":3,"cache_read_input_tokens":4}}'
"#,
    );
    let cli = AgentCli::with_binary(Provider::Claude, binary).unwrap();
    let result = cli.run(request(dir.path())).await.unwrap();
    assert_eq!(result.text, "{\"ok\":true}");
    assert_eq!(result.session_id.as_deref(), Some("s1"));
    assert_eq!(result.usage.unwrap().cache_read, 4);
}

#[tokio::test]
async fn codex_reads_final_file_and_usage() {
    let dir = tempfile::tempdir().unwrap();
    let binary = fake_cli(
        dir.path(),
        "codex",
        r#"
[ "$1" = exec ] || exit 2
out=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = -o ]; then shift; out="$1"; fi
  shift
done
[ "$(cat)" = 'hello
world' ] || exit 3
printf 'final answer' > "$out"
printf '%s\n' '{"type":"thread.started","thread_id":"thread1"}' '{"type":"turn.completed","usage":{"input_tokens":8,"output_tokens":5,"reasoning_output_tokens":2}}'
"#,
    );
    let cli = AgentCli::with_binary(Provider::Codex, binary).unwrap();
    let result = cli.run(request(dir.path())).await.unwrap();
    assert_eq!(result.text, "final answer");
    assert_eq!(result.session_id.as_deref(), Some("thread1"));
    assert_eq!(result.usage.unwrap().reasoning, 2);
}

#[tokio::test]
async fn devin_reads_prompt_file() {
    let dir = tempfile::tempdir().unwrap();
    let binary = fake_cli(
        dir.path(),
        "devin",
        r#"
[ "$1" = -p ] && [ "$2" = --prompt-file ] || exit 2
[ "$(cat "$3")" = 'hello
world' ] || exit 3
echo 'Devin answer'
"#,
    );
    let cli = AgentCli::with_binary(Provider::Devin, binary).unwrap();
    let result = cli.run(request(dir.path())).await.unwrap();
    assert_eq!(result.text, "Devin answer");
    assert!(result.usage.is_none());
}

#[tokio::test]
async fn claude_surfaces_error_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let binary = fake_cli(
        dir.path(),
        "claude-error",
        "cat >/dev/null\necho '{\"is_error\":true,\"result\":\"bad model\"}'",
    );
    let cli = AgentCli::with_binary(Provider::Claude, binary).unwrap();
    assert!(matches!(cli.run(request(dir.path())).await, Err(Error::Agent(s)) if s == "bad model"));
}

#[tokio::test]
async fn claude_passes_model_effort_and_clean_schema() {
    let dir = tempfile::tempdir().unwrap();
    let binary = fake_cli(
        dir.path(),
        "claude-schema",
        r#"
schema=''; model=''; effort=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --json-schema) shift; schema="$1" ;;
    --model) shift; model="$1" ;;
    --effort) shift; effort="$1" ;;
  esac
  shift
done
[ "$model" = sonnet ] && [ "$effort" = high ] || exit 2
case "$schema" in *'$schema'*) exit 3;; esac
[ -n "$schema" ] || exit 4
cat >/dev/null
echo '{"result":"ok"}'
"#,
    );
    let cli = AgentCli::with_binary(Provider::Claude, binary).unwrap();
    let mut req = request(dir.path());
    req.model = Some("sonnet@high".into());
    req.json_schema = Some(serde_json::json!({"$schema":"draft","type":"object"}));
    assert_eq!(cli.run(req).await.unwrap().text, "ok");
}

#[tokio::test]
async fn codex_normalizes_schema_and_rejects_bad_effort() {
    let dir = tempfile::tempdir().unwrap();
    let binary = fake_cli(
        dir.path(),
        "codex-schema",
        r#"
out=''; schema=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) shift; out="$1" ;;
    --output-schema) shift; schema="$1" ;;
  esac
  shift
done
[ -n "$schema" ] || exit 2
grep -q '"additionalProperties":false' "$schema" || exit 3
grep -q '"required":\["maybe"\]' "$schema" || exit 4
cat >/dev/null
echo ok > "$out"
echo '{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}'
"#,
    );
    let cli = AgentCli::with_binary(Provider::Codex, binary).unwrap();
    let mut req = request(dir.path());
    req.model = Some("gpt-5.6-sol@turbo".into());
    assert!(matches!(cli.run(req).await, Err(Error::InvalidRequest(_))));
    let mut req = request(dir.path());
    req.json_schema = Some(serde_json::json!({
        "type":"object", "properties":{"maybe":{"type":"string"}}
    }));
    assert_eq!(cli.run(req).await.unwrap().text, "ok");
}

#[tokio::test]
async fn contextual_error_preserves_provider_exit_code_and_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let binary = fake_cli(dir.path(), "devin-error", "echo 'bad model' >&2\nexit 17");
    let cli = AgentCli::with_binary(Provider::Devin, binary).unwrap();
    let error = cli.run_with_context(request(dir.path())).await.unwrap_err();
    assert_eq!(error.provider, Provider::Devin);
    assert_eq!(error.exit_code(), Some(17));
    assert_eq!(error.stderr_tail(), Some("bad model\n"));
    assert!(matches!(
        error.source,
        Error::Exit {
            status: Some(17),
            ..
        }
    ));
}

#[tokio::test]
async fn contextual_discovery_error_preserves_provider() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing-codex");
    let error = AgentCli::with_binary_with_context(Provider::Codex, missing).unwrap_err();
    assert_eq!(error.provider, Provider::Codex);
    assert!(matches!(error.source, Error::NotFound(_)));
}

#[tokio::test]
async fn other_adapters_enforce_timeout() {
    let dir = tempfile::tempdir().unwrap();
    for provider in [Provider::Claude, Provider::Codex, Provider::Devin] {
        let binary = fake_cli(dir.path(), provider.executable(), "exec sleep 5");
        let cli = AgentCli::with_binary(provider, binary).unwrap();
        let mut req = request(dir.path());
        req.timeout = Duration::from_millis(50);
        let error = cli.run_with_context(req).await.unwrap_err();
        assert_eq!(error.provider, provider);
        assert!(matches!(error.source, Error::Timeout(_)));
        assert_eq!(error.exit_code(), None);
        assert_eq!(error.stderr_tail(), None);
    }
}
