use clap::{Args, Parser, Subcommand};
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "jason", version, about = "Hosted Jason client for MAP")]
struct Cli {
    #[arg(long, global = true)]
    login_state: Option<PathBuf>,

    #[arg(long, global = true)]
    controller_endpoint: Option<String>,

    #[arg(long, global = true)]
    controller_token: Option<String>,

    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Doctor,
    Run(RunArgs),
    Status(StatusArgs),
    Watch(WatchArgs),
    Logs(IdArgs),
    Artifacts(IdArgs),
    Cancel(IdArgs),
    Version,
}

#[derive(Args)]
struct RunArgs {
    #[arg(long)]
    repo: String,

    #[arg(long)]
    issue: Option<String>,

    #[arg(long)]
    prompt: Option<String>,

    #[arg(long)]
    evidence_ref: Option<String>,
}

#[derive(Args)]
struct StatusArgs {
    run_id: Option<String>,
}

#[derive(Args)]
struct WatchArgs {
    run_id: String,

    #[arg(long, default_value_t = 5)]
    interval_seconds: u64,

    #[arg(long, default_value_t = 120)]
    timeout_seconds: u64,
}

#[derive(Args)]
struct IdArgs {
    run_id: String,
}

#[derive(Debug, Deserialize)]
struct LoginState {
    jason_controller_endpoint: Option<String>,
    access_token: String,
    expires_at: Option<String>,
    audience: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
}

fn main() {
    let cli = Cli::parse();
    if let Err(error) = run(cli) {
        eprintln!("jason: {}", redact(&error));
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match &cli.command {
        Command::Doctor => {
            let state = resolve_state(&cli);
            let payload = match state {
                Ok(state) => json!({
                    "ok": true,
                    "schema_version": "jason.doctor.v1",
                    "controller_endpoint": state.endpoint,
                    "has_token": true,
                }),
                Err(error) => json!({
                    "ok": false,
                    "schema_version": "jason.doctor.v1",
                    "error": redact(&error),
                }),
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&payload).unwrap());
            } else if payload["ok"].as_bool() == Some(true) {
                println!("Jason hosted client is configured");
            } else {
                println!("Jason hosted client is not configured; run `map login`");
            }
            Ok(())
        }
        Command::Run(args) => {
            validate_run_args(args)?;
            post(&cli, "/v1/task", task_submit_payload(args))
        }
        Command::Status(args) => match &args.run_id {
            Some(run_id) => get(&cli, &format!("/v1/task/{run_id}")),
            None => get(&cli, "/v1/status"),
        },
        Command::Watch(args) => watch(&cli, args),
        Command::Logs(args) => get(&cli, &format!("/v1/task/{}/logs", args.run_id)),
        Command::Artifacts(args) => get(&cli, &format!("/v1/task/{}/artifacts", args.run_id)),
        Command::Cancel(args) => post(&cli, &format!("/v1/task/{}/cancel", args.run_id), json!({})),
        Command::Version => print_json_or_text(
            cli.json,
            json!({ "name": "jason-client", "binary": "jason", "version": VERSION }),
            VERSION,
        ),
    }
}

fn validate_run_args(args: &RunArgs) -> Result<(), String> {
    if args.issue.is_none() && args.prompt.is_none() {
        return Err("run requires --issue or --prompt".to_string());
    }
    Ok(())
}

fn task_submit_payload(args: &RunArgs) -> Value {
    let prompt_ref = args
        .issue
        .as_deref()
        .map(|issue| issue_prompt_ref(&args.repo, issue))
        .unwrap_or_else(|| format!("prompt://{}", safe_repo_segment(&args.repo)));
    let evidence_refs = args
        .evidence_ref
        .as_ref()
        .map(|value| vec![value.clone()])
        .unwrap_or_default();
    json!({
        "prompt_ref": prompt_ref,
        "prompt_text": args.prompt,
        "capabilities": [],
        "priority": 0,
        "evidence_refs": evidence_refs,
    })
}

fn issue_prompt_ref(repo: &str, issue: &str) -> String {
    let trimmed = issue.trim();
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("github://")
        || trimmed.starts_with("issue://")
        || trimmed.starts_with("opaque://")
    {
        return trimmed.to_string();
    }
    format!("github://{}/issues/{}", repo.trim(), trimmed)
}

