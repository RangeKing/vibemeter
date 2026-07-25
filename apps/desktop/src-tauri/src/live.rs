use crate::database::Database;
use crate::errors::{AppError, AppResult};
use crate::models::{HookProviderStatus, HookStatus, LiveAction, LiveSession, LiveSnapshot};
use crate::providers::{codex_binary, write_json_line};
use chrono::{DateTime, Duration, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::time::{Duration as StdDuration, Instant};
use tauri::{AppHandle, Emitter};

const RAW_RETENTION_DAYS: i64 = 90;
const MAX_HOOK_BYTES: u64 = 768 * 1024;
const MANAGED_MARKER: &str = "vibemeter_hook.py";
const CODEX_PROBE_TTL: StdDuration = StdDuration::from_secs(20);
const CLAUDE_HOOKS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("SessionStart", None, None),
    ("UserPromptSubmit", None, None),
    ("PreToolUse", Some("*"), None),
    ("PostToolUse", Some("*"), None),
    ("PostToolUseFailure", Some("*"), None),
    ("PermissionRequest", Some("*"), None),
    ("Notification", Some("permission_prompt|idle_prompt"), None),
    ("PreCompact", Some("auto|manual"), None),
    ("PostCompact", None, None),
    ("Stop", None, Some(30)),
    ("SubagentStart", None, None),
    ("SubagentStop", None, None),
    ("SessionEnd", None, None),
];
const CODEX_HOOKS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("PreToolUse", Some("*"), None),
    ("PermissionRequest", Some("*"), None),
    ("PostToolUse", Some("*"), None),
    ("PreCompact", None, None),
    ("PostCompact", None, None),
    ("SessionStart", None, None),
    ("SessionEnd", None, None),
    ("UserPromptSubmit", None, None),
    ("SubagentStart", None, None),
    ("SubagentStop", None, None),
    ("Stop", None, Some(30)),
];
const CODEX_RUNTIME_EVENTS: &[&str] = &[
    "preToolUse",
    "permissionRequest",
    "postToolUse",
    "preCompact",
    "postCompact",
    "sessionStart",
    "sessionEnd",
    "userPromptSubmit",
    "subagentStart",
    "subagentStop",
    "stop",
];
const CODEX_EVENT_NAMES: &[&str] = &[
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PreCompact",
    "PostCompact",
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "SubagentStart",
    "SubagentStop",
    "Stop",
];

#[derive(Clone, Debug)]
struct CodexHookHealth {
    working: bool,
    detail: &'static str,
}

#[derive(Default)]
struct CodexHookProbeCache {
    checked_at: Option<Instant>,
    health: Option<CodexHookHealth>,
}

static CODEX_HOOK_PROBE: Lazy<Mutex<CodexHookProbeCache>> =
    Lazy::new(|| Mutex::new(CodexHookProbeCache::default()));
const PYTHON_HOOK: &str = r#"#!/usr/bin/python3
import json
import os
import socket
import subprocess
import sys

MAX_BYTES = 768 * 1024

def process_context(provider):
    try:
        output = subprocess.check_output(
            ["/bin/ps", "-axo", "pid=,ppid=,tty=,comm="],
            text=True,
            timeout=0.6,
        )
    except Exception:
        return {}
    table = {}
    for line in output.splitlines():
        parts = line.strip().split(None, 3)
        if len(parts) != 4 or not parts[0].isdigit() or not parts[1].isdigit():
            continue
        table[int(parts[0])] = {
            "ppid": int(parts[1]),
            "tty": parts[2],
            "command": parts[3],
        }
    current = os.getppid()
    fallback = None
    for _ in range(12):
        info = table.get(current)
        if not info:
            break
        command = os.path.basename(info["command"]).lower()
        origin = "cli" if info["tty"] != "??" else "desktop"
        if fallback is None:
            fallback = (current, origin, info["tty"])
        if provider in command or (provider == "claude" and "claude-code" in command):
            return {"process_id": current, "origin": origin, "tty": info["tty"]}
        parent = info["ppid"]
        if parent <= 1 or parent == current:
            break
        current = parent
    if fallback:
        return {"process_id": fallback[0], "origin": fallback[1], "tty": fallback[2]}
    return {}

def main():
    provider = sys.argv[1] if len(sys.argv) > 1 else "unknown"
    raw = sys.stdin.buffer.read(MAX_BYTES)
    try:
        payload = json.loads(raw.decode("utf-8")) if raw else {}
    except Exception:
        return
    if not isinstance(payload, dict):
        return
    envelope = {
        "provider": provider,
        "received_at": __import__("datetime").datetime.now(
            __import__("datetime").timezone.utc
        ).isoformat(),
        "payload": payload,
    }
    envelope.update(process_context(provider))
    socket_path = os.path.expanduser("~/.vibemeter/vibemeter.sock")
    try:
        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        connection.settimeout(0.3)
        connection.connect(socket_path)
        connection.sendall(json.dumps(envelope, ensure_ascii=False).encode("utf-8"))
        connection.close()
    except Exception:
        pass

if __name__ == "__main__":
    main()
"#;

#[derive(Clone)]
pub struct LiveMonitor {
    sessions: Arc<RwLock<HashMap<String, LiveSession>>>,
    socket_ready: Arc<AtomicBool>,
}

