//! One-shot Rust adapter for the OpenCode v2 CLI.
//!
//! Each call runs `opencode run --standalone --format json` and returns the
//! completed assistant text. Authentication and model defaults come from the
//! installed OpenCode CLI unless the caller chooses a model explicitly.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

const STDERR_LIMIT: usize = 4096;

/// A single, fresh OpenCode run.
#[derive(Debug, Clone)]
pub struct RunRequest {
    pub prompt: String,
    pub cwd: PathBuf,
    /// OpenCode model ID, for example `openai/gpt-5#high`.
    pub model: Option<String>,
    pub timeout: Duration,
    /// Environment variable names to remove from the child only.
    pub env_remove: Vec<OsString>,
}

/// Token accounting reported by completed OpenCode steps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// Result of one OpenCode run.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub text: String,
    pub session_id: Option<String>,
    /// `None` when the CLI stream did not contain complete usage data.
    pub usage: Option<TokenUsage>,
    pub duration: Duration,
    pub stderr_tail: String,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("OpenCode executable not found: {0}")]
    NotFound(PathBuf),
    #[error("failed to probe OpenCode version: {0}")]
    VersionProbe(String),
    #[error("OpenCode v2 required, found: {0}")]
    UnsupportedVersion(String),
    #[error("invalid run request: {0}")]
    InvalidRequest(String),
    #[error("failed to run OpenCode: {0}")]
    Io(#[from] std::io::Error),
    #[error("OpenCode timed out after {0:?}")]
    Timeout(Duration),
    #[error("OpenCode exited with {status:?}: {stderr_tail}")]
    Exit {
        status: Option<i32>,
        stderr_tail: String,
    },
    #[error("OpenCode reported an error: {0}")]
    Agent(String),
    #[error("invalid OpenCode JSONL: {0}")]
    Protocol(String),
    #[error("OpenCode returned no assistant text")]
    EmptyResponse,
}

/// A verified OpenCode v2 executable.
#[derive(Debug, Clone)]
pub struct OpenCodeV2 {
    binary: PathBuf,
}

impl OpenCodeV2 {
    /// Find `opencode` on `PATH` and verify that its major version is 2.
    pub fn discover() -> Result<Self, Error> {
        Self::with_binary(PathBuf::from("opencode"))
    }