fn safe_repo_segment(repo: &str) -> String {
    repo.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

struct ResolvedState {
    endpoint: String,
    token: String,
}

fn resolve_state(cli: &Cli) -> Result<ResolvedState, String> {
    if let (Some(endpoint), Some(token)) = (&cli.controller_endpoint, &cli.controller_token) {
        return Ok(ResolvedState {
            endpoint: endpoint.clone(),
            token: token.clone(),
        });
    }

    let path = login_state_path(cli.login_state.as_ref())?;
    let text = fs::read_to_string(&path).map_err(|error| {
        format!(
            "read login state {}: {error}; run `map login`",
            path.display()
        )
    })?;
    let state: LoginState = serde_json::from_str(&text)
        .map_err(|error| format!("parse {}: {error}", path.display()))?;
    if !audience_allowed(&state) {
        return Err("login state is not authorized for jason-controller".to_string());
    }
    let endpoint = state
        .jason_controller_endpoint
        .ok_or_else(|| "login state has no jason_controller_endpoint".to_string())?;
    if let Some(expires_at) = &state.expires_at {
        if expires_at.trim().is_empty() {
            return Err("login state has an empty expires_at".to_string());
        }
        if expires_at_is_expired(expires_at) {
            return Err("login state is expired; run `map login`".to_string());
        }
    }
    Ok(ResolvedState {
        endpoint,
        token: state.access_token,
    })
}

fn expires_at_is_expired(expires_at: &str) -> bool {
    let Ok(epoch_seconds) = expires_at.trim().parse::<u64>() else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    epoch_seconds <= now
}

fn login_state_path(override_path: Option<&PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = override_path {
        return Ok(path.clone());
    }
    if let Ok(path) = env::var("JASON_LOGIN_STATE") {
        return Ok(PathBuf::from(path));
    }
    if let Ok(path) = env::var("MITHRAN_LOGIN_STATE") {
        return Ok(PathBuf::from(path));
    }
    if let Ok(path) = env::var("AEGIS_LOGIN_STATE") {
        return Ok(PathBuf::from(path));
    }
    let config_home = env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|_| env::var("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map_err(|_| "HOME or XDG_CONFIG_HOME is required".to_string())?;
    let mithran = config_home.join("mithran").join("login.json");
    if mithran.exists() {
        return Ok(mithran);
    }
    Ok(config_home.join("aegis").join("login.json"))
}

fn audience_allowed(state: &LoginState) -> bool {
    state.audience.as_deref() == Some("jason-controller")
        || state.scopes.iter().any(|scope| scope == "jason:*")
        || state.scopes.iter().any(|scope| scope == "jason:controller")
        || state
            .scopes
            .iter()
            .any(|scope| scope == "audience:jason-controller")
}

fn client(cli: &Cli) -> Result<(Client, ResolvedState), String> {
    let state = resolve_state(cli)?;
    let client = Client::builder()
        .build()
        .map_err(|error| format!("build HTTP client: {error}"))?;
    Ok((client, state))
}

fn get(cli: &Cli, path: &str) -> Result<(), String> {
    let (client, state) = client(cli)?;
    let response = client
        .get(format!("{}{}", state.endpoint.trim_end_matches('/'), path))
        .bearer_auth(&state.token)
        .send()
        .map_err(|error| format!("Jason request failed: {error}"))?;
    print_response(cli.json, response)
}

fn post(cli: &Cli, path: &str, body: Value) -> Result<(), String> {
    let (client, state) = client(cli)?;
    let response = client
        .post(format!("{}{}", state.endpoint.trim_end_matches('/'), path))
        .bearer_auth(&state.token)
        .json(&body)
        .send()
        .map_err(|error| format!("Jason request failed: {error}"))?;
    print_response(cli.json, response)
}

fn watch(cli: &Cli, args: &WatchArgs) -> Result<(), String> {
    let mut elapsed = 0;
    loop {
        let (client, state) = client(cli)?;
        let response = client
            .get(format!(
                "{}/v1/task/{}",
                state.endpoint.trim_end_matches('/'),
                args.run_id
            ))
            .bearer_auth(&state.token)
            .send()
            .map_err(|error| format!("Jason watch failed: {error}"))?;
        let value: Value = response
            .json()
            .map_err(|error| format!("read Jason watch response: {error}"))?;
        if cli.json {
            println!("{}", serde_json::to_string(&value).unwrap());
        } else {
            println!("{}", task_state(&value).unwrap_or("unknown"));
        }
        if task_is_terminal(&value) {
            return Ok(());
        }
        if elapsed >= args.timeout_seconds {
            return Err("watch timed out".to_string());
        }
        thread::sleep(Duration::from_secs(args.interval_seconds));
        elapsed += args.interval_seconds;
    }
}

fn task_state(value: &Value) -> Option<&str> {
    value
        .pointer("/task/state")
        .and_then(Value::as_str)
        .or_else(|| value.get("state").and_then(Value::as_str))
        .or_else(|| value.get("status").and_then(Value::as_str))
}

fn task_is_terminal(value: &Value) -> bool {
    matches!(
        task_state(value),
        Some("completed" | "failed" | "blocked" | "cancelled" | "declined" | "succeeded")
    )
}

fn print_response(json_output: bool, response: reqwest::blocking::Response) -> Result<(), String> {
    let status = response.status();
    let text = response
        .text()
        .map_err(|error| format!("read Jason response: {error}"))?;
    if status != StatusCode::OK && status != StatusCode::CREATED && status != StatusCode::ACCEPTED {
        return Err(format!("Jason returned {status}: {}", redact(&text)));
    }
    if json_output {
        println!("{text}");
    } else {
        println!("ok");
    }
    Ok(())
}

fn print_json_or_text(json_output: bool, payload: Value, text: &str) -> Result<(), String> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
    } else {
        println!("{text}");
    }
    Ok(())
}