impl LiveMonitor {
    pub fn start(database: Database, app: AppHandle) -> AppResult<Self> {
        database.purge_expired_live_events()?;
        let sessions = Arc::new(RwLock::new(HashMap::new()));
        let socket_ready = Arc::new(AtomicBool::new(false));
        let monitor = Self {
            sessions: sessions.clone(),
            socket_ready: socket_ready.clone(),
        };
        let socket_path = socket_path()?;
        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent)?;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        if let Ok(metadata) = fs::symlink_metadata(&socket_path) {
            if !metadata.file_type().is_socket() {
                return Err(AppError::InvalidRequest(format!(
                    "refusing to replace non-socket path {}",
                    socket_path.display()
                )));
            }
            fs::remove_file(&socket_path)?;
        }
        let listener = UnixListener::bind(&socket_path)?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
        socket_ready.store(true, Ordering::SeqCst);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let mut input = String::new();
                if stream
                    .take(MAX_HOOK_BYTES)
                    .read_to_string(&mut input)
                    .is_err()
                {
                    continue;
                }
                let Ok(envelope) = serde_json::from_str::<Value>(&input) else {
                    continue;
                };
                if let Some((session, raw, event_name)) = session_from_envelope(&envelope) {
                    let transitioned_session_id = session.id.clone();
                    let received = session.updated_at.clone();
                    let expires = DateTime::parse_from_rfc3339(&received)
                        .map(|value| {
                            (value.with_timezone(&Utc) + Duration::days(RAW_RETENTION_DAYS))
                                .to_rfc3339()
                        })
                        .unwrap_or_else(|_| {
                            (Utc::now() + Duration::days(RAW_RETENTION_DAYS)).to_rfc3339()
                        });
                    let _ = database.record_live_event(
                        &received,
                        &expires,
                        &session.agent,
                        &session.source_session_id,
                        &event_name,
                        &session.project_label,
                        &raw,
                        &session.status,
                    );
                    let transition = merge_session(&sessions, session);
                    let snapshot = snapshot_from(&sessions, socket_ready.load(Ordering::SeqCst));
                    let _ = app.emit("live-update", &snapshot);
                    if matches!(transition.as_deref(), Some("waiting" | "error"))
                        && let Some(active) = snapshot
                            .sessions
                            .iter()
                            .find(|item| item.id == transitioned_session_id)
                    {
                        notify_if_background(active, transition.as_deref().unwrap_or_default());
                    }
                }
            }
            socket_ready.store(false, Ordering::SeqCst);
        });
        Ok(monitor)
    }

    pub fn snapshot(&self) -> LiveSnapshot {
        prune_sessions(&self.sessions);
        snapshot_from(&self.sessions, self.socket_ready.load(Ordering::SeqCst))
    }

    pub fn session(&self, id: &str) -> Option<LiveSession> {
        self.sessions
            .read()
            .ok()
            .and_then(|sessions| sessions.get(id).cloned())
    }
}

pub fn install_hooks() -> AppResult<HookStatus> {
    let script = hook_script_path()?;
    if let Some(parent) = script.parent() {
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    write_if_changed(&script, PYTHON_HOOK.as_bytes(), Some(0o700), false)?;
    let home = dirs::home_dir()
        .ok_or_else(|| AppError::InvalidRequest("home directory is unavailable".into()))?;
    let command = format!("\"/usr/bin/python3\" \"{}\"", script.display());
    let claude_dir = home.join(".claude");
    if claude_dir.is_dir() {
        let settings = claude_dir.join("settings.json");
        upsert_hook_json(&settings, &format!("{command} claude"), CLAUDE_HOOKS)?;
    }
    let codex_dir = home.join(".codex");
    if codex_dir.is_dir() {
        let hooks = codex_dir.join("hooks.json");
        upsert_codex_hook_json(&hooks, &format!("{command} codex"), CODEX_HOOKS)?;
        enable_codex_hooks(&codex_dir.join("config.toml"))?;
        invalidate_codex_hook_probe();
    }
    Ok(hook_status(true))
}

pub fn uninstall_hooks() -> AppResult<HookStatus> {
    let home = dirs::home_dir()
        .ok_or_else(|| AppError::InvalidRequest("home directory is unavailable".into()))?;
    for path in [
        home.join(".claude/settings.json"),
        home.join(".codex/hooks.json"),
    ] {
        remove_managed_hooks(&path)?;
    }
    let script = hook_script_path()?;
    if script.exists() {
        fs::remove_file(script)?;
    }
    invalidate_codex_hook_probe();
    Ok(hook_status(socket_path()?.exists()))
}

pub fn hook_status(socket_ready: bool) -> HookStatus {
    let home = dirs::home_dir().unwrap_or_default();
    let claude_path = home.join(".claude/settings.json");
    let codex_hooks = home.join(".codex/hooks.json");
    let codex_config = home.join(".codex/config.toml");
    let claude_available = home.join(".claude").is_dir();
    let codex_available = home.join(".codex").is_dir();
    let claude_health = if claude_available {
        claude_hook_config_status(&claude_path)
    } else {
        CodexHookHealth {
            working: false,
            detail: "not-found",
        }
    };
    let codex_config_health = if codex_available {
        codex_hook_config_status(&codex_hooks)
    } else {
        CodexHookHealth {
            working: false,
            detail: "not-found",
        }
    };
    let codex_feature = fs::read_to_string(&codex_config)
        .ok()
        .is_some_and(|text| codex_hook_feature_enabled(&text));
    let codex_health = if !codex_available {
        CodexHookHealth {
            working: false,
            detail: "not-found",
        }
    } else if !codex_feature {
        CodexHookHealth {
            working: false,
            detail: "feature-disabled",
        }
    } else if !codex_config_health.working {
        codex_config_health
    } else {
        codex_hook_runtime_status()
    };
    let providers = vec![
        HookProviderStatus {
            provider: "claude-code".into(),
            available: claude_available,
            installed: claude_health.working,
            detail: claude_health.detail.into(),
        },
        HookProviderStatus {
            provider: "codex".into(),
            available: codex_available,
            installed: codex_health.working,
            detail: codex_health.detail.into(),
        },
    ];
    let installed_count = providers.iter().filter(|item| item.installed).count();
    HookStatus {
        state: if installed_count == providers.iter().filter(|item| item.available).count()
            && installed_count > 0
        {
            "ready"
        } else if installed_count > 0 {
            "partial"
        } else {
            "unavailable"
        }
        .into(),
        providers,
        socket_ready,
    }
}

fn claude_hook_config_status(path: &Path) -> CodexHookHealth {
    let Ok(root) = read_json_object(path) else {
        return CodexHookHealth {
            working: false,
            detail: "config-invalid",
        };
    };
    if root
        .get("disableAllHooks")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return CodexHookHealth {
            working: false,
            detail: "hooks-disabled",
        };
    }
    managed_hook_config_status(&root, CLAUDE_HOOKS)
}

