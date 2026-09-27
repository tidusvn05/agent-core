# agent-core

`agent-core` is a Rust library for one-shot calls to four local agent CLIs:
Claude, Codex, Devin, and OpenCode v2. It uses each CLI's saved authentication
and starts a fresh process for every request. The caller supplies a prompt and
gets final text, elapsed time, and token usage when the CLI reports it.

## Requirements

Install and authenticate the CLI you want to use. `AgentCli::discover` finds
it on `PATH`. OpenCode must report major version 2; version 1 is rejected.
No provider API key is required by the library itself. Use `env_remove` if your
application needs to remove environment variables before spawning the CLI.

## Add the dependency

```toml
[dependencies]
agent-core = { git = "https://github.com/tidusvn05/agent-core", tag = "v0.3.0" }
tokio = { version = "1", features = ["macros", "rt"] }
```

## Run a prompt

```rust
use agent_core::{AgentCli, Provider, RunRequest};
use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent = AgentCli::discover(Provider::Claude)?;
    let result = agent.run(RunRequest {
        prompt: "Summarize this repository in three sentences.".into(),
        cwd: std::env::current_dir()?,
        model: None, // use the CLI's configured default
        timeout: Duration::from_secs(300),
        env_remove: vec![],
        json_schema: None,
    }).await?;

    println!("{}", result.text);
    Ok(())
}
```

Change `Provider::Claude` to select another CLI. The repository also includes
a runnable [four-provider example](examples/run.rs):

```sh
cargo run --example run -- claude
cargo run --example run -- codex
cargo run --example run -- devin
cargo run --example run -- opencode
```

Run those commands from this repository after authenticating the selected CLI.
The example accepts an optional model as a second argument.

## Provider examples

The following calls use the same `RunRequest` shape shown above. Set `model`
to `None` to inherit the selected CLI's configured default.

| Provider | Rust selection | Example `model` | Native command |
|---|---|---|---|
| Claude | `Provider::Claude` | `Some("sonnet@high".into())` | `claude -p --output-format json` |
| Codex | `Provider::Codex` | `Some("gpt-5.6-sol@high".into())` | `codex exec --json` |
| Devin | `Provider::Devin` | `Some("swe-2-medium".into())` | `devin -p --prompt-file <file>` |
| OpenCode v2 | `Provider::OpenCodeV2` | `Some("openai/gpt-5#high".into())` | `opencode run --standalone --format json` |

To try an explicit model for each provider:

```sh
cargo run --example run -- claude sonnet@high
cargo run --example run -- codex gpt-5.6-sol@high
cargo run --example run -- devin swe-2-medium
cargo run --example run -- opencode openai/gpt-5#high
```

Choose model IDs available in your authenticated CLI configuration.

## Structured errors

Use `discover_with_context`, `with_binary_with_context`, and
`run_with_context` when the caller needs the selected provider alongside a
failure. `ProviderError` exposes `provider` and the original `source: Error`;
`exit_code()` and `stderr_tail()` read the fields of an `Error::Exit` without
parsing its display text. The original methods remain available.

## Real CLI smoke tests

The opt-in smoke tests make one short request per provider. Set the model to
one supported by your authenticated CLI, then run a single test:

```sh
AGENT_CORE_CODEX_MODEL='gpt-5.6-sol@high' \
  cargo test --test real_cli_smoke codex -- --ignored
```

Replace `CODEX` and `codex` with `CLAUDE`/`claude`, `DEVIN`/`devin`, or
`OPENCODE`/`opencode`. These calls use real quota and have a 180-second timeout.
Claude and Codex receive a native JSON Schema; Devin and OpenCode output is
checked as JSON by the test because their adapters do not enforce schemas.
The regular `cargo test` suite uses fake CLIs and does not make real calls.

## Releases

Release tags use `vMAJOR.MINOR.PATCH` and must match the version in
`Cargo.toml`. After updating `Cargo.toml`, `Cargo.lock`, and the README's
dependency example, push the commit and its tag:

```sh
git tag -a v0.4.0 -m 'agent-core v0.4.0'
git push origin main v0.4.0
```

The [release workflow](.github/workflows/release.yml) tests and packages the
tagged crate, then creates a GitHub Release. Its body and attached
`CHANGELOG.md` list every commit since the previous version tag, including
commits made outside pull requests. To publish an existing tag, run the Release
workflow manually and enter the tag. Publishing the same tag again is safe.

Claude and Codex accept an optional `json_schema` and enforce it through their
native CLI flags. Devin and OpenCode return text for the caller to validate.
`RunResult::usage` is `None` when usage is unavailable or incomplete. For the
exact command and output contracts, see [SPEC.md](SPEC.md).

## Execution behavior

Each request starts a new CLI process. A timeout or cancellation terminates
that process; the library does not retry automatically. Claude uses
`bypassPermissions`, Codex uses a read-only sandbox, Devin uses its `auto`
permission mode, and OpenCode follows its headless permission defaults without
`--auto`.

License: [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