fn redact(text: &str) -> String {
    let mut redacted = text.to_string();
    for marker in ["access_token", "Authorization", "Bearer"] {
        if redacted.contains(marker) {
            redacted = redacted.replace(marker, "[REDACTED]");
        }
    }
    if let Ok(home) = env::var("HOME") {
        let home = home.trim_end_matches('/');
        if !home.is_empty() && redacted.contains(home) {
            redacted = redacted.replace(home, "[HOME]");
        }
    }
    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audience_accepts_jason_scope() {
        let state = LoginState {
            jason_controller_endpoint: Some("https://jason.example".to_string()),
            access_token: "secret".to_string(),
            expires_at: None,
            audience: Some("map-control".to_string()),
            scopes: vec!["jason:controller".to_string()],
        };
        assert!(audience_allowed(&state));
    }

    #[test]
    fn audience_rejects_unrelated_login() {
        let state = LoginState {
            jason_controller_endpoint: Some("https://jason.example".to_string()),
            access_token: "secret".to_string(),
            expires_at: None,
            audience: Some("map-control".to_string()),
            scopes: vec!["map:deploy".to_string()],
        };
        assert!(!audience_allowed(&state));
    }

    #[test]
    fn run_requires_issue_or_prompt() {
        let args = RunArgs {
            repo: "mithran-hq/demo".to_string(),
            issue: None,
            prompt: None,
            evidence_ref: None,
        };
        assert!(validate_run_args(&args).is_err());
    }

    #[test]
    fn redaction_removes_local_home_paths() {
        let home = env::var("HOME").expect("HOME should be set in tests");
        let text = format!("read login state {home}/.config/aegis/login.json");
        let redacted = redact(&text);

        assert!(!redacted.contains(&home));
        assert!(redacted.contains("[HOME]/.config/aegis/login.json"));
    }

    #[test]
    fn issue_run_payload_uses_controller_task_contract() {
        let args = RunArgs {
            repo: "mithran-hq/demo".to_string(),
            issue: Some("123".to_string()),
            prompt: None,
            evidence_ref: Some("evidence://demo/run".to_string()),
        };
        let payload = task_submit_payload(&args);
        assert_eq!(payload["prompt_ref"], "github://mithran-hq/demo/issues/123");
        assert_eq!(payload["evidence_refs"][0], "evidence://demo/run");
    }

    #[test]
    fn prompt_run_payload_uses_opaque_prompt_ref() {
        let args = RunArgs {
            repo: "mithran-hq/demo".to_string(),
            issue: None,
            prompt: Some("check status".to_string()),
            evidence_ref: None,
        };
        let payload = task_submit_payload(&args);
        assert_eq!(payload["prompt_ref"], "prompt://mithran-hq/demo");
        assert_eq!(payload["prompt_text"], "check status");
    }

    #[test]
    fn expired_numeric_login_state_is_rejected() {
        assert!(expires_at_is_expired("1"));
        assert!(!expires_at_is_expired("4102444800"));
        assert!(!expires_at_is_expired("2026-05-29T00:00:00Z"));
    }

    #[test]
    fn task_terminal_state_uses_controller_task_state() {
        assert!(task_is_terminal(&json!({"task": {"state": "completed"}})));
        assert!(!task_is_terminal(&json!({"task": {"state": "running"}})));
    }
}