fn codex_hook_config_status(path: &Path) -> CodexHookHealth {
    let Ok(root) = read_json_object(path) else {
        return CodexHookHealth {
            working: false,
            detail: "config-invalid",
        };
    };
    if CODEX_EVENT_NAMES
        .iter()
        .any(|event| root.contains_key(*event))
    {
        return CodexHookHealth {
            working: false,
            detail: "config-invalid",
        };
    }
    managed_hook_config_status(&root, CODEX_HOOKS)
}

fn managed_hook_config_status(
    root: &Map<String, Value>,
    expected: &[(&str, Option<&str>, Option<u64>)],
) -> CodexHookHealth {
    let Ok(events) = managed_hook_events(root) else {
        return CodexHookHealth {
            working: false,
            detail: "config-invalid",
        };
    };
    if expected.iter().all(|(event, _, _)| events.contains(*event)) {
        CodexHookHealth {
            working: true,
            detail: "ready",
        }
    } else {
        CodexHookHealth {
            working: false,
            detail: "hook-missing",
        }
    }
}

fn managed_hook_events(root: &Map<String, Value>) -> Result<HashSet<String>, ()> {
    let Some(hooks_value) = root.get("hooks") else {
        return Ok(HashSet::new());
    };
    let hooks = hooks_value.as_object().ok_or(())?;
    let mut events = HashSet::new();
    for (event, groups_value) in hooks {
        let groups = groups_value.as_array().ok_or(())?;
        for group in groups {
            let commands = group
                .as_object()
                .and_then(|group| group.get("hooks"))
                .and_then(Value::as_array)
                .ok_or(())?;
            if commands.iter().any(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains(MANAGED_MARKER))
            }) {
                events.insert(event.clone());
            }
        }
    }
    Ok(events)
}

fn invalidate_codex_hook_probe() {
    if let Ok(mut cache) = CODEX_HOOK_PROBE.lock() {
        *cache = CodexHookProbeCache::default();
    }
}

fn codex_hook_runtime_status() -> CodexHookHealth {
    if let Ok(cache) = CODEX_HOOK_PROBE.lock()
        && cache
            .checked_at
            .is_some_and(|checked_at| checked_at.elapsed() < CODEX_PROBE_TTL)
        && let Some(health) = cache.health.clone()
    {
        return health;
    }
    let health = probe_codex_hooks().unwrap_or(CodexHookHealth {
        working: false,
        detail: "status-unavailable",
    });
    if let Ok(mut cache) = CODEX_HOOK_PROBE.lock() {
        cache.checked_at = Some(Instant::now());
        cache.health = Some(health.clone());
    }
    health
}

fn probe_codex_hooks() -> AppResult<CodexHookHealth> {
    let binary = codex_binary()
        .ok_or_else(|| AppError::ProviderUnavailable("a working Codex CLI was not found".into()))?;
    let mut child = Command::new(binary)
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| AppError::ProviderUnavailable("Codex stdin is unavailable".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::ProviderUnavailable("Codex stdout is unavailable".into()))?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    write_json_line(
        &mut stdin,
        &json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {"name":"vibemeter","title":"VibeMeter","version":"0.1.0"},
                "capabilities": {"experimentalApi":true}
            }
        }),
    )?;

    let deadline = Instant::now() + StdDuration::from_secs(5);
    let mut result = None;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let line = match receiver.recv_timeout(remaining.min(StdDuration::from_millis(500))) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let Ok(payload) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match payload.get("id").and_then(Value::as_i64) {
            Some(1) => {
                write_json_line(&mut stdin, &json!({"method":"initialized","params":{}}))?;
                let cwd = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
                write_json_line(
                    &mut stdin,
                    &json!({"id":2,"method":"hooks/list","params":{"cwds":[cwd]}}),
                )?;
            }
            Some(2) => {
                result = payload.get("result").cloned();
                break;
            }
            _ => {}
        }
    }
    let _ = child.kill();
    let result =
        result.ok_or_else(|| AppError::ProviderUnavailable("Codex hook probe timed out".into()))?;
    Ok(codex_hook_health_from_list(&result))
}

fn codex_hook_health_from_list(result: &Value) -> CodexHookHealth {
    let Some(entry) = result
        .get("data")
        .and_then(Value::as_array)
        .and_then(|entries| entries.first())
    else {
        return CodexHookHealth {
            working: false,
            detail: "status-unavailable",
        };
    };
    let config_warning = entry
        .get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|warning| warning.contains(".codex/hooks.json"));
    let config_error = entry
        .get("errors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|error| {
            error
                .get("path")
                .and_then(Value::as_str)
                .is_some_and(|path| path.ends_with("hooks.json"))
        });
    if config_warning || config_error {
        return CodexHookHealth {
            working: false,
            detail: "config-invalid",
        };
    }
    let managed = entry
        .get("hooks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|hook| {
            hook.get("command")
                .and_then(Value::as_str)
                .is_some_and(|command| command.contains(MANAGED_MARKER))
        })
        .collect::<Vec<_>>();
    let events = managed
        .iter()
        .filter_map(|hook| hook.get("eventName").and_then(Value::as_str))
        .collect::<HashSet<_>>();
    if !CODEX_RUNTIME_EVENTS
        .iter()
        .all(|event| events.contains(event))
    {
        return CodexHookHealth {
            working: false,
            detail: "hook-missing",
        };
    }
    if managed
        .iter()
        .any(|hook| hook.get("trustStatus").and_then(Value::as_str) == Some("modified"))
    {
        return CodexHookHealth {
            working: false,
            detail: "hook-modified",
        };
    }
    if managed
        .iter()
        .any(|hook| hook.get("trustStatus").and_then(Value::as_str) == Some("untrusted"))
    {
        return CodexHookHealth {
            working: false,
            detail: "review-required",
        };
    }
    if managed
        .iter()
        .any(|hook| hook.get("enabled").and_then(Value::as_bool) != Some(true))
    {
        return CodexHookHealth {
            working: false,
            detail: "hooks-disabled",
        };
    }
    if managed.iter().all(|hook| {
        matches!(
            hook.get("trustStatus").and_then(Value::as_str),
            Some("trusted" | "managed")
        )
    }) {
        CodexHookHealth {
            working: true,
            detail: "ready",
        }
    } else {
        CodexHookHealth {
            working: false,
            detail: "status-unavailable",
        }
    }
}

