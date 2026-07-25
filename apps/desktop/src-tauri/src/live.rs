use crate::database::Database;
use crate::errors::{AppError, AppResult};
use crate::models::{HookProviderStatus, HookStatus, LiveAction, LiveSession, LiveSnapshot};
use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter};

const RAW_RETENTION_DAYS: i64 = 90;
const MAX_HOOK_BYTES: u64 = 768 * 1024;
const MANAGED_MARKER: &str = "vibemeter_hook.py";
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
        upsert_hook_json(
            &settings,
            &format!("{command} claude"),
            &[
                ("SessionStart", None, None),
                ("UserPromptSubmit", None, None),
                ("PreToolUse", Some("*"), None),
                ("PostToolUse", Some("*"), None),
                ("PermissionRequest", Some("*"), None),
                ("PreCompact", Some("auto|manual"), None),
                ("Stop", None, Some(30)),
                ("SubagentStop", None, None),
                ("SessionEnd", None, None),
            ],
        )?;
    }
    let codex_dir = home.join(".codex");
    if codex_dir.is_dir() {
        let hooks = codex_dir.join("hooks.json");
        upsert_hook_json(
            &hooks,
            &format!("{command} codex"),
            &[
                ("SessionStart", Some("startup|resume"), None),
                ("UserPromptSubmit", None, None),
                ("Stop", None, Some(30)),
            ],
        )?;
        enable_codex_hooks(&codex_dir.join("config.toml"))?;
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
    Ok(hook_status(socket_path()?.exists()))
}

pub fn hook_status(socket_ready: bool) -> HookStatus {
    let home = dirs::home_dir().unwrap_or_default();
    let claude_path = home.join(".claude/settings.json");
    let codex_hooks = home.join(".codex/hooks.json");
    let codex_config = home.join(".codex/config.toml");
    let claude_available = home.join(".claude").is_dir();
    let codex_available = home.join(".codex").is_dir();
    let claude_installed = json_contains_managed_hook(&claude_path);
    let codex_command_installed = json_contains_managed_hook(&codex_hooks);
    let codex_feature = fs::read_to_string(&codex_config)
        .ok()
        .is_some_and(|text| codex_hook_feature_enabled(&text));
    let providers = vec![
        HookProviderStatus {
            provider: "claude-code".into(),
            available: claude_available,
            installed: claude_installed,
            detail: if !claude_available {
                "not-found"
            } else if claude_installed {
                "ready"
            } else {
                "hook-missing"
            }
            .into(),
        },
        HookProviderStatus {
            provider: "codex".into(),
            available: codex_available,
            installed: codex_command_installed && codex_feature,
            detail: if !codex_available {
                "not-found"
            } else if !codex_feature {
                "feature-disabled"
            } else if !codex_command_installed {
                "hook-missing"
            } else {
                "ready"
            }
            .into(),
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
    let project_label = Path::new(&cwd)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Unknown project")
        .to_string();
    let tool = string_field(payload, &["tool_name", "tool"]).unwrap_or_default();
    let error = payload
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || payload
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "error" | "failed"));
    let status = if error {
        "error"
    } else if event_name == "PermissionRequest" {
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
    } else if event == "PreCompact" {
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
    prune_managed_hooks(&mut root);
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
    let bytes = serde_json::to_vec_pretty(&Value::Object(root))?;
    write_if_changed(path, &bytes, None, true)
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
    let line = Regex::new(r"(?m)^[ \t]*codex_hooks[ \t]*=[^\n]*$").expect("codex hooks regex");
    if line.is_match(text) {
        return line.replace(text, "codex_hooks = true").into_owned();
    }
    let features = Regex::new(r"(?m)^\[features\][ \t]*$").expect("features regex");
    if let Some(found) = features.find(text) {
        let insertion = text[found.end()..]
            .find('\n')
            .map(|offset| found.end() + offset + 1)
            .unwrap_or(found.end());
        let mut updated = text.to_string();
        let feature_line = if insertion == found.end() {
            "\ncodex_hooks = true\n"
        } else {
            "codex_hooks = true\n"
        };
        updated.insert_str(insertion, feature_line);
        return updated;
    }
    let separator = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    format!("{text}{separator}\n[features]\ncodex_hooks = true\n")
}

fn codex_hook_feature_enabled(text: &str) -> bool {
    Regex::new(r"(?m)^[ \t]*codex_hooks[ \t]*=[ \t]*true[ \t]*$")
        .expect("codex feature regex")
        .is_match(text)
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
