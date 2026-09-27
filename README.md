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
agent-core = { git = "https://github.com/tidusvn05/agent-core", tag = "v0.2.0" }
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