pub fn jump_to_session(session: &LiveSession) -> AppResult<()> {
    if session.agent == "codex" && session.origin.as_deref() == Some("desktop") {
        let mut url = url::Url::parse("codex://threads/")
            .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
        url.path_segments_mut()
            .map_err(|_| AppError::InvalidRequest("invalid Codex URL".into()))?
            .push(&session.source_session_id);
        Command::new("/usr/bin/open")
            .arg(url.as_str())
            .spawn()
            .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
        return Ok(());
    }
    let Some(mut process_id) = session.process_id else {
        return Err(AppError::InvalidRequest(
            "source process is no longer available".into(),
        ));
    };
    for _ in 0..12 {
        let command = Command::new("/bin/ps")
            .args(["-p", &process_id.to_string(), "-o", "comm="])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
            .unwrap_or_default();
        let app_name = if command.contains("Terminal.app") {
            Some("Terminal")
        } else if command.contains("iTerm.app") {
            Some("iTerm")
        } else if command.contains("Warp.app") {
            Some("Warp")
        } else if command.contains("Codex.app") {
            Some("Codex")
        } else {
            None
        };
        if let Some(app_name) = app_name {
            Command::new("/usr/bin/open")
                .args(["-a", app_name])
                .spawn()
                .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
            return Ok(());
        }
        let parent = Command::new("/bin/ps")
            .args(["-p", &process_id.to_string(), "-o", "ppid="])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u32>()
                    .ok()
            });
        let Some(parent) = parent else { break };
        if parent <= 1 || parent == process_id {
            break;
        }
        process_id = parent;
    }
    Err(AppError::InvalidRequest(
        "the source terminal could not be identified".into(),
    ))
}

fn session_from_envelope(envelope: &Value) -> Option<(LiveSession, String, String)> {
    let object = envelope.as_object()?;
    let provider = object.get("provider")?.as_str()?;
    let agent = match provider {
        "claude" | "claude-code" => "claude-code",
        "codex" => "codex",
        _ => return None,
    };
    let payload = object.get("payload")?.as_object()?;
    let received_at = object
        .get("received_at")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    let source_session_id = string_field(payload, &["session_id", "sessionId", "thread_id"])
        .filter(|value| !value.is_empty())
        .or_else(|| {
            object
                .get("process_id")
                .and_then(Value::as_u64)
                .map(|pid| format!("process-{pid}"))
        })?;
    let event_name = string_field(payload, &["hook_event_name", "event", "type"])
        .unwrap_or_else(|| "Unknown".into());
    let cwd = string_field(payload, &["cwd", "working_directory"]).unwrap_or_default();
    let project_label = project_label_from_cwd(&cwd);
    let tool = string_field(payload, &["tool_name", "tool"]).unwrap_or_default();
    let notification_type =
        string_field(payload, &["notification_type", "notificationType"]).unwrap_or_default();
    let error = event_name == "PostToolUseFailure"
        || payload
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || payload
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "error" | "failed"));
    let status = if error {
        "error"
    } else if event_name == "PermissionRequest"
        || (event_name == "Notification"
            && matches!(
                notification_type.as_str(),
                "permission_prompt" | "idle_prompt"
            ))
    {
        "waiting"
    } else if matches!(event_name.as_str(), "Stop" | "SessionEnd") {
        "completed"
    } else if event_name == "SessionStart" {
        "idle"
    } else {
        "running"
    };
    let (phase, action_kind) = phase_for(&event_name, &tool, status);
    let waiting_reason = if status == "waiting" {
        Some(if tool.is_empty() {
            "Permission required".into()
        } else {
            format!("{tool} needs approval")
        })
    } else if status == "error" {
        Some("The latest action reported an error".into())
    } else {
        None
    };
    let id = crate::privacy::stable_hash(&format!("{agent}:{source_session_id}"));
    let session = LiveSession {
        id,
        source_session_id,
        agent: agent.into(),
        project_label,
        status: status.into(),
        phase: phase.into(),
        started_at: received_at.clone(),
        updated_at: received_at.clone(),
        waiting_reason,
        actions: vec![LiveAction {
            kind: action_kind.into(),
            label: if tool.is_empty() {
                event_name.clone()
            } else {
                tool
            },
            occurred_at: received_at,
        }],
        process_id: object
            .get("process_id")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        origin: object
            .get("origin")
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    let raw = serde_json::to_string(envelope).ok()?;
    Some((session, raw, event_name))
}

