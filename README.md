# agent-core

Rust library for one-shot Claude, Codex, Devin, and OpenCode CLI runs. It uses
the CLIs' existing authentication and model configuration. Only OpenCode major
version 2 is supported; the other adapters use their native CLI commands.

```rust
use agent_core::{AgentCli, Provider, RunRequest};
use std::time::Duration;

let agent = AgentCli::discover(Provider::OpenCodeV2)?;
let result = agent.run(RunRequest {
    prompt: "Explain this project".into(),
    cwd: std::env::current_dir()?,
    model: None, // OpenCode's configured default
    timeout: Duration::from_secs(300),
    env_remove: vec![],
    json_schema: None,
}).await?;
println!("{}", result.text);
```

`Provider` also accepts `Claude`, `Codex`, and `Devin`. Model strings can use
`claude` or `codex` effort suffixes such as `sonnet@high` and
`gpt-5.6-sol@high`; OpenCode models use `provider/model#variant`. Claude and
Codex enforce an optional JSON Schema; Devin and OpenCode leave schema
validation to the caller.

The adapters use `claude -p --output-format json`, `codex exec --json`,
`devin -p --prompt-file`, and `opencode run --standalone --format json`.
Each call returns a single completed result. Pending permissions for OpenCode
follow its headless behavior; the library does not pass `--auto`.

Install and authenticate the desired CLI separately. See the [OpenCode v2 CLI
documentation](https://opencode.ai/v2/docs/cli/commands/) for OpenCode setup.

License: MIT or Apache-2.0.