    /// Use an explicit executable path, useful for isolated deployments and tests.
    pub fn with_binary(binary: impl Into<PathBuf>) -> Result<Self, Error> {
        let binary = binary.into();
        let output = std::process::Command::new(&binary)
            .arg("--version")
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::NotFound(binary.clone())
                } else {
                    Error::VersionProbe(e.to_string())
                }
            })?;
        if !output.status.success() {
            return Err(Error::VersionProbe(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let first = version.trim_start_matches('v');
        if !first.starts_with("2.") {
            return Err(Error::UnsupportedVersion(version));
        }
        Ok(Self { binary })
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    /// Run a fresh prompt. Dropping the future terminates its child process.
    pub async fn run(&self, request: RunRequest) -> Result<RunResult, Error> {
        if request.prompt.trim().is_empty() {
            return Err(Error::InvalidRequest("prompt is empty".into()));
        }
        if !request.cwd.is_dir() {
            return Err(Error::InvalidRequest(format!(
                "working directory does not exist: {}",
                request.cwd.display()
            )));
        }
        let started = Instant::now();
        let mut command = Command::new(&self.binary);
        command.args(["run", "--standalone", "--format", "json"]);
        if let Some(model) = request.model.as_deref() {
            if model.trim().is_empty() || !model.contains('/') {
                return Err(Error::InvalidRequest(format!(
                    "model must be provider/model[#variant]: {model}"
                )));
            }
            command.arg("--model").arg(model);
        }
        for name in &request.env_remove {
            command.env_remove(name);
        }
        command
            .current_dir(&request.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let work = async {
            let input = async {
                stdin.write_all(request.prompt.as_bytes()).await?;
                stdin.shutdown().await?;
                drop(stdin);
                Ok::<(), std::io::Error>(())
            };
            let (input, parsed, stderr_tail, status) = tokio::join!(
                input,
                parse_stdout(stdout),
                read_stderr_tail(stderr),
                child.wait()
            );
            input?;
            let parsed = parsed?;
            let stderr_tail = stderr_tail?;
            let status = status?;
            if !status.success() {
                return Err(Error::Exit {
                    status: status.code(),
                    stderr_tail,
                });
            }
            if let Some(error) = parsed.error {
                return Err(Error::Agent(error));
            }
            let text = parsed.text_parts.join("\n").trim().to_string();
            if text.is_empty() {
                return Err(Error::EmptyResponse);
            }
            let usage = parsed.usage();
            Ok(RunResult {
                text,
                session_id: parsed.session_id,
                usage,
                duration: started.elapsed(),
                stderr_tail,
            })
        };
        match tokio::time::timeout(request.timeout, work).await {
            Ok(result) => result,
            Err(_) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                Err(Error::Timeout(request.timeout))
            }
        }
    }
}

#[derive(Default)]
struct Parsed {
    text_parts: Vec<String>,
    session_id: Option<String>,
    usage_sum: TokenUsage,
    starts: usize,
    finishes: usize,
    last_reason: Option<String>,
    error: Option<String>,
}

impl Parsed {
    fn usage(&self) -> Option<TokenUsage> {
        (self.finishes > 0
            && self.starts == self.finishes
            && self.last_reason.as_deref() != Some("tool-calls"))
        .then(|| self.usage_sum.clone())
    }
}

async fn parse_stdout(stdout: impl AsyncRead + Unpin) -> Result<Parsed, Error> {
    let mut lines = BufReader::new(stdout).lines();
    let mut parsed = Parsed::default();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let event: Value = serde_json::from_str(&line)
            .map_err(|e| Error::Protocol(format!("{e}; line: {}", tail(&line, 200))))?;
        if let Some(id) = event["sessionID"].as_str() {
            parsed.session_id = Some(id.to_string());
        }
        let part = &event["part"];
        match event["type"].as_str() {
            Some("text") => {
                if let Some(text) = part["text"].as_str()
                    && !text.trim().is_empty()
                {
                    parsed.text_parts.push(text.to_string());
                }
            }
            Some("step_start") => parsed.starts += 1,
            Some("step_finish") => {
                parsed.finishes += 1;
                parsed.last_reason = part["reason"].as_str().map(str::to_string);
                let tokens = &part["tokens"];
                parsed.usage_sum.input += tokens["input"].as_u64().unwrap_or(0);
                parsed.usage_sum.output += tokens["output"].as_u64().unwrap_or(0);
                parsed.usage_sum.reasoning += tokens["reasoning"].as_u64().unwrap_or(0);
                parsed.usage_sum.cache_read += tokens["cache"]["read"].as_u64().unwrap_or(0);
                parsed.usage_sum.cache_write += tokens["cache"]["write"].as_u64().unwrap_or(0);
            }
            Some("error") => {
                let message = event["error"]["data"]["message"]
                    .as_str()
                    .or_else(|| event["error"]["message"].as_str())
                    .or_else(|| event["error"]["name"].as_str())
                    .or_else(|| event["error"].as_str())
                    .unwrap_or("unknown OpenCode error");
                parsed.error = Some(message.to_string());
            }
            _ => {}
        }
    }
    Ok(parsed)
}

async fn read_stderr_tail(mut stderr: impl AsyncRead + Unpin) -> Result<String, std::io::Error> {
    let mut tail_bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let n = stderr.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        tail_bytes.extend_from_slice(&chunk[..n]);
        if tail_bytes.len() > STDERR_LIMIT {
            tail_bytes.drain(..tail_bytes.len() - STDERR_LIMIT);
        }
    }
    Ok(String::from_utf8_lossy(&tail_bytes).to_string())
}

fn tail(s: &str, count: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(count)..].iter().collect()
}