fn project_label_from_cwd(cwd: &str) -> String {
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return "Unknown project".into();
    }
    let path = Path::new(cwd);
    let root = path
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .or_else(|| {
            path.ancestors()
                .find(|ancestor| project_manifest_exists(ancestor))
        })
        .unwrap_or(path);
    project_name_from_manifest(root)
        .or_else(|| {
            root.file_name()
                .and_then(|value| value.to_str())
                .and_then(clean_project_name)
        })
        .unwrap_or_else(|| "Unknown project".into())
}

fn project_manifest_exists(root: &Path) -> bool {
    ["package.json", "Cargo.toml", "pyproject.toml"]
        .iter()
        .any(|name| root.join(name).is_file())
}

fn project_name_from_manifest(root: &Path) -> Option<String> {
    let package_json = root.join("package.json");
    if fs::metadata(&package_json)
        .ok()
        .is_some_and(|metadata| metadata.len() <= 512 * 1024)
        && let Ok(bytes) = fs::read(package_json)
        && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
        && let Some(name) = value.get("name").and_then(Value::as_str)
        && let Some(name) = clean_project_name(name)
    {
        return Some(name);
    }
    project_name_from_toml(&root.join("Cargo.toml"), "package")
        .or_else(|| project_name_from_toml(&root.join("pyproject.toml"), "project"))
}

fn project_name_from_toml(path: &Path, section: &str) -> Option<String> {
    if !fs::metadata(path)
        .ok()
        .is_some_and(|metadata| metadata.len() <= 512 * 1024)
    {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line
                .strip_prefix('[')
                .and_then(|line| line.strip_suffix(']'))
                == Some(section);
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "name" {
            continue;
        }
        let value = value.trim();
        let name = value
            .strip_prefix('"')
            .and_then(|value| value.split('"').next())
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.split('\'').next())
            })?;
        return clean_project_name(name);
    }
    None
}

fn clean_project_name(name: &str) -> Option<String> {
    let name = name.trim().rsplit('/').next()?.trim();
    (!name.is_empty()
        && name.len() <= 80
        && !name.contains('\\')
        && !name.chars().any(char::is_control))
    .then(|| name.to_string())
}

fn merge_session(
    sessions: &Arc<RwLock<HashMap<String, LiveSession>>>,
    incoming: LiveSession,
) -> Option<String> {
    let mut guard = sessions.write().ok()?;
    let previous_status = guard.get(&incoming.id).map(|item| item.status.clone());
    if let Some(existing) = guard.get_mut(&incoming.id) {
        existing.updated_at = incoming.updated_at;
        existing.status = incoming.status.clone();
        existing.phase = incoming.phase;
        existing.project_label = incoming.project_label;
        existing.waiting_reason = incoming.waiting_reason;
        existing.process_id = incoming.process_id.or(existing.process_id);
        existing.origin = incoming.origin.or_else(|| existing.origin.clone());
        existing.actions.extend(incoming.actions);
        if existing.actions.len() > 3 {
            existing.actions.drain(0..existing.actions.len() - 3);
        }
    } else {
        guard.insert(incoming.id.clone(), incoming.clone());
    }
    if previous_status.as_deref() != Some(incoming.status.as_str())
        && matches!(incoming.status.as_str(), "waiting" | "error")
    {
        Some(incoming.status)
    } else {
        None
    }
}

fn snapshot_from(
    sessions: &Arc<RwLock<HashMap<String, LiveSession>>>,
    socket_ready: bool,
) -> LiveSnapshot {
    let mut items = sessions
        .read()
        .map(|value| value.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    items.sort_by(|left, right| {
        priority(&right.status)
            .cmp(&priority(&left.status))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    let urgent_session_id = items.first().map(|item| item.id.clone());
    let active_count = items
        .iter()
        .filter(|item| item.status != "completed")
        .count() as u64;
    LiveSnapshot {
        generated_at: Utc::now().to_rfc3339(),
        sessions: items,
        urgent_session_id,
        active_count,
        hook_status: hook_status(socket_ready),
    }
}

fn prune_sessions(sessions: &Arc<RwLock<HashMap<String, LiveSession>>>) {
    let now = Utc::now();
    if let Ok(mut guard) = sessions.write() {
        guard.retain(|_, session| {
            let age = DateTime::parse_from_rfc3339(&session.updated_at)
                .map(|value| now.signed_duration_since(value.with_timezone(&Utc)))
                .unwrap_or_else(|_| Duration::zero());
            if session.status == "completed" {
                age < Duration::seconds(45)
            } else {
                age < Duration::hours(6)
            }
        });
    }
}

fn priority(status: &str) -> u8 {
    match status {
        "waiting" => 4,
        "error" => 3,
        "running" => 2,
        "idle" => 1,
        "completed" => 0,
        _ => 0,
    }
}

fn phase_for<'a>(event: &'a str, tool: &'a str, status: &str) -> (&'a str, &'a str) {
    if status == "waiting" {
        return ("needs-you", "waiting");
    }
    if status == "error" {
        return ("error", "error");
    }
    if status == "completed" {
        return ("completed", "completed");
    }
    let normalized = tool.to_ascii_lowercase();
    if normalized.contains("test") || normalized.contains("lint") || normalized.contains("check") {
        ("verifying", "verify")
    } else if normalized.contains("edit")
        || normalized.contains("write")
        || normalized.contains("patch")
    {
        ("editing", "edit")
    } else if normalized.contains("read")
        || normalized.contains("search")
        || normalized.contains("grep")
    {
        ("reading", "read")
    } else if event == "UserPromptSubmit" {
        ("thinking", "prompt")
    } else if matches!(event, "PreCompact" | "PostCompact") {
        ("compacting", "compact")
    } else if !tool.is_empty() {
        ("running-tool", "tool")
    } else {
        ("ready", "session")
    }
}

fn string_field(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn notify_if_background(session: &LiveSession, status: &str) {
    if source_is_foreground(session) {
        return;
    }
    let body = if status == "waiting" {
        format!("{} needs your attention.", provider_label(&session.agent))
    } else {
        format!("{} reported an error.", provider_label(&session.agent))
    };
    let escaped = body.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{escaped}\" with title \"VibeMeter\"");
    let _ = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .status();
}

fn source_is_foreground(session: &LiveSession) -> bool {
    let Some(name) = frontmost_application_name() else {
        return false;
    };
    source_matches_frontmost(session, &name)
}

fn source_matches_frontmost(session: &LiveSession, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if session.origin.as_deref() == Some("desktop") {
        return name.contains("codex");
    }
    name.contains("terminal") || name.contains("iterm") || name.contains("warp")
}

#[cfg(target_os = "macos")]
fn frontmost_application_name() -> Option<String> {
    use objc2_app_kit::NSWorkspace;

    let application = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    application.localizedName().map(|name| name.to_string())
}

#[cfg(not(target_os = "macos"))]
fn frontmost_application_name() -> Option<String> {
    None
}

fn provider_label(agent: &str) -> &'static str {
    if agent == "claude-code" {
        "Claude Code"
    } else {
        "Codex"
    }
}

fn hook_script_path() -> AppResult<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| AppError::InvalidRequest("home directory is unavailable".into()))?
        .join(".vibemeter/hooks/vibemeter_hook.py"))
}

