# agent-core

Rust library for one-shot OpenCode v2 CLI runs. It uses your existing OpenCode
authentication and model configuration. Only OpenCode major version 2 is
supported.

```rust
use agent_core::{OpenCodeV2, RunRequest};
use std::time::Duration;

let agent = OpenCodeV2::discover()?;
let result = agent.run(RunRequest {
    prompt: "Explain this project".into(),
    cwd: std::env::current_dir()?,
    model: None, // OpenCode's configured default
    timeout: Duration::from_secs(300),
    env_remove: vec![],
}).await?;
println!("{}", result.text);
```

The library runs `opencode run --standalone --format json`, passes the prompt
through stdin, and parses the resulting JSONL. It does not manage interactive
sessions or enforce a JSON Schema. Pending tool permissions follow OpenCode's
headless behavior; the library does not pass `--auto`.

Install and authenticate OpenCode v2 separately. See the [OpenCode v2 CLI
documentation](https://opencode.ai/v2/docs/cli/commands/).

License: MIT or Apache-2.0.