fn socket_path() -> AppResult<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| AppError::InvalidRequest("home directory is unavailable".into()))?
        .join(".vibemeter/vibemeter.sock"))
}

fn upsert_hook_json(
    path: &Path,
    command: &str,
    desired: &[(&str, Option<&str>, Option<u64>)],
) -> AppResult<()> {
    let mut root = read_json_object(path)?;
    upsert_managed_hooks(&mut root, path, command, desired)?;
    let bytes = serde_json::to_vec_pretty(&Value::Object(root))?;
    write_if_changed(path, &bytes, None, true)
}

fn upsert_codex_hook_json(
    path: &Path,
    command: &str,
    desired: &[(&str, Option<&str>, Option<u64>)],
) -> AppResult<()> {
    let mut root = read_json_object(path)?;
    migrate_codex_legacy_events(&mut root, path)?;
    upsert_managed_hooks(&mut root, path, command, desired)?;
    let bytes = serde_json::to_vec_pretty(&Value::Object(root))?;
    write_if_changed(path, &bytes, None, true)
}

fn migrate_codex_legacy_events(root: &mut Map<String, Value>, path: &Path) -> AppResult<()> {
    let mut legacy = Vec::new();
    for event in CODEX_EVENT_NAMES {
        let Some(value) = root.remove(*event) else {
            continue;
        };
        let groups = value.as_array().cloned().ok_or_else(|| {
            AppError::InvalidRequest(format!(
                "{} legacy hook event {} must be an array",
                path.display(),
                event
            ))
        })?;
        legacy.push((*event, groups));
    }
    if legacy.is_empty() {
        return Ok(());
    }
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            AppError::InvalidRequest(format!("{} hooks must be an object", path.display()))
        })?;
    for (event, mut groups) in legacy {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| {
                AppError::InvalidRequest(format!(
                    "{} hook event {} must be an array",
                    path.display(),
                    event
                ))
            })?;
        groups.append(entries);
        *entries = groups;
    }
    Ok(())
}

fn upsert_managed_hooks(
    root: &mut Map<String, Value>,
    path: &Path,
    command: &str,
    desired: &[(&str, Option<&str>, Option<u64>)],
) -> AppResult<()> {
    prune_managed_hooks(root);
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            AppError::InvalidRequest(format!("{} hooks must be an object", path.display()))
        })?;
    for (event, matcher, timeout) in desired {
        let entries = hooks
            .entry(*event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| {
                AppError::InvalidRequest(format!(
                    "{} hook event {} must be an array",
                    path.display(),
                    event
                ))
            })?;
        let mut hook = json!({ "type": "command", "command": command });
        if let Some(timeout) = timeout {
            hook["timeout"] = json!(timeout);
        }
        let mut group = json!({ "hooks": [hook] });
        if let Some(matcher) = matcher {
            group["matcher"] = json!(matcher);
        }
        entries.push(group);
    }
    Ok(())
}

fn remove_managed_hooks(path: &Path) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut root = read_json_object(path)?;
    let changed = prune_managed_hooks(&mut root);
    if changed {
        let bytes = serde_json::to_vec_pretty(&Value::Object(root))?;
        write_if_changed(path, &bytes, None, true)?;
    }
    Ok(())
}

fn read_json_object(path: &Path) -> AppResult<Map<String, Value>> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let bytes = fs::read(path)?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Map::new());
    }
    serde_json::from_slice::<Value>(&bytes)?
        .as_object()
        .cloned()
        .ok_or_else(|| AppError::InvalidRequest(format!("{} is not a JSON object", path.display())))
}

fn prune_managed_hooks(root: &mut Map<String, Value>) -> bool {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    hooks.retain(|_, entries| {
        let Some(groups) = entries.as_array_mut() else {
            return true;
        };
        let original_groups = groups.len();
        groups.retain_mut(|group| {
            let Some(commands) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let original_commands = commands.len();
            commands.retain(|command| {
                !command
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.contains(MANAGED_MARKER))
            });
            if commands.len() != original_commands {
                changed = true;
            }
            !commands.is_empty()
        });
        if groups.len() != original_groups {
            changed = true;
        }
        !groups.is_empty()
    });
    if hooks.is_empty() {
        root.remove("hooks");
        changed = true;
    }
    changed
}

#[cfg(test)]
fn json_contains_managed_hook(path: &Path) -> bool {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .is_some_and(|root| {
            root.get("hooks")
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(|hooks| hooks.values())
                .filter_map(Value::as_array)
                .flatten()
                .filter_map(|group| group.get("hooks").and_then(Value::as_array))
                .flatten()
                .any(|hook| {
                    hook.get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|command| command.contains(MANAGED_MARKER))
                })
        })
}

fn enable_codex_hooks(path: &Path) -> AppResult<()> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let updated = upsert_codex_feature(&text);
    write_if_changed(path, updated.as_bytes(), None, true)
}

fn upsert_codex_feature(text: &str) -> String {
    let mut updated = remove_legacy_codex_feature(text);
    if let Some((body_start, block_end)) = codex_features_block(&updated) {
        let block = &updated[body_start..block_end];
        let line = Regex::new(r"(?m)^[ \t]*hooks[ \t]*=[^\n]*$").expect("hooks regex");
        if let Some(existing) = line.find(block) {
            updated.replace_range(
                body_start + existing.start()..body_start + existing.end(),
                "hooks = true",
            );
            return updated;
        }
        let insertion = if updated.as_bytes().get(body_start.saturating_sub(1)) == Some(&b'\n') {
            "hooks = true\n"
        } else {
            "\nhooks = true\n"
        };
        updated.insert_str(body_start, insertion);
        return updated;
    }
    let separator = if updated.is_empty() || updated.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    format!("{updated}{separator}\n[features]\nhooks = true\n")
}

fn codex_hook_feature_enabled(text: &str) -> bool {
    let Some((body_start, block_end)) = codex_features_block(text) else {
        return false;
    };
    Regex::new(r"(?m)^[ \t]*hooks[ \t]*=[ \t]*true[ \t]*$")
        .expect("codex feature regex")
        .is_match(&text[body_start..block_end])
}

fn remove_legacy_codex_feature(text: &str) -> String {
    let mut updated = text.to_string();
    let Some((body_start, block_end)) = codex_features_block(&updated) else {
        return updated;
    };
    let legacy =
        Regex::new(r"(?m)^[ \t]*codex_hooks[ \t]*=[^\n]*(?:\n|$)").expect("legacy hooks regex");
    if let Some(found) = legacy.find(&updated[body_start..block_end]) {
        updated.replace_range(body_start + found.start()..body_start + found.end(), "");
    }
    updated
}

fn codex_features_block(text: &str) -> Option<(usize, usize)> {
    let features = Regex::new(r"(?m)^\[features\][ \t]*$").expect("features regex");
    let found = features.find(text)?;
    let body_start = if text.as_bytes().get(found.end()) == Some(&b'\n') {
        found.end() + 1
    } else {
        found.end()
    };
    let block_end = Regex::new(r"(?m)^\[[^\n]+\][ \t]*$")
        .expect("section regex")
        .find(&text[body_start..])
        .map(|next| body_start + next.start())
        .unwrap_or(text.len());
    Some((body_start, block_end))
}

fn write_if_changed(
    path: &Path,
    bytes: &[u8],
    mode: Option<u32>,
    backup_existing: bool,
) -> AppResult<()> {
    let existing = fs::read(path).ok();
    if existing.as_deref() == Some(bytes) {
        if let Some(mode) = mode {
            fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if backup_existing && path.exists() {
        let stamp = Utc::now().format("%Y%m%dT%H%M%S%3fZ");
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("settings");
        let backup = path.with_file_name(format!("{name}.vibemeter-backup-{stamp}"));
        fs::copy(path, backup)?;
    }
    let temporary = path.with_extension(format!("vibemeter-tmp-{}", std::process::id()));
    fs::write(&temporary, bytes)?;
    if let Some(mode) = mode {
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))?;
    } else if let Ok(metadata) = fs::metadata(path) {
        fs::set_permissions(&temporary, metadata.permissions())?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn hook_merge_preserves_other_commands_and_uninstall_only_prunes_ours() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("settings.json");
        fs::write(
            &path,
            br#"{"hooks":{"PreToolUse":[{"matcher":"*","hooks":[{"type":"command","command":"other-hook"}]}]},"theme":"dark"}"#,
        )
        .expect("fixture");
        upsert_hook_json(
            &path,
            "\"/usr/bin/python3\" \"/tmp/vibemeter_hook.py\" claude",
            &[("PreToolUse", Some("*"), None), ("Stop", None, Some(30))],
        )
        .expect("install");
        assert!(json_contains_managed_hook(&path));
        remove_managed_hooks(&path).expect("uninstall");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("other-hook"));
        assert!(text.contains("\"theme\": \"dark\""));
        assert!(!text.contains(MANAGED_MARKER));
    }

    #[test]
    fn codex_feature_merge_reuses_features_section() {
        let updated = upsert_codex_feature("[model]\nname=\"x\"\n\n[features]\nfoo=true\n");
        assert_eq!(updated.matches("[features]").count(), 1);
        assert!(codex_hook_feature_enabled(&updated));
        assert!(updated.contains("foo=true"));
        assert!(!codex_hook_feature_enabled(
            "[features]\ncodex_hooks = true\n"
        ));
        let scoped = upsert_codex_feature(
            "[features]\nhooks = false\ncodex_hooks = true\n\n[unrelated]\nhooks = false\n",
        );
        assert!(codex_hook_feature_enabled(&scoped));
        assert!(!scoped.contains("codex_hooks"));
        assert!(scoped.contains("[unrelated]\nhooks = false"));
        assert_eq!(
            upsert_codex_feature("[features]"),
            "[features]\nhooks = true\n"
        );
    }

    #[test]
    fn codex_hook_merge_migrates_legacy_events_without_losing_other_commands() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("hooks.json");
        fs::write(
            &path,
            br#"{
              "SessionStart": [{
                "matcher": "startup",
                "hooks": [{"type": "command", "command": "other-session-hook"}]
              }],
              "hooks": {
                "Stop": [{
                  "hooks": [{"type": "command", "command": "python3 /tmp/vibemeter_hook.py codex"}]
                }]
              }
            }"#,
        )
        .expect("fixture");
        upsert_codex_hook_json(
            &path,
            "\"/usr/bin/python3\" \"/tmp/vibemeter_hook.py\" codex",
            CODEX_HOOKS,
        )
        .expect("install");
        let root = read_json_object(&path).expect("read migrated config");
        assert!(!root.contains_key("SessionStart"));
        let hooks = root
            .get("hooks")
            .and_then(Value::as_object)
            .expect("nested hooks");
        assert!(
            hooks["SessionStart"]
                .as_array()
                .expect("session groups")
                .iter()
                .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
                .any(|hook| hook["command"] == "other-session-hook")
        );
        let managed = managed_hook_events(&root).expect("managed events");
        assert!(
            CODEX_HOOKS
                .iter()
                .all(|(event, _, _)| managed.contains(*event))
        );
    }

    #[test]
    fn codex_runtime_status_requires_loaded_trusted_hooks() {
        let hooks = CODEX_RUNTIME_EVENTS
            .iter()
            .map(|event| {
                json!({
                    "eventName": event,
                    "command": "/usr/bin/python3 ~/.vibemeter/hooks/vibemeter_hook.py codex",
                    "enabled": true,
                    "trustStatus": "untrusted"
                })
            })
            .collect::<Vec<_>>();
        let untrusted = json!({"data":[{"hooks":hooks,"warnings":[],"errors":[]}]});
        let health = codex_hook_health_from_list(&untrusted);
        assert!(!health.working);
        assert_eq!(health.detail, "review-required");

        let trusted_hooks = CODEX_RUNTIME_EVENTS
            .iter()
            .map(|event| {
                json!({
                    "eventName": event,
                    "command": "/usr/bin/python3 ~/.vibemeter/hooks/vibemeter_hook.py codex",
                    "enabled": true,
                    "trustStatus": "trusted"
                })
            })
            .collect::<Vec<_>>();
        let trusted = json!({"data":[{"hooks":trusted_hooks,"warnings":[],"errors":[]}]});
        let health = codex_hook_health_from_list(&trusted);
        assert!(health.working);
        assert_eq!(health.detail, "ready");

        let invalid = json!({
            "data":[{
                "hooks":[],
                "warnings":["failed to parse hooks config /Users/me/.codex/hooks.json"],
                "errors":[]
            }]
        });
        assert_eq!(
            codex_hook_health_from_list(&invalid).detail,
            "config-invalid"
        );
    }

    #[test]
    fn claude_failure_and_attention_notifications_map_to_visible_states() {
        let failure = json!({
            "provider":"claude",
            "received_at":Utc::now().to_rfc3339(),
            "payload":{
                "session_id":"claude-failure",
                "hook_event_name":"PostToolUseFailure",
                "cwd":"/tmp/project",
                "tool_name":"Bash"
            }
        });
        let (session, _, _) = session_from_envelope(&failure).expect("failure session");
        assert_eq!(session.status, "error");
        assert_eq!(session.phase, "error");

        let attention = json!({
            "provider":"claude",
            "received_at":Utc::now().to_rfc3339(),
            "payload":{
                "session_id":"claude-attention",
                "hook_event_name":"Notification",
                "notification_type":"idle_prompt",
                "cwd":"/tmp/project"
            }
        });
        let (session, _, _) = session_from_envelope(&attention).expect("attention session");
        assert_eq!(session.status, "waiting");
        assert_eq!(session.phase, "needs-you");
    }

    #[test]
    fn live_project_label_prefers_repository_metadata_over_the_folder_name() {
        let directory = tempdir().expect("tempdir");
        let root = directory.path().join("TokenGraph");
        let nested = root.join("apps/desktop");
        fs::create_dir_all(root.join(".git")).expect("git marker");
        fs::create_dir_all(&nested).expect("nested cwd");
        fs::write(
            root.join("package.json"),
            br#"{"name":"vibemeter","private":true}"#,
        )
        .expect("package metadata");
        assert_eq!(
            project_label_from_cwd(nested.to_str().expect("cwd")),
            "vibemeter"
        );
    }

    #[test]
    fn live_project_label_supports_scoped_packages_and_folder_fallback() {
        let directory = tempdir().expect("tempdir");
        let scoped = directory.path().join("scoped-folder");
        fs::create_dir_all(scoped.join(".git")).expect("git marker");
        fs::write(
            scoped.join("package.json"),
            br#"{"name":"@vibemeter/desktop"}"#,
        )
        .expect("package metadata");
        assert_eq!(
            project_label_from_cwd(scoped.to_str().expect("cwd")),
            "desktop"
        );

        let fallback = directory.path().join("plain-project");
        fs::create_dir_all(&fallback).expect("fallback cwd");
        assert_eq!(
            project_label_from_cwd(fallback.to_str().expect("cwd")),
            "plain-project"
        );
    }

    #[test]
    fn priority_matches_product_contract() {
        assert!(priority("waiting") > priority("error"));
        assert!(priority("error") > priority("running"));
        assert!(priority("running") > priority("completed"));
    }

    #[test]
    fn foreground_matching_stays_provider_and_origin_specific() {
        let desktop = LiveSession {
            id: "desktop".into(),
            source_session_id: "thread".into(),
            agent: "codex".into(),
            project_label: "project".into(),
            status: "running".into(),
            phase: "thinking".into(),
            started_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            waiting_reason: None,
            actions: Vec::new(),
            process_id: None,
            origin: Some("desktop".into()),
        };
        let mut cli = desktop.clone();
        cli.origin = Some("cli".into());
        assert!(source_matches_frontmost(&desktop, "Codex"));
        assert!(!source_matches_frontmost(&desktop, "Terminal"));
        assert!(source_matches_frontmost(&cli, "iTerm2"));
        assert!(!source_matches_frontmost(&cli, "VibeMeter"));
    }
}
