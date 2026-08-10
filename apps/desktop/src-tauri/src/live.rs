use crate::database::Database;
use crate::errors::{AppError, AppResult};
use crate::live_sources;
use crate::models::{
    AttentionEvent, HookProviderStatus, HookStatus, LiveAction, LiveJumpContext, LiveSession,
    LiveSnapshot, NotchCompletedSession, ObservedLiveEvent, WorkPulse, WorkPulseDimension,
};
use crate::providers::{codex_binary, write_json_line};
use crate::source_capabilities::{SourceLiveCapability, source_capabilities};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::time::{Duration as StdDuration, Instant, SystemTime};
use tauri::{AppHandle, Emitter};

const MAX_HOOK_BYTES: u64 = 768 * 1024;
const MAX_TRANSCRIPT_TAIL_BYTES: u64 = 1024 * 1024;
const MAX_TRANSCRIPT_READ_BYTES: u64 = MAX_TRANSCRIPT_TAIL_BYTES;
const MAX_TRANSCRIPT_BOUNDARY_BYTES: u64 = 8 * MAX_TRANSCRIPT_TAIL_BYTES;
const MAX_TRANSCRIPT_LINE_BYTES: usize = 256 * 1024;
const MAX_SESSION_META_BYTES: u64 = 512 * 1024;
const CODEX_DISCOVERY_INTERVAL: StdDuration = StdDuration::from_secs(2);
const EXTERNAL_DISCOVERY_INTERVAL: StdDuration = StdDuration::from_millis(750);
const CODEX_DISCOVERY_WINDOW: StdDuration = StdDuration::from_secs(10 * 60);
const MAX_DISCOVERED_TRANSCRIPTS: usize = 64;
const MANAGED_MARKER: &str = "vibemeter_hook.py";
const CODEX_PROBE_TTL: StdDuration = StdDuration::from_secs(20);
const WORK_PULSE_FRESH_SECONDS: u64 = 30;
const WORK_PULSE_LOST_UPDATE_SECONDS: u64 = 120;
const ATTENTION_NOTIFICATION_TIMEOUT: StdDuration = StdDuration::from_secs(2);
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

#[derive(Debug)]
struct CodexTranscriptWatch {
    path: PathBuf,
    offset: u64,
    discard_partial_line: bool,
    initialized: bool,
    collaboration_mode: CodexCollaborationMode,
}

struct CodexTranscriptBootstrap {
    session_id: String,
    session: Option<LiveSession>,
    watch: CodexTranscriptWatch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CodexCollaborationMode {
    #[default]
    Default,
    Plan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CodexMetadataSignal {
    phase: String,
    status: String,
    action_kind: String,
    action_label: String,
    occurred_at: String,
    waiting_reason: Option<String>,
}
const PYTHON_HOOK: &str = r#"#!/usr/bin/python3
import json
import os
import shutil
import socket
import subprocess
import sys

MAX_BYTES = 768 * 1024

def app_name_from_command(command):
    marker = ".app/"
    lower = command.lower()
    index = lower.find(marker)
    if index < 0:
        return None
    bundle_path = command[:index + len(".app")]
    name = os.path.basename(bundle_path)
    return name[:-4] if name.lower().endswith(".app") else name

def terminal_kind_from_environment(host_app_name):
    if os.environ.get("CMUX_WORKSPACE_ID") or os.environ.get("CMUX_SURFACE_ID"):
        return "cmux"
    candidates = " ".join(filter(None, [
        os.environ.get("TERM_PROGRAM"),
        os.environ.get("LC_TERMINAL"),
        os.environ.get("TERM"),
        os.environ.get("VSCODE_IPC_HOOK_CLI"),
        os.environ.get("VSCODE_GIT_ASKPASS_NODE"),
        host_app_name,
    ])).lower()
    if "apple_terminal" in candidates or host_app_name == "Terminal":
        return "terminal"
    if "iterm" in candidates:
        return "iterm"
    if "warp" in candidates:
        return "warp"
    if "ghostty" in candidates:
        return "ghostty"
    if "wezterm" in candidates:
        return "wezterm"
    if "kitty" in candidates:
        return "kitty"
    if "alacritty" in candidates:
        return "alacritty"
    if "cursor" in candidates:
        return "cursor"
    if "vscode" in candidates or host_app_name == "Visual Studio Code":
        return "vscode"
    return None

def jump_context(table, process_id, tty):
    host_app_name = None
    current = process_id
    for _ in range(24):
        info = table.get(current)
        if not info:
            break
        host_app_name = app_name_from_command(info["command"]) or host_app_name
        if host_app_name:
            break
        parent = info["ppid"]
        if parent <= 1 or parent == current:
            break
        current = parent

    context = {
        "tty": tty if tty and tty != "??" else None,
        "terminalKind": terminal_kind_from_environment(host_app_name),
        "hostAppName": host_app_name,
        "processStartedAt": table.get(process_id, {}).get("started_at"),
        "tmuxSocket": None,
        "tmuxPane": os.environ.get("TMUX_PANE"),
        "tmuxExecutable": shutil.which("tmux"),
        "cmuxSocket": None,
        "cmuxWorkspaceId": os.environ.get("CMUX_WORKSPACE_ID"),
        "cmuxSurfaceId": os.environ.get("CMUX_SURFACE_ID"),
        "cmuxExecutable": shutil.which("cmux"),
    }
    tmux = os.environ.get("TMUX")
    if tmux:
        context["tmuxSocket"] = tmux.split(",", 1)[0]
    if context["cmuxWorkspaceId"] or context["cmuxSurfaceId"]:
        context["cmuxSocket"] = os.environ.get(
            "CMUX_SOCKET_PATH",
            os.path.expanduser("~/.local/state/cmux/cmux.sock"),
        )
        if not context["cmuxExecutable"]:
            bundled = "/Applications/cmux.app/Contents/Resources/bin/cmux"
            if os.path.isfile(bundled):
                context["cmuxExecutable"] = bundled
        context["hostAppName"] = context["hostAppName"] or "cmux"
    return {key: value for key, value in context.items() if value is not None}

def process_context(provider):
    try:
        output = subprocess.check_output(
            ["/bin/ps", "-axo", "pid=,ppid=,tty=,lstart=,comm="],
            text=True,
            timeout=0.6,
        )
    except Exception:
        return {}
    table = {}
    for line in output.splitlines():
        parts = line.strip().split(None, 8)
        if len(parts) != 9 or not parts[0].isdigit() or not parts[1].isdigit():
            continue
        table[int(parts[0])] = {
            "ppid": int(parts[1]),
            "tty": parts[2],
            "started_at": " ".join(parts[3:8]),
            "command": parts[8],
        }
    current = os.getppid()
    fallback = None
    selected = None
    for _ in range(12):
        info = table.get(current)
        if not info:
            break
        command = os.path.basename(info["command"]).lower()
        origin = "cli" if info["tty"] != "??" else "desktop"
        if fallback is None:
            fallback = (current, origin, info["tty"])
        if provider in command or (provider == "claude" and "claude-code" in command):
            selected = (current, origin, info["tty"])
            break
        parent = info["ppid"]
        if parent <= 1 or parent == current:
            break
        current = parent
    selected = selected or fallback
    if selected:
        return {
            "process_id": selected[0],
            "origin": selected[1],
            "tty": selected[2],
            "jump_context": jump_context(table, selected[0], selected[2]),
        }
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
    database: Database,
}

impl LiveMonitor {
    pub fn start(
        database: Database,
        app: AppHandle,
        diagnostics: crate::diagnostics::DiagnosticRetention,
    ) -> AppResult<Self> {
        let sessions = Arc::new(RwLock::new(HashMap::new()));
        let auxiliary_sessions = Arc::new(RwLock::new(HashMap::new()));
        let socket_ready = Arc::new(AtomicBool::new(false));
        let codex_transcripts = Arc::new(Mutex::new(HashMap::new()));
        let monitor = Self {
            sessions: sessions.clone(),
            socket_ready: socket_ready.clone(),
            database: database.clone(),
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
        let transcript_sessions = sessions.clone();
        let transcript_socket_ready = socket_ready.clone();
        let transcript_app = app.clone();
        let transcript_watches = codex_transcripts.clone();
        let transcript_database = database.clone();
        std::thread::spawn(move || {
            let mut discover_immediately = true;
            let mut last_discovery = Instant::now();
            loop {
                let should_discover =
                    discover_immediately || last_discovery.elapsed() >= CODEX_DISCOVERY_INTERVAL;
                let discovered = if should_discover {
                    discover_immediately = false;
                    last_discovery = Instant::now();
                    discover_codex_transcripts(&transcript_watches, &transcript_sessions)
                } else {
                    false
                };
                if discovered {
                    let completed_sessions = transcript_database
                        .notch_completed_sessions()
                        .unwrap_or_default();
                    let snapshot = snapshot_from(
                        &transcript_sessions,
                        transcript_socket_ready.load(Ordering::SeqCst),
                        completed_sessions,
                        &transcript_database,
                    );
                    let _ = transcript_app.emit("live-update", &snapshot);
                }
                let updates = poll_codex_transcripts(&transcript_watches, &transcript_sessions);
                for (session_id, signal, live_append) in updates {
                    let signal_for_recording = signal.clone();
                    let transition = merge_codex_metadata(
                        &transcript_sessions,
                        &session_id,
                        signal,
                        live_append,
                    );
                    if live_append
                        && signal_for_recording.status == "paused"
                        && let Some(session) = transcript_sessions
                            .read()
                            .ok()
                            .and_then(|items| items.get(&session_id).cloned())
                        && session.updated_at == signal_for_recording.occurred_at
                        && session.status == signal_for_recording.status
                        && session.phase == signal_for_recording.phase
                    {
                        let observed = observed_live_event_from_codex_metadata(
                            &session,
                            &signal_for_recording,
                            Utc::now().to_rfc3339(),
                        );
                        let _ = transcript_database.record_observed_live_event(&observed);
                    }
                    if transition.as_deref() == Some("completed")
                        && let Some(session) = transcript_sessions
                            .read()
                            .ok()
                            .and_then(|items| items.get(&session_id).cloned())
                    {
                        let _ = transcript_database.complete_notch_session(&session);
                    }
                    let completed_sessions = transcript_database
                        .notch_completed_sessions()
                        .unwrap_or_default();
                    let snapshot = snapshot_from(
                        &transcript_sessions,
                        transcript_socket_ready.load(Ordering::SeqCst),
                        completed_sessions,
                        &transcript_database,
                    );
                    let _ = transcript_app.emit("live-update", &snapshot);
                    if let Some(status) = transition.as_deref()
                        && let Some(active) =
                            snapshot.sessions.iter().find(|item| item.id == session_id)
                    {
                        notify_if_background(&transcript_database, active, status);
                    }
                }
                prune_transcript_watches(&transcript_watches, &transcript_sessions);
                std::thread::sleep(StdDuration::from_millis(250));
            }
        });
        let external_sessions = sessions.clone();
        let external_app = app.clone();
        let external_database = database.clone();
        let external_socket_ready = socket_ready.clone();
        std::thread::spawn(move || {
            let mut known_ids = HashSet::new();
            let mut recorded = HashMap::<String, String>::new();
            loop {
                let discovered = live_sources::discover();
                let discovered_ids = discovered
                    .iter()
                    .map(|session| session.id.clone())
                    .collect::<HashSet<_>>();
                let mut transitions = Vec::new();
                let mut changed = false;
                for session in discovered {
                    let id = session.id.clone();
                    let fingerprint = format!(
                        "{}|{}|{}|{}",
                        session.status,
                        session.updated_at,
                        session.phase,
                        session
                            .actions
                            .last()
                            .map(|action| action.label.as_str())
                            .unwrap_or_default()
                    );
                    let should_record = recorded.get(&id) != Some(&fingerprint);
                    known_ids.insert(id.clone());
                    let transition = merge_session(&external_sessions, session.clone());
                    if let Some(status) = transition {
                        transitions.push((id.clone(), status));
                        changed = true;
                    }
                    if should_record {
                        recorded.insert(id, fingerprint);
                        let _ = external_database.record_live_event(
                            &session.updated_at,
                            &session.agent,
                            &session.source_session_id,
                            "runtime.activity",
                            &session.project_label,
                            &session.status,
                        );
                        changed = true;
                    }
                }
                let stale_ids = known_ids
                    .difference(&discovered_ids)
                    .cloned()
                    .collect::<Vec<_>>();
                if !stale_ids.is_empty()
                    && let Ok(mut sessions) = external_sessions.write()
                {
                    for id in &stale_ids {
                        if sessions.get(id).is_some_and(|session| {
                            matches!(session.agent.as_str(), "kimi-code" | "zcode")
                        }) {
                            sessions.remove(id);
                            recorded.remove(id);
                            changed = true;
                        }
                    }
                }
                known_ids.retain(|id| discovered_ids.contains(id));
                if changed {
                    let completed_sessions = external_database
                        .notch_completed_sessions()
                        .unwrap_or_default();
                    let snapshot = snapshot_from(
                        &external_sessions,
                        external_socket_ready.load(Ordering::SeqCst),
                        completed_sessions,
                        &external_database,
                    );
                    let _ = external_app.emit("live-update", &snapshot);
                    for (id, status) in transitions {
                        if let Some(active) = snapshot.sessions.iter().find(|item| item.id == id) {
                            notify_if_background(&external_database, active, &status);
                        }
                    }
                }
                std::thread::sleep(EXTERNAL_DISCOVERY_INTERVAL);
            }
        });
        let listener_watches = codex_transcripts;
        let listener_auxiliary_sessions = auxiliary_sessions;
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
                    let auxiliary_memory_activity = is_codex_memory_activity(&envelope);
                    let Some(session) = fold_codex_memory_activity(
                        &sessions,
                        &listener_auxiliary_sessions,
                        &envelope,
                        session,
                    ) else {
                        continue;
                    };
                    if !auxiliary_memory_activity {
                        register_codex_transcript(&listener_watches, &envelope, &session);
                    }
                    let transitioned_session_id = session.id.clone();
                    let observed = observed_live_event_from_envelope(
                        &envelope,
                        &session,
                        raw,
                        event_name,
                        Utc::now().to_rfc3339(),
                    );
                    if database.record_observed_live_event(&observed).is_ok()
                        && diagnostics.retain(&observed.payload_json).is_err()
                    {
                        eprintln!("VibeMeter diagnostic retention is unavailable");
                    }
                    let transition = merge_session(&sessions, session);
                    if transition.as_deref() == Some("completed")
                        && let Some(session) = sessions
                            .read()
                            .ok()
                            .and_then(|items| items.get(&transitioned_session_id).cloned())
                    {
                        let _ = database.complete_notch_session(&session);
                    }
                    let completed_sessions =
                        database.notch_completed_sessions().unwrap_or_default();
                    let snapshot = snapshot_from(
                        &sessions,
                        socket_ready.load(Ordering::SeqCst),
                        completed_sessions,
                        &database,
                    );
                    let _ = app.emit("live-update", &snapshot);
                    if transition.is_some()
                        && let Some(active) = snapshot
                            .sessions
                            .iter()
                            .find(|item| item.id == transitioned_session_id)
                    {
                        notify_if_background(
                            &database,
                            active,
                            transition.as_deref().unwrap_or_default(),
                        );
                    }
                }
            }
            socket_ready.store(false, Ordering::SeqCst);
        });
        Ok(monitor)
    }

    pub fn snapshot(&self) -> LiveSnapshot {
        prune_sessions(&self.sessions);
        snapshot_from(
            &self.sessions,
            self.socket_ready.load(Ordering::SeqCst),
            self.database.notch_completed_sessions().unwrap_or_default(),
            &self.database,
        )
    }

    pub fn session(&self, id: &str) -> Option<LiveSession> {
        self.sessions
            .read()
            .ok()
            .and_then(|sessions| sessions.get(id).cloned())
    }

    pub fn session_for_source(&self, agent: &str, source_session_id: &str) -> Option<LiveSession> {
        self.sessions.read().ok().and_then(|sessions| {
            sessions
                .values()
                .find(|session| {
                    session.agent == agent && session.source_session_id == source_session_id
                })
                .cloned()
        })
    }

    pub fn mark_notch_sessions_seen(&self, ids: &[String]) -> AppResult<()> {
        let sessions = self
            .sessions
            .read()
            .map(|sessions| {
                ids.iter()
                    .filter_map(|id| sessions.get(id))
                    .filter(|session| {
                        matches!(session.status.as_str(), "waiting" | "error" | "running")
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.database.mark_notch_sessions_seen(&sessions)
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
    let kimi_available = live_sources::provider_available("kimi-code");
    let zcode_available = live_sources::provider_available("zcode");
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
        HookProviderStatus {
            provider: "kimi-code".into(),
            available: kimi_available,
            installed: kimi_available,
            detail: if kimi_available { "ready" } else { "not-found" }.into(),
        },
        HookProviderStatus {
            provider: "zcode".into(),
            available: zcode_available,
            installed: zcode_available,
            detail: if zcode_available {
                "ready"
            } else {
                "not-found"
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
    let probe_result = (|| {
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
                    "clientInfo": {"name":"vibemeter","title":"VibeMeter","version":env!("CARGO_PKG_VERSION")},
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
        let result = result
            .ok_or_else(|| AppError::ProviderUnavailable("Codex hook probe timed out".into()))?;
        Ok(codex_hook_health_from_list(&result))
    })();
    let _ = child.kill();
    let _ = child.wait();
    probe_result
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JumpRoute {
    CodexDesktop,
    ZCodeDesktop,
    Cmux,
    Tmux,
    DirectTerminal,
}

#[derive(Clone, Debug)]
struct ProcessRecord {
    parent_id: u32,
    command: String,
}

fn jump_route(session: &LiveSession) -> JumpRoute {
    if session.agent == "codex" && session.origin.as_deref() == Some("desktop") {
        return JumpRoute::CodexDesktop;
    }
    if session.agent == "zcode" && session.origin.as_deref() == Some("desktop") {
        return JumpRoute::ZCodeDesktop;
    }
    if session.jump_context.as_ref().is_some_and(|context| {
        context.cmux_workspace_id.is_some() || context.cmux_surface_id.is_some()
    }) {
        return JumpRoute::Cmux;
    }
    if session
        .jump_context
        .as_ref()
        .is_some_and(|context| context.tmux_socket.is_some() && context.tmux_pane.is_some())
    {
        return JumpRoute::Tmux;
    }
    JumpRoute::DirectTerminal
}

pub fn jump_to_session(session: &LiveSession) -> AppResult<()> {
    match jump_route(session) {
        JumpRoute::CodexDesktop => jump_to_codex_desktop(session),
        JumpRoute::ZCodeDesktop => jump_to_zcode_desktop(),
        JumpRoute::Cmux => jump_to_cmux(session),
        JumpRoute::Tmux => jump_to_tmux(session),
        JumpRoute::DirectTerminal => jump_to_direct_terminal(session),
    }
}

fn jump_to_zcode_desktop() -> AppResult<()> {
    run_checked(
        Command::new("/usr/bin/open").args(["-a", "ZCode"]),
        "ZCode could not be opened",
    )
}

fn jump_to_codex_desktop(session: &LiveSession) -> AppResult<()> {
    let mut url = url::Url::parse("codex://threads/")
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    url.path_segments_mut()
        .map_err(|_| AppError::InvalidRequest("invalid Codex URL".into()))?
        .push(&session.source_session_id);
    run_checked(
        Command::new("/usr/bin/open").arg(url.as_str()),
        "Codex could not open the source task",
    )
}

fn jump_to_cmux(session: &LiveSession) -> AppResult<()> {
    let context = session
        .jump_context
        .as_ref()
        .ok_or_else(|| AppError::InvalidRequest("cmux jump context is unavailable".into()))?;
    let executable = trusted_cli_executable(context.cmux_executable.as_deref(), "cmux")
        .or_else(|| {
            trusted_cli_executable(
                Some("/Applications/cmux.app/Contents/Resources/bin/cmux"),
                "cmux",
            )
        })
        .ok_or_else(|| AppError::InvalidRequest("cmux command is unavailable".into()))?;
    let socket = context
        .cmux_socket
        .as_deref()
        .filter(|value| valid_socket_path(value))
        .ok_or_else(|| AppError::InvalidRequest("cmux socket is unavailable".into()))?;

    let workspace = context.cmux_workspace_id.as_deref().map(|value| {
        valid_target_id(value)
            .then_some(value)
            .ok_or_else(|| AppError::InvalidRequest("invalid cmux workspace identifier".into()))
    });
    let surface = context.cmux_surface_id.as_deref().map(|value| {
        valid_target_id(value)
            .then_some(value)
            .ok_or_else(|| AppError::InvalidRequest("invalid cmux surface identifier".into()))
    });
    let workspace = workspace.transpose()?;
    let surface = surface.transpose()?;
    if workspace.is_none() && surface.is_none() {
        return Err(AppError::InvalidRequest(
            "cmux target is unavailable".into(),
        ));
    }

    if let Some(workspace) = workspace {
        run_checked(
            Command::new(&executable).args([
                "--socket",
                socket,
                "select-workspace",
                "--workspace",
                workspace,
            ]),
            "cmux could not select the source workspace; allow external socket control in cmux",
        )?;
    }
    if let Some(surface) = surface {
        run_checked(
            Command::new(&executable).args(["--socket", socket, "focus-panel", "--panel", surface]),
            "cmux could not focus the source surface; allow external socket control in cmux",
        )?;
    }
    activate_host_application(Some("cmux"), Some("cmux"))
}

fn jump_to_tmux(session: &LiveSession) -> AppResult<()> {
    let context = session
        .jump_context
        .as_ref()
        .ok_or_else(|| AppError::InvalidRequest("tmux jump context is unavailable".into()))?;
    let executable = trusted_cli_executable(context.tmux_executable.as_deref(), "tmux")
        .or_else(|| trusted_cli_executable(Some("/opt/homebrew/bin/tmux"), "tmux"))
        .or_else(|| trusted_cli_executable(Some("/usr/local/bin/tmux"), "tmux"))
        .ok_or_else(|| AppError::InvalidRequest("tmux command is unavailable".into()))?;
    let socket = context
        .tmux_socket
        .as_deref()
        .filter(|value| valid_socket_path(value))
        .ok_or_else(|| AppError::InvalidRequest("tmux socket is unavailable".into()))?;
    let pane = context
        .tmux_pane
        .as_deref()
        .filter(|value| valid_tmux_pane(value))
        .ok_or_else(|| AppError::InvalidRequest("tmux pane is unavailable".into()))?;

    run_checked(
        Command::new(&executable).args(["-S", socket, "select-window", "-t", pane]),
        "tmux could not select the source window",
    )?;
    run_checked(
        Command::new(&executable).args(["-S", socket, "select-pane", "-t", pane]),
        "tmux could not select the source pane",
    )?;
    activate_host_application(
        context.terminal_kind.as_deref(),
        context.host_app_name.as_deref(),
    )
}

fn jump_to_direct_terminal(session: &LiveSession) -> AppResult<()> {
    let process_id = session
        .process_id
        .ok_or_else(|| AppError::InvalidRequest("source process is no longer available".into()))?;
    if let Some(expected) = session
        .jump_context
        .as_ref()
        .and_then(|context| context.process_started_at.as_deref())
    {
        validate_process_identity(process_id, expected)?;
    }
    let processes = process_snapshot()?;
    if !processes.contains_key(&process_id) {
        return Err(AppError::InvalidRequest(
            "source process is no longer available".into(),
        ));
    }
    let detected_host = host_application_for_process(process_id, &processes);
    let context = session.jump_context.as_ref();
    let terminal_kind = context
        .and_then(|item| item.terminal_kind.as_deref())
        .or_else(|| detected_host.as_deref().and_then(terminal_kind_for_app));
    let host_app_name = detected_host
        .as_deref()
        .or_else(|| context.and_then(|item| item.host_app_name.as_deref()));
    let tty = context.and_then(|item| item.tty.as_deref());

    match terminal_kind {
        Some("terminal") if tty.is_some_and(valid_tty) => focus_terminal_tty(tty.unwrap()),
        Some("iterm") if tty.is_some_and(valid_tty) => focus_iterm_tty(tty.unwrap()),
        _ => activate_host_application(terminal_kind, host_app_name),
    }
}

fn run_checked(command: &mut Command, message: &str) -> AppResult<()> {
    let status = command
        .status()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::InvalidRequest(message.into()))
    }
}

fn trusted_cli_executable(candidate: Option<&str>, expected_name: &str) -> Option<PathBuf> {
    let path = Path::new(candidate?);
    let trusted_prefix = path.starts_with("/opt/homebrew/bin")
        || path.starts_with("/usr/local/bin")
        || path.starts_with("/usr/bin")
        || path.starts_with("/opt/local/bin")
        || path.starts_with("/run/current-system/sw/bin")
        || path.starts_with("/nix/store")
        || dirs::home_dir().is_some_and(|home| path.starts_with(home.join(".nix-profile/bin")))
        || path.starts_with("/Applications/cmux.app/Contents/Resources/bin");
    (path.is_absolute()
        && trusted_prefix
        && path.file_name().and_then(|name| name.to_str()) == Some(expected_name)
        && path.is_file())
    .then(|| path.to_path_buf())
}

fn valid_socket_path(value: &str) -> bool {
    let path = Path::new(value);
    value.len() <= 512
        && path.is_absolute()
        && !path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
}

fn valid_target_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_:.%".contains(character))
}

fn valid_tmux_pane(value: &str) -> bool {
    value.strip_prefix('%').is_some_and(|suffix| {
        !suffix.is_empty() && suffix.chars().all(|char| char.is_ascii_digit())
    })
}

fn valid_tty(value: &str) -> bool {
    value.starts_with("/dev/tty")
        && value.len() <= 64
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '/' | '_'))
}

fn validate_process_identity(process_id: u32, expected: &str) -> AppResult<()> {
    let output = Command::new("/bin/ps")
        .args(["-p", &process_id.to_string(), "-o", "lstart="])
        .output()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    let observed = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || observed.is_empty() || observed != expected.trim() {
        return Err(AppError::InvalidRequest(
            "source process identity no longer matches".into(),
        ));
    }
    Ok(())
}

fn process_snapshot() -> AppResult<HashMap<u32, ProcessRecord>> {
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    if !output.status.success() {
        return Err(AppError::InvalidRequest(
            "process list is unavailable".into(),
        ));
    }
    Ok(parse_process_snapshot(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_process_snapshot(output: &str) -> HashMap<u32, ProcessRecord> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let process_id = fields.next()?.parse::<u32>().ok()?;
            let parent_id = fields.next()?.parse::<u32>().ok()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            if command.is_empty() {
                return None;
            }
            Some((process_id, ProcessRecord { parent_id, command }))
        })
        .collect()
}

fn host_application_for_process(
    mut process_id: u32,
    processes: &HashMap<u32, ProcessRecord>,
) -> Option<String> {
    for _ in 0..24 {
        let process = processes.get(&process_id)?;
        if let Some(name) = app_name_from_command(&process.command)
            && trusted_host_application(&name)
        {
            return Some(name);
        }
        if process.parent_id <= 1 || process.parent_id == process_id {
            break;
        }
        process_id = process.parent_id;
    }
    None
}

fn app_name_from_command(command: &str) -> Option<String> {
    let lower = command.to_ascii_lowercase();
    let marker = ".app/";
    let index = lower.find(marker)?;
    let app_path = &command[..index + 4];
    Path::new(app_path)
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

fn trusted_host_application(name: &str) -> bool {
    matches!(
        name,
        "Terminal"
            | "iTerm"
            | "iTerm2"
            | "Warp"
            | "cmux"
            | "Ghostty"
            | "WezTerm"
            | "kitty"
            | "Alacritty"
            | "Hyper"
            | "Rio"
            | "Visual Studio Code"
            | "Cursor"
            | "Codex"
            | "ZCode"
    )
}

fn terminal_kind_for_app(name: &str) -> Option<&'static str> {
    match name {
        "Terminal" => Some("terminal"),
        "iTerm" | "iTerm2" => Some("iterm"),
        "Warp" => Some("warp"),
        "cmux" => Some("cmux"),
        "Ghostty" => Some("ghostty"),
        "WezTerm" => Some("wezterm"),
        "kitty" => Some("kitty"),
        "Alacritty" => Some("alacritty"),
        "Hyper" => Some("hyper"),
        "Rio" => Some("rio"),
        "Visual Studio Code" => Some("vscode"),
        "Cursor" => Some("cursor"),
        "Codex" => Some("codex"),
        _ => None,
    }
}

fn host_application_name(
    terminal_kind: Option<&str>,
    observed: Option<&str>,
) -> Option<&'static str> {
    match terminal_kind {
        Some("terminal") => Some("Terminal"),
        Some("iterm") => Some("iTerm"),
        Some("warp") => Some("Warp"),
        Some("cmux") => Some("cmux"),
        Some("ghostty") => Some("Ghostty"),
        Some("wezterm") => Some("WezTerm"),
        Some("kitty") => Some("kitty"),
        Some("alacritty") => Some("Alacritty"),
        Some("hyper") => Some("Hyper"),
        Some("rio") => Some("Rio"),
        Some("cursor") => Some("Cursor"),
        Some("vscode") if observed == Some("Cursor") => Some("Cursor"),
        Some("vscode") => Some("Visual Studio Code"),
        Some("codex") => Some("Codex"),
        Some("zcode") => Some("ZCode"),
        _ => observed
            .filter(|name| trusted_host_application(name))
            .and_then(|name| match name {
                "iTerm2" => Some("iTerm"),
                "Terminal" => Some("Terminal"),
                "iTerm" => Some("iTerm"),
                "Warp" => Some("Warp"),
                "cmux" => Some("cmux"),
                "Ghostty" => Some("Ghostty"),
                "WezTerm" => Some("WezTerm"),
                "kitty" => Some("kitty"),
                "Alacritty" => Some("Alacritty"),
                "Hyper" => Some("Hyper"),
                "Rio" => Some("Rio"),
                "Visual Studio Code" => Some("Visual Studio Code"),
                "Cursor" => Some("Cursor"),
                "Codex" => Some("Codex"),
                "ZCode" => Some("ZCode"),
                _ => None,
            }),
    }
}

fn activate_host_application(terminal_kind: Option<&str>, observed: Option<&str>) -> AppResult<()> {
    let application = host_application_name(terminal_kind, observed).ok_or_else(|| {
        AppError::InvalidRequest("the source terminal could not be identified".into())
    })?;
    run_checked(
        Command::new("/usr/bin/open").args(["-a", application]),
        "the source terminal could not be activated",
    )
}

fn focus_terminal_tty(tty: &str) -> AppResult<()> {
    let script = format!(
        r#"tell application "Terminal"
repeat with windowItem in windows
repeat with tabItem in tabs of windowItem
if tty of tabItem is "{tty}" then
set selected tab of windowItem to tabItem
set index of windowItem to 1
activate
return "ok"
end if
end repeat
end repeat
error "terminal session not found"
end tell"#
    );
    run_checked(
        Command::new("/usr/bin/osascript").args(["-e", &script]),
        "the source Terminal tab could not be focused",
    )
}

fn focus_iterm_tty(tty: &str) -> AppResult<()> {
    let script = format!(
        r#"tell application "iTerm2"
repeat with windowItem in windows
repeat with tabItem in tabs of windowItem
repeat with sessionItem in sessions of tabItem
if tty of sessionItem is "{tty}" then
tell sessionItem to select
tell tabItem to select
set index of windowItem to 1
activate
return "ok"
end if
end repeat
end repeat
end repeat
error "iTerm session not found"
end tell"#
    );
    run_checked(
        Command::new("/usr/bin/osascript").args(["-e", &script]),
        "the source iTerm session could not be focused",
    )
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
        .and_then(normalize_live_timestamp)
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    let source_session_id = string_field(payload, &["session_id", "sessionId", "thread_id"])
        .filter(|value| !value.is_empty())
        .or_else(|| {
            object
                .get("process_id")
                .and_then(Value::as_u64)
                .map(|pid| format!("process-{pid}"))
        })?;
    let source_session_id = crate::privacy::safe_opaque_identifier(&source_session_id);
    let event_name = string_field(payload, &["hook_event_name", "event", "type"])
        .unwrap_or_else(|| "Unknown".into());
    if !hook_event_belongs_to_provider(provider, payload, &event_name) {
        return None;
    }
    let cwd = string_field(payload, &["cwd", "working_directory"]).unwrap_or_default();
    let project_label = project_label_from_cwd(&cwd);
    let raw_tool = string_field(payload, &["tool_name", "tool"]).unwrap_or_default();
    let tool = if raw_tool.is_empty() {
        String::new()
    } else {
        crate::privacy::sanitize_tool_name(&raw_tool)
    };
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
    let occurred_at = string_field(
        payload,
        &["timestamp", "occurred_at", "occurredAt", "created_at"],
    )
    .as_deref()
    .and_then(normalize_live_timestamp)
    .unwrap_or(received_at);
    let event_order_key = source_event_order_key(payload, &event_name, status, phase, &tool);
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
        conversation_title: None,
        status: status.into(),
        phase: phase.into(),
        started_at: occurred_at.clone(),
        updated_at: occurred_at.clone(),
        activity_ended_at: (!matches!(status, "idle" | "running")).then(|| occurred_at.clone()),
        event_order_key,
        waiting_reason,
        actions: vec![LiveAction {
            kind: action_kind.into(),
            label: if tool.is_empty() {
                visible_live_event_label(&event_name, status).into()
            } else {
                tool
            },
            occurred_at,
        }],
        process_id: object
            .get("process_id")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        origin: object
            .get("origin")
            .and_then(Value::as_str)
            .map(str::to_string),
        pulse: WorkPulse::default(),
        jump_context: object
            .get("jump_context")
            .cloned()
            .and_then(|value| serde_json::from_value::<LiveJumpContext>(value).ok()),
    };
    let raw = serde_json::to_string(envelope).ok()?;
    Some((session, raw, event_name))
}

fn observed_live_event_from_envelope(
    envelope: &Value,
    session: &LiveSession,
    payload_json: String,
    event_name: String,
    observed_at: String,
) -> ObservedLiveEvent {
    let payload = envelope.get("payload").and_then(Value::as_object);
    let source_time = payload
        .and_then(|payload| {
            string_field(
                payload,
                &["timestamp", "occurred_at", "occurredAt", "created_at"],
            )
        })
        .as_deref()
        .and_then(normalize_live_timestamp);
    let occurred_at = source_time
        .clone()
        .unwrap_or_else(|| session.updated_at.clone());
    let explicit_source_event_id = payload.and_then(|payload| {
        string_field(
            payload,
            &[
                "event_id",
                "eventId",
                "request_id",
                "requestId",
                "tool_use_id",
                "toolUseId",
            ],
        )
    });
    let source_sequence = payload.and_then(|payload| {
        ["sequence", "event_sequence", "eventSequence", "seq"]
            .iter()
            .find_map(|key| payload.get(*key))
            .and_then(|value| {
                value
                    .as_u64()
                    .and_then(|value| i64::try_from(value).ok())
                    .or_else(|| value.as_str().and_then(|value| value.parse::<i64>().ok()))
            })
    });
    let safe_tool = payload
        .and_then(|payload| string_field(payload, &["tool_name", "tool"]))
        .as_deref()
        .and_then(safe_tool_name)
        .unwrap_or_default();
    let source_event_id = explicit_source_event_id.or_else(|| {
        source_time.as_ref().map(|source_time| {
            format!(
                "derived-{}",
                crate::privacy::stable_hash(&format!(
                    "{}|{}|{}|{}|{}|{}|{}",
                    session.agent,
                    session.source_session_id,
                    event_name,
                    source_time,
                    source_sequence
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    safe_tool,
                    session.phase,
                ))
            )
        })
    });
    let source_event_fingerprint = payload.map(|payload| {
        let notification_kind = string_field(payload, &["notification_type", "notificationType"])
            .filter(|value| matches!(value.as_str(), "permission_prompt" | "idle_prompt"))
            .unwrap_or_default();
        crate::privacy::stable_hash(&format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
            session.agent,
            session.source_session_id,
            event_name,
            session.status,
            session.phase,
            notification_kind,
            source_time.as_deref().unwrap_or_default(),
            source_sequence
                .map(|value| value.to_string())
                .unwrap_or_default(),
            safe_tool,
        ))
    });
    ObservedLiveEvent {
        occurred_at,
        observed_at,
        agent: session.agent.clone(),
        source_session_id: session.source_session_id.clone(),
        source_event_id,
        source_sequence,
        source_event_fingerprint,
        event_name,
        project_label: session.project_label.clone(),
        payload_json,
        status: session.status.clone(),
        phase: Some(session.phase.clone()),
    }
}

fn observed_live_event_from_codex_metadata(
    session: &LiveSession,
    signal: &CodexMetadataSignal,
    observed_at: String,
) -> ObservedLiveEvent {
    let event_name = if signal.status == "completed" {
        "SessionEnd"
    } else if signal.status == "paused" {
        "TurnPaused"
    } else if signal.phase == "compacting" {
        "ContextCompact"
    } else if signal.action_kind == "tool" {
        "PreToolUse"
    } else {
        "MetadataProgress"
    };
    let identity = crate::privacy::stable_hash(&format!(
        "{}|{}|{}|{}|{}|{}",
        session.agent,
        session.source_session_id,
        event_name,
        signal.occurred_at,
        signal.status,
        signal.phase,
    ));
    let payload_json = json!({
        "source": "codex-metadata",
        "event": event_name,
        "status": signal.status,
        "phase": signal.phase,
    })
    .to_string();

    ObservedLiveEvent {
        occurred_at: signal.occurred_at.clone(),
        observed_at,
        agent: session.agent.clone(),
        source_session_id: session.source_session_id.clone(),
        source_event_id: Some(format!("metadata-{identity}")),
        source_sequence: None,
        source_event_fingerprint: Some(identity),
        event_name: event_name.into(),
        project_label: session.project_label.clone(),
        payload_json,
        status: signal.status.clone(),
        phase: Some(signal.phase.clone()),
    }
}

fn fold_codex_memory_activity(
    sessions: &Arc<RwLock<HashMap<String, LiveSession>>>,
    auxiliary_sessions: &Arc<RwLock<HashMap<String, String>>>,
    envelope: &Value,
    mut incoming: LiveSession,
) -> Option<LiveSession> {
    if incoming.agent != "codex" || !is_codex_memory_activity(envelope) {
        return Some(incoming);
    }
    if incoming.status == "completed" {
        return None;
    }
    let auxiliary_source_id = incoming.source_session_id.clone();
    let aliased_parent = auxiliary_sessions
        .read()
        .ok()
        .and_then(|aliases| aliases.get(&auxiliary_source_id).cloned());
    let parent = sessions.read().ok().and_then(|active| {
        aliased_parent
            .as_ref()
            .and_then(|id| active.get(id))
            .filter(|session| session_accepts_auxiliary_activity(session))
            .cloned()
            .or_else(|| {
                active
                    .values()
                    .filter(|session| {
                        session.agent == "codex"
                            && session.project_label != "memories"
                            && session_accepts_auxiliary_activity(session)
                            && same_process_context(session, &incoming)
                            && recent_before(session, &incoming)
                    })
                    .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
                    .cloned()
            })
    })?;
    if aliased_parent.is_none()
        && let Ok(mut aliases) = auxiliary_sessions.write()
    {
        aliases.insert(auxiliary_source_id, parent.id.clone());
    }
    incoming.id = parent.id;
    incoming.source_session_id = parent.source_session_id;
    incoming.project_label = parent.project_label;
    incoming.conversation_title = parent.conversation_title.or(incoming.conversation_title);
    incoming.started_at = parent.started_at;
    incoming.status = "running".into();
    incoming.phase = "reading".into();
    incoming.activity_ended_at = None;
    incoming.waiting_reason = None;
    incoming.process_id = parent.process_id.or(incoming.process_id);
    incoming.origin = parent.origin.or(incoming.origin);
    incoming.jump_context = parent.jump_context.or(incoming.jump_context);
    incoming.actions = vec![LiveAction {
        kind: "memory".into(),
        label: "Memory".into(),
        occurred_at: incoming.updated_at.clone(),
    }];
    Some(incoming)
}

fn session_accepts_auxiliary_activity(session: &LiveSession) -> bool {
    matches!(session.status.as_str(), "running" | "idle")
}

fn is_codex_memory_activity(envelope: &Value) -> bool {
    let Some(cwd) = envelope
        .get("payload")
        .and_then(Value::as_object)
        .and_then(|payload| string_field(payload, &["cwd", "working_directory"]))
    else {
        return false;
    };
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    Path::new(&cwd).starts_with(codex_home.join("memories"))
}

fn same_process_context(parent: &LiveSession, auxiliary: &LiveSession) -> bool {
    match (parent.process_id, auxiliary.process_id) {
        (Some(parent), Some(auxiliary)) => parent == auxiliary,
        _ => parent.origin == auxiliary.origin,
    }
}

fn recent_before(parent: &LiveSession, auxiliary: &LiveSession) -> bool {
    let Ok(parent_time) = DateTime::parse_from_rfc3339(&parent.updated_at) else {
        return false;
    };
    let Ok(auxiliary_time) = DateTime::parse_from_rfc3339(&auxiliary.updated_at) else {
        return false;
    };
    let age = auxiliary_time.signed_duration_since(parent_time);
    age >= Duration::zero() && age <= Duration::minutes(5)
}

fn hook_event_belongs_to_provider(
    provider: &str,
    payload: &Map<String, Value>,
    event_name: &str,
) -> bool {
    match provider {
        "claude" | "claude-code" => {
            let cursor_payload = [
                "cursor_version",
                "composer_mode",
                "conversation_id",
                "workspace_roots",
            ]
            .iter()
            .any(|key| payload.contains_key(*key));
            !cursor_payload
                && CLAUDE_HOOKS
                    .iter()
                    .any(|(supported_event, _, _)| *supported_event == event_name)
        }
        "codex" => CODEX_EVENT_NAMES.contains(&event_name),
        _ => false,
    }
}

pub(crate) fn project_label_from_cwd(cwd: &str) -> String {
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

fn register_codex_transcript(
    watches: &Arc<Mutex<HashMap<String, CodexTranscriptWatch>>>,
    envelope: &Value,
    session: &LiveSession,
) {
    if session.agent != "codex" {
        return;
    }
    let Some(path) = envelope
        .get("payload")
        .and_then(Value::as_object)
        .and_then(|payload| string_field(payload, &["transcript_path", "transcriptPath"]))
        .and_then(|path| validated_codex_transcript_path(&path))
    else {
        return;
    };
    let Ok(metadata) = fs::metadata(&path) else {
        return;
    };
    let offset = metadata.len().saturating_sub(MAX_TRANSCRIPT_TAIL_BYTES);
    if let Ok(mut guard) = watches.lock() {
        let replace = guard
            .get(&session.id)
            .is_none_or(|existing| existing.path != path);
        if replace {
            guard.insert(
                session.id.clone(),
                CodexTranscriptWatch {
                    path,
                    offset,
                    discard_partial_line: offset > 0,
                    initialized: false,
                    collaboration_mode: CodexCollaborationMode::Default,
                },
            );
        }
    }
}

fn discover_codex_transcripts(
    watches: &Arc<Mutex<HashMap<String, CodexTranscriptWatch>>>,
    sessions: &Arc<RwLock<HashMap<String, LiveSession>>>,
) -> bool {
    let watched_paths = watches
        .lock()
        .map(|guard| {
            guard
                .iter()
                .map(|(session_id, watch)| (watch.path.clone(), (session_id.clone(), watch.offset)))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let active_ids = sessions
        .read()
        .map(|guard| guard.keys().cloned().collect::<HashSet<_>>())
        .unwrap_or_default();
    let mut changed = false;

    for path in recent_codex_transcript_paths() {
        if let Some((session_id, offset)) = watched_paths.get(&path)
            && (active_ids.contains(session_id)
                || fs::metadata(&path)
                    .ok()
                    .is_none_or(|metadata| metadata.len() <= *offset))
        {
            continue;
        }
        let Some(bootstrap) = bootstrap_codex_transcript(&path) else {
            continue;
        };
        if let Ok(mut guard) = watches.lock() {
            guard.insert(bootstrap.session_id.clone(), bootstrap.watch);
        }
        let Some(session) = bootstrap.session else {
            continue;
        };
        if let Ok(mut guard) = sessions.write() {
            let should_insert = guard.get(&session.id).is_none_or(|existing| {
                !matches!(existing.status.as_str(), "waiting" | "error")
                    && existing.updated_at < session.updated_at
            });
            if should_insert {
                guard.insert(session.id.clone(), session);
                changed = true;
            }
        }
    }
    changed
}

fn recent_codex_transcript_paths() -> Vec<PathBuf> {
    let Some(codex_home) = codex_home_dir() else {
        return Vec::new();
    };
    let root = codex_home.join("sessions");
    let now = SystemTime::now();
    let mut stack = vec![(root, 0_u8)];
    let mut candidates = Vec::new();
    while let Some((directory, depth)) = stack.pop() {
        if depth > 6 {
            continue;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            if now.duration_since(modified).unwrap_or_default() > CODEX_DISCOVERY_WINDOW {
                continue;
            }
            candidates.push((modified, path));
        }
    }
    candidates.sort_by_key(|item| std::cmp::Reverse(item.0));
    candidates
        .into_iter()
        .take(MAX_DISCOVERED_TRANSCRIPTS)
        .filter_map(|(_, path)| path.canonicalize().ok())
        .collect()
}

fn bootstrap_codex_transcript(path: &Path) -> Option<CodexTranscriptBootstrap> {
    let codex_home = codex_home_dir()?;
    bootstrap_codex_transcript_from(path, &codex_home)
}

fn bootstrap_codex_transcript_from(
    path: &Path,
    codex_home: &Path,
) -> Option<CodexTranscriptBootstrap> {
    let sessions_root = codex_home.join("sessions").canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    if !path.starts_with(&sessions_root) || !path.is_file() {
        return None;
    }
    let metadata = fs::metadata(&path).ok()?;
    let mut first_line = Vec::new();
    BufReader::new(fs::File::open(&path).ok()?)
        .take(MAX_SESSION_META_BYTES + 1)
        .read_until(b'\n', &mut first_line)
        .ok()?;
    if first_line.is_empty()
        || first_line.len() as u64 > MAX_SESSION_META_BYTES
        || !first_line.ends_with(b"\n")
    {
        return None;
    }
    let meta = serde_json::from_slice::<Value>(&first_line).ok()?;
    if meta.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let payload = meta.get("payload").and_then(Value::as_object)?;
    let source_session_id = payload.get("id").and_then(Value::as_str)?.trim();
    if !valid_target_id(source_session_id) || codex_metadata_is_auxiliary(payload, codex_home) {
        return None;
    }
    let source_session_id = source_session_id.to_string();
    let session_id = crate::privacy::stable_hash(&format!("codex:{source_session_id}"));
    let cwd = string_field(payload, &["cwd"]).unwrap_or_default();
    let origin = payload
        .get("originator")
        .and_then(Value::as_str)
        .filter(|value| value.to_ascii_lowercase().contains("desktop"))
        .map(|_| "desktop".to_string());

    let offset = metadata.len().saturating_sub(MAX_TRANSCRIPT_TAIL_BYTES);
    let mut file = fs::File::open(&path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_TRANSCRIPT_TAIL_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let mut lines = bytes.split(|byte| *byte == b'\n');
    if offset > 0 {
        let _ = lines.next();
    }
    let mut collaboration_mode = CodexCollaborationMode::Default;
    let mut turn_open = false;
    let mut saw_turn_boundary = false;
    let mut turn_started_at = None;
    let mut latest_signal = None;
    for line in lines {
        if line.is_empty() || line.len() > MAX_TRANSCRIPT_LINE_BYTES {
            continue;
        }
        if let Ok(record) = serde_json::from_slice::<Value>(line) {
            let payload_type = record
                .get("payload")
                .and_then(Value::as_object)
                .and_then(|payload| payload.get("type"))
                .and_then(Value::as_str);
            match payload_type {
                Some("task_started") => {
                    saw_turn_boundary = true;
                    turn_open = true;
                    turn_started_at = record
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .filter(|value| DateTime::parse_from_rfc3339(value).is_ok())
                        .map(str::to_string);
                    latest_signal = None;
                }
                Some("task_complete") => {
                    saw_turn_boundary = true;
                    turn_open = false;
                }
                Some("turn_aborted") => {
                    saw_turn_boundary = true;
                    turn_open = false;
                }
                _ => {}
            }
        }
        if let Some(signal) = codex_metadata_signal(line, &mut collaboration_mode) {
            latest_signal = Some(signal);
        }
    }
    if !saw_turn_boundary
        && latest_signal
            .as_ref()
            .is_some_and(|signal| signal.status == "running")
    {
        if let Some((open, started_at)) = latest_codex_turn_boundary(&path, metadata.len()) {
            turn_open = open;
            turn_started_at = started_at;
        } else {
            // Long turns can push task_started beyond the bounded tail. A whitelisted
            // running signal cannot occur after task_complete, so restoring it is safe.
            turn_open = true;
        }
    }
    let watch = CodexTranscriptWatch {
        path,
        offset: metadata.len(),
        discard_partial_line: false,
        initialized: true,
        collaboration_mode,
    };
    let terminal_non_running = latest_signal
        .as_ref()
        .is_some_and(|signal| matches!(signal.status.as_str(), "paused" | "waiting" | "error"));
    if !turn_open && !terminal_non_running {
        return Some(CodexTranscriptBootstrap {
            session_id,
            session: None,
            watch,
        });
    }
    let signal = latest_signal.unwrap_or_else(|| {
        let occurred_at = turn_started_at.clone().unwrap_or_else(|| {
            DateTime::<Utc>::from(metadata.modified().unwrap_or_else(|_| SystemTime::now()))
                .to_rfc3339()
        });
        metadata_signal(
            collaboration_phase(collaboration_mode),
            "running",
            if collaboration_mode == CodexCollaborationMode::Plan {
                "plan"
            } else {
                "think"
            },
            collaboration_phase(collaboration_mode),
            occurred_at,
        )
    });
    let started_at = turn_started_at.unwrap_or_else(|| signal.occurred_at.clone());
    let event_order_key = metadata_event_order_key(&signal);
    let session = LiveSession {
        id: session_id.clone(),
        source_session_id,
        agent: "codex".into(),
        project_label: project_label_from_cwd(&cwd),
        conversation_title: None,
        status: signal.status.clone(),
        phase: signal.phase.clone(),
        started_at,
        updated_at: signal.occurred_at.clone(),
        activity_ended_at: (!matches!(signal.status.as_str(), "idle" | "running"))
            .then(|| signal.occurred_at.clone()),
        event_order_key,
        waiting_reason: signal.waiting_reason.clone(),
        actions: vec![LiveAction {
            kind: signal.action_kind,
            label: signal.action_label,
            occurred_at: signal.occurred_at,
        }],
        process_id: None,
        origin,
        pulse: WorkPulse::default(),
        jump_context: None,
    };
    Some(CodexTranscriptBootstrap {
        session_id,
        session: Some(session),
        watch,
    })
}

fn latest_codex_turn_boundary(path: &Path, length: u64) -> Option<(bool, Option<String>)> {
    let offset = length.saturating_sub(MAX_TRANSCRIPT_BOUNDARY_BYTES);
    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_TRANSCRIPT_BOUNDARY_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let mut lines = bytes.split(|byte| *byte == b'\n');
    if offset > 0 {
        let _ = lines.next();
    }
    let mut boundary = None;
    for line in lines {
        if line.is_empty() || line.len() > MAX_TRANSCRIPT_LINE_BYTES {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let payload_type = record
            .get("payload")
            .and_then(Value::as_object)
            .and_then(|payload| payload.get("type"))
            .and_then(Value::as_str);
        match payload_type {
            Some("task_started") => {
                let started_at = record
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .filter(|value| DateTime::parse_from_rfc3339(value).is_ok())
                    .map(str::to_string);
                boundary = Some((true, started_at));
            }
            Some("task_complete") => boundary = Some((false, None)),
            Some("turn_aborted") => boundary = Some((false, None)),
            _ => {}
        }
    }
    boundary
}

fn codex_metadata_is_auxiliary(payload: &Map<String, Value>, codex_home: &Path) -> bool {
    if payload
        .get("thread_source")
        .and_then(Value::as_str)
        .is_some_and(|value| matches!(value, "subagent" | "memory"))
        || payload
            .get("source")
            .and_then(Value::as_str)
            .is_some_and(|value| matches!(value, "subagent" | "memory"))
        || payload
            .get("source")
            .and_then(Value::as_object)
            .is_some_and(|source| source.contains_key("subagent") || source.contains_key("memory"))
    {
        return true;
    }
    string_field(payload, &["cwd"])
        .is_some_and(|cwd| Path::new(&cwd).starts_with(codex_home.join("memories")))
}

fn codex_home_dir() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
}

fn validated_codex_transcript_path(path: &str) -> Option<PathBuf> {
    let root = codex_home_dir()?.join("sessions").canonicalize().ok()?;
    let path = Path::new(path).canonicalize().ok()?;
    (path.starts_with(&root) && path.is_file()).then_some(path)
}

fn poll_codex_transcripts(
    watches: &Arc<Mutex<HashMap<String, CodexTranscriptWatch>>>,
    sessions: &Arc<RwLock<HashMap<String, LiveSession>>>,
) -> Vec<(String, CodexMetadataSignal, bool)> {
    let active_ids = sessions
        .read()
        .map(|guard| guard.keys().cloned().collect::<HashSet<_>>())
        .unwrap_or_default();
    let Ok(mut guard) = watches.lock() else {
        return Vec::new();
    };
    let mut updates = Vec::new();
    for (session_id, watch) in guard.iter_mut() {
        if !active_ids.contains(session_id) {
            continue;
        }
        let Ok(metadata) = fs::metadata(&watch.path) else {
            continue;
        };
        if metadata.len() < watch.offset {
            watch.offset = metadata.len().saturating_sub(MAX_TRANSCRIPT_TAIL_BYTES);
            watch.discard_partial_line = watch.offset > 0;
            watch.initialized = false;
            watch.collaboration_mode = CodexCollaborationMode::Default;
        }
        if metadata.len() == watch.offset {
            continue;
        }
        let Ok(mut file) = fs::File::open(&watch.path) else {
            continue;
        };
        if file.seek(SeekFrom::Start(watch.offset)).is_err() {
            continue;
        }
        let mut bytes = Vec::new();
        if file
            .take(MAX_TRANSCRIPT_READ_BYTES)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.is_empty()
        {
            continue;
        }
        let consumed = if bytes.ends_with(b"\n") {
            bytes.len()
        } else if let Some(last_newline) = bytes.iter().rposition(|byte| *byte == b'\n') {
            last_newline + 1
        } else if bytes.len() as u64 == MAX_TRANSCRIPT_READ_BYTES {
            bytes.len()
        } else {
            0
        };
        if consumed == 0 {
            continue;
        }
        watch.offset = watch.offset.saturating_add(consumed as u64);
        let mut lines = bytes[..consumed].split(|byte| *byte == b'\n');
        if watch.discard_partial_line {
            let _ = lines.next();
            watch.discard_partial_line = false;
        }
        let mut latest_signal = None;
        for line in lines {
            if line.is_empty() || line.len() > MAX_TRANSCRIPT_LINE_BYTES {
                continue;
            }
            if let Some(signal) = codex_metadata_signal(line, &mut watch.collaboration_mode) {
                latest_signal = Some(signal);
            }
        }
        if let Some(signal) = latest_signal {
            updates.push((session_id.clone(), signal, watch.initialized));
        }
        watch.initialized = true;
    }
    updates
}

fn prune_transcript_watches(
    watches: &Arc<Mutex<HashMap<String, CodexTranscriptWatch>>>,
    sessions: &Arc<RwLock<HashMap<String, LiveSession>>>,
) {
    let active = sessions
        .read()
        .map(|sessions| sessions.keys().cloned().collect::<HashSet<_>>())
        .unwrap_or_default();
    if let Ok(mut guard) = watches.lock() {
        guard.retain(|session_id, watch| {
            active.contains(session_id)
                || fs::metadata(&watch.path)
                    .ok()
                    .and_then(|metadata| metadata.modified().ok())
                    .is_some_and(|modified| {
                        SystemTime::now()
                            .duration_since(modified)
                            .unwrap_or_default()
                            <= CODEX_DISCOVERY_WINDOW
                    })
        });
    }
}

fn codex_metadata_signal(
    line: &[u8],
    collaboration_mode: &mut CodexCollaborationMode,
) -> Option<CodexMetadataSignal> {
    let record = serde_json::from_slice::<Value>(line).ok()?;
    let record_type = record.get("type").and_then(Value::as_str)?;
    let payload = record.get("payload").and_then(Value::as_object)?;
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let occurred_at = record
        .get("timestamp")
        .and_then(Value::as_str)
        .filter(|timestamp| DateTime::parse_from_rfc3339(timestamp).is_ok())
        .map(str::to_string)
        .unwrap_or_else(|| Utc::now().to_rfc3339());

    if record_type == "turn_context" {
        *collaboration_mode = match payload
            .get("collaboration_mode")
            .and_then(Value::as_object)
            .and_then(|mode| mode.get("mode"))
            .and_then(Value::as_str)
        {
            Some("plan") => CodexCollaborationMode::Plan,
            _ => CodexCollaborationMode::Default,
        };
        return Some(metadata_signal(
            collaboration_phase(*collaboration_mode),
            "running",
            "session",
            collaboration_phase(*collaboration_mode),
            occurred_at,
        ));
    }

    if payload_type == "task_complete" {
        return Some(metadata_signal(
            "completed",
            "completed",
            "completed",
            "Completed",
            occurred_at,
        ));
    }
    if payload_type == "turn_aborted" {
        let mut signal = metadata_signal("paused", "paused", "paused", "Turn paused", occurred_at);
        signal.waiting_reason = Some("turn-paused".into());
        return Some(signal);
    }
    if payload_type.contains("compact") {
        return Some(metadata_signal(
            "compacting",
            "running",
            "compact",
            "Context compacted",
            occurred_at,
        ));
    }
    if matches!(payload_type, "patch_apply_begin" | "patch_apply_end") {
        return Some(metadata_signal(
            "editing",
            "running",
            "edit",
            "apply_patch",
            occurred_at,
        ));
    }
    if matches!(
        payload_type,
        "task_started" | "agent_reasoning" | "reasoning"
    ) || (record_type == "response_item" && payload_type == "reasoning")
    {
        let phase = collaboration_phase(*collaboration_mode);
        return Some(metadata_signal(
            phase,
            "running",
            if phase == "planning" { "plan" } else { "think" },
            phase,
            occurred_at,
        ));
    }
    if record_type == "response_item"
        && matches!(payload_type, "function_call" | "custom_tool_call")
    {
        let tool = payload
            .get("name")
            .and_then(Value::as_str)
            .and_then(safe_tool_name)
            .unwrap_or_else(|| "tool".into());
        let (phase, action_kind) = phase_for("", &tool, "running");
        return Some(metadata_signal(
            phase,
            "running",
            action_kind,
            &tool,
            occurred_at,
        ));
    }
    None
}

fn collaboration_phase(mode: CodexCollaborationMode) -> &'static str {
    match mode {
        CodexCollaborationMode::Plan => "planning",
        CodexCollaborationMode::Default => "thinking",
    }
}

fn safe_tool_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()
        && name.len() <= 80
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-.".contains(character)))
    .then(|| name.to_string())
}

fn metadata_signal(
    phase: &str,
    status: &str,
    action_kind: &str,
    action_label: &str,
    occurred_at: String,
) -> CodexMetadataSignal {
    CodexMetadataSignal {
        phase: phase.into(),
        status: status.into(),
        action_kind: action_kind.into(),
        action_label: action_label.into(),
        occurred_at,
        waiting_reason: None,
    }
}

fn merge_codex_metadata(
    sessions: &Arc<RwLock<HashMap<String, LiveSession>>>,
    session_id: &str,
    signal: CodexMetadataSignal,
    live_append: bool,
) -> Option<String> {
    let mut guard = sessions.write().ok()?;
    let session = guard.get_mut(session_id)?;
    let incoming_order_key = metadata_event_order_key(&signal);
    if !live_event_is_newer(
        &session.updated_at,
        &session.event_order_key,
        &signal.occurred_at,
        &incoming_order_key,
    ) {
        return None;
    }
    if signal.status == "completed" && !live_append && session.status != "completed" {
        return None;
    }
    if matches!(session.status.as_str(), "waiting" | "error")
        && signal.status == "running"
        && session.waiting_reason.as_deref() != Some("turn-paused")
    {
        return None;
    }
    let previous_status = session.status.clone();
    let previous_was_active = matches!(previous_status.as_str(), "idle" | "running");
    if matches!(signal.status.as_str(), "idle" | "running") {
        session.activity_ended_at = None;
    } else if previous_was_active {
        session.activity_ended_at = Some(signal.occurred_at.clone());
    }
    if matches!(
        previous_status.as_str(),
        "waiting" | "error" | "paused" | "completed"
    ) && signal.status == "running"
    {
        session.started_at = signal.occurred_at.clone();
    }
    if previous_status == "completed" && signal.status == "running" {
        session.actions.clear();
    }
    session.status = signal.status;
    session.phase = signal.phase;
    session.updated_at = signal.occurred_at.clone();
    session.event_order_key = incoming_order_key;
    session.waiting_reason = signal.waiting_reason;
    session.actions.push(LiveAction {
        kind: signal.action_kind,
        label: signal.action_label,
        occurred_at: signal.occurred_at,
    });
    if session.actions.len() > 3 {
        session.actions.drain(0..session.actions.len() - 3);
    }
    if previous_status != session.status && session.status == "completed" {
        Some(session.status.clone())
    } else {
        None
    }
}

fn merge_session(
    sessions: &Arc<RwLock<HashMap<String, LiveSession>>>,
    mut incoming: LiveSession,
) -> Option<String> {
    let mut guard = sessions.write().ok()?;
    let previous_status = guard.get(&incoming.id).map(|item| item.status.clone());
    if let Some(existing) = guard.get_mut(&incoming.id) {
        if !live_event_is_newer(
            &existing.updated_at,
            &existing.event_order_key,
            &incoming.updated_at,
            &incoming.event_order_key,
        ) {
            if matches!(
                existing.status.as_str(),
                "waiting" | "error" | "paused" | "completed"
            ) && matches!(incoming.status.as_str(), "idle" | "running")
                && live_event_time_order(&existing.started_at, &incoming.started_at)
                    == std::cmp::Ordering::Less
            {
                existing.started_at = incoming.started_at.clone();
            }
            merge_live_actions(existing, std::mem::take(&mut incoming.actions));
            return None;
        }
        let auxiliary_refresh = incoming.status == "running"
            && !incoming.actions.is_empty()
            && incoming
                .actions
                .iter()
                .all(|action| action.kind == "memory");
        if auxiliary_refresh && !session_accepts_auxiliary_activity(existing) {
            return None;
        }
        if matches!(
            existing.status.as_str(),
            "waiting" | "error" | "paused" | "completed"
        ) && matches!(incoming.status.as_str(), "idle" | "running")
        {
            existing.started_at = incoming.updated_at.clone();
            if existing.status == "completed" {
                existing.actions.clear();
            }
        }
        let existing_was_active = matches!(existing.status.as_str(), "idle" | "running");
        if matches!(incoming.status.as_str(), "idle" | "running") {
            existing.activity_ended_at = None;
        } else if existing_was_active {
            existing.activity_ended_at = Some(incoming.updated_at.clone());
        }
        existing.updated_at = incoming.updated_at;
        existing.event_order_key = incoming.event_order_key;
        existing.status = incoming.status.clone();
        existing.phase = incoming.phase;
        existing.project_label = incoming.project_label;
        existing.conversation_title = incoming
            .conversation_title
            .or_else(|| existing.conversation_title.clone());
        existing.waiting_reason = incoming.waiting_reason;
        existing.process_id = incoming.process_id.or(existing.process_id);
        existing.origin = incoming.origin.or_else(|| existing.origin.clone());
        existing.jump_context = incoming
            .jump_context
            .or_else(|| existing.jump_context.clone());
        merge_live_actions(existing, incoming.actions);
    } else {
        guard.insert(incoming.id.clone(), incoming.clone());
    }
    if previous_status.as_deref() != Some(incoming.status.as_str())
        && (matches!(incoming.status.as_str(), "waiting" | "error")
            || (incoming.status == "completed" && previous_status.is_some()))
    {
        Some(incoming.status)
    } else {
        None
    }
}

fn merge_live_actions(session: &mut LiveSession, incoming: Vec<LiveAction>) {
    session.actions.extend(incoming);
    session.actions.sort_by(|left, right| {
        compare_live_timestamps(&left.occurred_at, &right.occurred_at)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.label.cmp(&right.label))
    });
    session.actions.dedup_by(|left, right| {
        compare_live_timestamps(&left.occurred_at, &right.occurred_at) == std::cmp::Ordering::Equal
            && left.kind == right.kind
            && left.label == right.label
    });
    if session.actions.len() > 3 {
        session.actions.drain(0..session.actions.len() - 3);
    }
}

fn snapshot_from(
    sessions: &Arc<RwLock<HashMap<String, LiveSession>>>,
    socket_ready: bool,
    completed_sessions: Vec<NotchCompletedSession>,
    database: &Database,
) -> LiveSnapshot {
    snapshot_from_at(
        sessions,
        socket_ready,
        completed_sessions,
        database,
        Utc::now(),
    )
}

fn snapshot_from_at(
    sessions: &Arc<RwLock<HashMap<String, LiveSession>>>,
    socket_ready: bool,
    mut completed_sessions: Vec<NotchCompletedSession>,
    database: &Database,
    now: DateTime<Utc>,
) -> LiveSnapshot {
    let mut items = sessions
        .read()
        .map(|value| value.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    hydrate_conversation_titles(database, &mut items, &mut completed_sessions);
    for session in items
        .iter_mut()
        .chain(completed_sessions.iter_mut().map(|item| &mut item.session))
    {
        session.pulse = work_pulse_at(session, now);
    }
    let attention = database.attention_queue_at(now).unwrap_or_default();
    overlay_attention_pulses(&mut items, &attention);
    for completed in &mut completed_sessions {
        overlay_attention_pulses(std::slice::from_mut(&mut completed.session), &attention);
    }
    sort_live_sessions(&mut items);
    let attention_queue = attention;
    let urgent_session_id = attention_queue
        .first()
        .and_then(|attention| {
            items
                .iter()
                .find(|session| {
                    session.agent == attention.agent
                        && session.source_session_id == attention.source_session_id
                })
                .map(|session| session.id.clone())
        })
        .or_else(|| items.first().map(|item| item.id.clone()));
    let active_count = items
        .iter()
        .filter(|item| matches!(item.status.as_str(), "waiting" | "error" | "running"))
        .count() as u64;
    LiveSnapshot {
        generated_at: now.to_rfc3339(),
        sessions: items,
        completed_sessions,
        urgent_session_id,
        attention_queue,
        active_count,
        hook_status: hook_status(socket_ready),
    }
}

fn overlay_attention_pulses(sessions: &mut [LiveSession], attention: &[AttentionEvent]) {
    for session in sessions {
        let has_stuck = attention.iter().any(|event| {
            event.kind == "stuck"
                && matches!(event.state.as_str(), "open" | "acknowledged")
                && event.agent == session.agent
                && event.source_session_id == session.source_session_id
        });
        if has_stuck && session.pulse.attention_signal.value.as_deref() == Some("none") {
            session.pulse.attention_signal = WorkPulseDimension {
                availability: "available".into(),
                value: Some("stuck".into()),
                evidence_level: "derived".into(),
                source_coverage: "exact".into(),
                age_seconds: None,
            };
        }
    }
}

fn work_pulse_at(session: &LiveSession, now: DateTime<Utc>) -> WorkPulse {
    let live_capability = source_capabilities()
        .iter()
        .find(|capability| capability.agent == session.agent)
        .map(|capability| capability.live_capability)
        .unwrap_or(SourceLiveCapability::None);
    let source_coverage = live_capability.as_str();
    let available = |value: &str, evidence_level: &str| WorkPulseDimension {
        availability: "available".into(),
        value: Some(value.into()),
        evidence_level: evidence_level.into(),
        source_coverage: source_coverage.into(),
        age_seconds: None,
    };
    let unknown = || WorkPulseDimension {
        availability: "unknown".into(),
        value: None,
        evidence_level: "not-recorded".into(),
        source_coverage: source_coverage.into(),
        age_seconds: None,
    };
    let freshness = match DateTime::parse_from_rfc3339(&session.updated_at) {
        Ok(updated_at) => {
            let age_seconds = now
                .signed_duration_since(updated_at.with_timezone(&Utc))
                .num_seconds()
                .max(0) as u64;
            let value = if age_seconds <= WORK_PULSE_FRESH_SECONDS {
                "fresh"
            } else if age_seconds <= WORK_PULSE_LOST_UPDATE_SECONDS {
                "aging"
            } else {
                "lost-update"
            };
            WorkPulseDimension {
                availability: "available".into(),
                value: Some(value.into()),
                evidence_level: "derived".into(),
                source_coverage: source_coverage.into(),
                age_seconds: Some(age_seconds),
            }
        }
        Err(_) => unknown(),
    };

    match live_capability {
        SourceLiveCapability::Exact => WorkPulse {
            lifecycle: available(&session.status, "observed"),
            work_phase: if session.phase.trim().is_empty() {
                unknown()
            } else {
                available(&session.phase, "derived")
            },
            attention_signal: available(
                match session.status.as_str() {
                    "waiting" => "needs-you",
                    "error" => "blocking-error",
                    "completed" => "completion-review",
                    _ => "none",
                },
                "derived",
            ),
            freshness,
        },
        SourceLiveCapability::Experimental => WorkPulse {
            lifecycle: unknown(),
            work_phase: available("recent-activity", "observed"),
            attention_signal: unknown(),
            freshness,
        },
        SourceLiveCapability::None => WorkPulse {
            lifecycle: unknown(),
            work_phase: unknown(),
            attention_signal: unknown(),
            freshness,
        },
    }
}

fn sort_live_sessions(items: &mut [LiveSession]) {
    items.sort_by(|left, right| {
        priority(&right.status)
            .cmp(&priority(&left.status))
            .then_with(|| compare_live_timestamps(&left.started_at, &right.started_at))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn hydrate_conversation_titles(
    database: &Database,
    sessions: &mut [LiveSession],
    completed_sessions: &mut [NotchCompletedSession],
) {
    let sources = sessions
        .iter()
        .chain(completed_sessions.iter().map(|item| &item.session))
        .map(|session| (session.agent.clone(), session.source_session_id.clone()))
        .collect::<Vec<_>>();
    let mut titles = database
        .live_conversation_titles(&sources)
        .unwrap_or_default();
    for (key, value) in codex_conversation_titles(&sources) {
        titles.insert(key, value);
    }
    for session in sessions
        .iter_mut()
        .chain(completed_sessions.iter_mut().map(|item| &mut item.session))
    {
        let key = (session.agent.clone(), session.source_session_id.clone());
        session.conversation_title = titles.get(&key).cloned().filter(|title| {
            !title.eq_ignore_ascii_case(&session.project_label) && title.trim().len() > 1
        });
    }
}

fn codex_conversation_titles(sources: &[(String, String)]) -> HashMap<(String, String), String> {
    let Some(home) = dirs::home_dir() else {
        return HashMap::new();
    };
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    codex_conversation_titles_from(&codex_home, sources)
}

fn codex_conversation_titles_from(
    codex_home: &Path,
    sources: &[(String, String)],
) -> HashMap<(String, String), String> {
    let source_ids = sources
        .iter()
        .filter(|(agent, _)| agent == "codex")
        .map(|(_, source_session_id)| source_session_id.as_str())
        .collect::<HashSet<_>>();
    let mut titles = HashMap::new();
    if let Ok(index) = fs::File::open(codex_home.join("session_index.jsonl")) {
        for line in BufReader::new(index).lines().map_while(Result::ok) {
            let Ok(record) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(source_session_id) = record.get("id").and_then(Value::as_str) else {
                continue;
            };
            if !source_ids.contains(source_session_id) {
                continue;
            }
            let title = record
                .get("thread_name")
                .and_then(Value::as_str)
                .and_then(crate::privacy::sanitize_title);
            if let Some(title) = title {
                titles.insert(("codex".into(), source_session_id.into()), title);
            }
        }
    }
    for path in [
        codex_home.join("state_5.sqlite"),
        codex_home.join("sqlite/state_5.sqlite"),
    ] {
        if !path.is_file() {
            continue;
        }
        let Ok(connection) = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            continue;
        };
        for (agent, source_session_id) in sources {
            if agent != "codex" {
                continue;
            }
            let key = (agent.clone(), source_session_id.clone());
            if titles.contains_key(&key) {
                continue;
            }
            let title = connection
                .query_row(
                    "SELECT COALESCE(NULLIF(name, ''), NULLIF(title, ''))
                     FROM threads WHERE id=?1 LIMIT 1",
                    params![source_session_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()
                .ok()
                .flatten()
                .flatten()
                .or_else(|| {
                    connection
                        .query_row(
                            "SELECT NULLIF(title, '') FROM threads WHERE id=?1 LIMIT 1",
                            params![source_session_id],
                            |row| row.get::<_, Option<String>>(0),
                        )
                        .optional()
                        .ok()
                        .flatten()
                        .flatten()
                })
                .and_then(|value| crate::privacy::sanitize_title(&value));
            if let Some(title) = title {
                titles.insert(key, title);
            }
        }
    }
    titles
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
        "paused" => 1,
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
    if normalized.contains("plan") {
        ("planning", "plan")
    } else if normalized.contains("test")
        || normalized.contains("lint")
        || normalized.contains("check")
    {
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
    } else if matches!(event, "SubagentStart" | "SubagentStop") {
        ("running-tool", "agent")
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

fn normalize_live_timestamp(value: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(value).ok().map(|value| {
        value
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::AutoSi, true)
    })
}

fn visible_live_event_label(event_name: &str, status: &str) -> &'static str {
    if status == "error" {
        return "Error";
    }
    if status == "waiting" {
        return "Needs You";
    }
    match event_name {
        "SessionStart" => "Session Started",
        "UserPromptSubmit" | "Resume" => "Resumed",
        "Stop" | "SessionEnd" => "Completed",
        "TurnPaused" => "Paused",
        "PreCompact" | "PostCompact" | "ContextCompact" => "Compacting",
        "PreToolUse" => "Tool Started",
        "PostToolUse" => "Tool Finished",
        "SubagentStart" => "Agent Started",
        "SubagentStop" => "Agent Finished",
        _ => "Activity",
    }
}

fn live_event_rank(event_name: &str, status: &str) -> u8 {
    match status {
        "idle" => 0,
        "running" if event_name == "PreToolUse" => 1,
        "running" => 2,
        "waiting" => 3,
        "paused" => 4,
        "error" => 5,
        "completed" => 6,
        _ => 2,
    }
}

fn source_event_order_key(
    payload: &Map<String, Value>,
    event_name: &str,
    status: &str,
    phase: &str,
    tool: &str,
) -> String {
    let rank = live_event_rank(event_name, status);
    let context = crate::privacy::stable_hash(&format!("{event_name}|{status}|{phase}|{tool}"));
    if let Some(sequence) = ["sequence", "event_sequence", "eventSequence", "seq"]
        .iter()
        .find_map(|key| payload.get(*key))
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|value| value.parse::<u64>().ok()))
        })
    {
        return format!("sequence:{sequence:020}:{rank:02}:{context}");
    }
    if let Some(source_event_id) = string_field(
        payload,
        &[
            "event_id",
            "eventId",
            "request_id",
            "requestId",
            "tool_use_id",
            "toolUseId",
        ],
    ) {
        return format!(
            "source:{rank:02}:{}",
            crate::privacy::stable_hash(&format!("{source_event_id}|{context}"))
        );
    }
    format!("derived:{rank:02}:{context}")
}

fn metadata_event_order_key(signal: &CodexMetadataSignal) -> String {
    format!(
        "metadata:{}",
        crate::privacy::stable_hash(&format!(
            "{}|{}|{}|{}",
            signal.status, signal.phase, signal.action_kind, signal.action_label
        ))
    )
}

fn live_event_is_newer(
    existing_at: &str,
    existing_key: &str,
    incoming_at: &str,
    incoming_key: &str,
) -> bool {
    match live_event_time_order(existing_at, incoming_at) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => incoming_key > existing_key,
    }
}

fn live_event_time_order(existing_at: &str, incoming_at: &str) -> std::cmp::Ordering {
    compare_live_timestamps(incoming_at, existing_at)
}

fn compare_live_timestamps(left: &str, right: &str) -> std::cmp::Ordering {
    DateTime::parse_from_rfc3339(left)
        .ok()
        .zip(DateTime::parse_from_rfc3339(right).ok())
        .map(|(left, right)| left.cmp(&right))
        .unwrap_or_else(|| left.cmp(right))
}

fn notify_if_background(database: &Database, session: &LiveSession, status: &str) {
    if !notification_allowed_for_origin(session, status) {
        return;
    }
    if source_is_foreground(session) {
        return;
    }
    let Some(claim_token) = database
        .claim_attention_notification(&session.agent, &session.source_session_id, status)
        .unwrap_or(None)
    else {
        return;
    };
    let body = if status == "waiting" {
        format!("{} needs your attention.", provider_label(&session.agent))
    } else if status == "completed" {
        format!("{} finished its task.", provider_label(&session.agent))
    } else {
        format!("{} reported an error.", provider_label(&session.agent))
    };
    let escaped = body.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{escaped}\" with title \"VibeMeter\"");
    let delivered = run_notification_script(&script);
    if delivered {
        if database
            .confirm_attention_notification(&claim_token)
            .is_err()
        {
            eprintln!("VibeMeter could not confirm a delivered attention notification");
        }
    } else if database
        .release_attention_notification(&claim_token)
        .is_err()
    {
        eprintln!("VibeMeter could not release a failed attention notification claim");
    }
}

fn run_notification_script(script: &str) -> bool {
    let Ok(mut child) = Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + ATTENTION_NOTIFICATION_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(StdDuration::from_millis(10));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn notification_allowed_for_origin(session: &LiveSession, status: &str) -> bool {
    session.pulse.lifecycle.availability == "available"
        && (status != "completed" || session.origin.as_deref() == Some("cli"))
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
        return match session.agent.as_str() {
            "codex" => name.contains("codex"),
            "zcode" => name.contains("zcode"),
            _ => false,
        };
    }
    if let Some(context) = session.jump_context.as_ref()
        && let Some(expected) = host_application_name(
            context.terminal_kind.as_deref(),
            context.host_app_name.as_deref(),
        )
    {
        return name.contains(&expected.to_ascii_lowercase());
    }
    [
        "terminal",
        "iterm",
        "warp",
        "cmux",
        "ghostty",
        "wezterm",
        "kitty",
        "alacritty",
        "hyper",
        "rio",
        "visual studio code",
        "cursor",
    ]
    .iter()
    .any(|candidate| name.contains(candidate))
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
    match agent {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "kimi-code" => "Kimi Code",
        "zcode" => "ZCode",
        _ => "Agent",
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
    fn snapshot_exposes_independent_work_pulse_dimensions_without_overclaiming_experimental_live() {
        let temporary = tempdir().expect("tempdir");
        let database = Database::open(temporary.path().join("work-pulse.sqlite"))
            .expect("database should open");
        let now = DateTime::parse_from_rfc3339("2026-08-10T08:00:30Z")
            .expect("fixed clock")
            .with_timezone(&Utc);

        let mut exact = jump_test_session("codex", "desktop", None);
        exact.id = "exact".into();
        exact.status = "waiting".into();
        exact.phase = "needs-you".into();
        exact.updated_at = "2026-08-10T08:00:20Z".into();

        let mut experimental = jump_test_session("kimi-code", "desktop", None);
        experimental.id = "experimental".into();
        experimental.status = "error".into();
        experimental.phase = "error".into();
        experimental.updated_at = "2026-08-10T07:57:30Z".into();

        let sessions = Arc::new(RwLock::new(HashMap::from([
            (exact.id.clone(), exact),
            (experimental.id.clone(), experimental),
        ])));
        let snapshot = snapshot_from_at(&sessions, true, Vec::new(), &database, now);
        let exact = snapshot
            .sessions
            .iter()
            .find(|session| session.id == "exact")
            .expect("exact pulse");
        assert_eq!(exact.pulse.lifecycle.value.as_deref(), Some("waiting"));
        assert_eq!(exact.pulse.work_phase.value.as_deref(), Some("needs-you"));
        assert_eq!(
            exact.pulse.attention_signal.value.as_deref(),
            Some("needs-you")
        );
        assert_eq!(exact.pulse.freshness.value.as_deref(), Some("fresh"));

        let experimental = snapshot
            .sessions
            .iter()
            .find(|session| session.id == "experimental")
            .expect("experimental pulse");
        assert_eq!(experimental.pulse.lifecycle.availability, "unknown");
        assert_eq!(experimental.pulse.lifecycle.value, None);
        assert_eq!(
            experimental.pulse.work_phase.value.as_deref(),
            Some("recent-activity")
        );
        assert_eq!(experimental.pulse.attention_signal.availability, "unknown");
        assert_eq!(experimental.pulse.attention_signal.value, None);
        assert_eq!(
            experimental.pulse.freshness.value.as_deref(),
            Some("lost-update")
        );
    }

    #[test]
    fn high_confidence_stuck_attention_overlays_a_running_exact_pulse() {
        let mut session = jump_test_session("codex", "desktop", None);
        session.id = "stuck-live".into();
        session.source_session_id = "stuck-source".into();
        session.status = "running".into();
        session.phase = "running-tool".into();
        session.pulse = work_pulse_at(&session, Utc::now());
        let attention = crate::models::AttentionEvent {
            id: "attention-stuck".into(),
            kind: "stuck".into(),
            state: "open".into(),
            reason_key: "repeated-operation-loop".into(),
            agent: "codex".into(),
            source_session_id: "stuck-source".into(),
            project_label: "vibemeter".into(),
            opened_at: Utc::now().to_rfc3339(),
            latest_evidence_at: Utc::now().to_rfc3339(),
            expires_at: (Utc::now() + Duration::hours(24)).to_rfc3339(),
            resolved_at: None,
            evidence_level: "derived".into(),
            source_coverage: "exact-lifecycle".into(),
            rule_version: "stuck-detector-1.0.0".into(),
            evidence_count: 3,
            intervention_count: 0,
        };

        overlay_attention_pulses(std::slice::from_mut(&mut session), &[attention]);

        assert_eq!(
            session.pulse.attention_signal.value.as_deref(),
            Some("stuck")
        );
        assert_eq!(session.pulse.attention_signal.evidence_level, "derived");
    }

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
    fn untrusted_event_and_tool_names_never_reach_visible_live_fields() {
        let envelope = json!({
            "provider":"claude",
            "received_at":"2026-08-10T00:00:00Z",
            "payload":{
                "session_id":"private-label-session",
                "hook_event_name":"PermissionRequest",
                "cwd":"/Users/private/project",
                "tool_name":"/Users/private/.secrets/sk-abcdefghijklmnop"
            }
        });
        let (session, _, _) = session_from_envelope(&envelope).expect("private event session");
        let visible = serde_json::to_string(&session).expect("session should serialize");
        assert_eq!(session.actions[0].label, "other");
        assert_eq!(
            session.waiting_reason.as_deref(),
            Some("other needs approval")
        );
        for secret in ["/Users/private", "sk-abcdefghijklmnop"] {
            assert!(!visible.contains(secret));
        }

        let unknown = json!({
            "provider":"claude",
            "received_at":"2026-08-10T00:00:00Z",
            "payload":{
                "session_id":"private-event-session",
                "hook_event_name":"Notification",
                "notification_type":"secret-visible-name",
                "cwd":"/Users/private/project"
            }
        });
        let (session, _, _) = session_from_envelope(&unknown).expect("unknown event session");
        assert_eq!(session.actions[0].label, "Activity");
        assert!(
            !serde_json::to_string(&session)
                .expect("session should serialize")
                .contains("secret-visible-name")
        );
    }

    #[test]
    fn exact_waiting_envelope_preserves_source_and_observation_times() {
        let envelope = json!({
            "provider":"codex",
            "received_at":"2026-08-09T10:00:01Z",
            "payload":{
                "session_id":"codex-waiting",
                "hook_event_name":"PermissionRequest",
                "timestamp":"2026-08-09T10:00:00Z",
                "request_id":"permission-42",
                "cwd":"/Users/private/project",
                "tool_name":"Bash"
            }
        });
        let (session, raw, event_name) = session_from_envelope(&envelope).expect("waiting session");

        let observed = observed_live_event_from_envelope(
            &envelope,
            &session,
            raw,
            event_name,
            "2026-08-09T10:00:02Z".into(),
        );

        assert_eq!(observed.occurred_at, "2026-08-09T10:00:00Z");
        assert_eq!(observed.observed_at, "2026-08-09T10:00:02Z");
        assert_eq!(observed.source_event_id.as_deref(), Some("permission-42"));
        assert_eq!(observed.status, "waiting");
        assert_eq!(observed.phase.as_deref(), Some("needs-you"));
    }

    #[test]
    fn exact_source_envelopes_share_one_private_lifecycle_contract() {
        for (provider, agent) in [("claude", "claude-code"), ("codex", "codex")] {
            let temporary = tempfile::tempdir().expect("temporary directory");
            let database = Database::open(temporary.path().join(format!("{agent}.sqlite")))
                .expect("database should open");
            let event_names = [
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "WAITING",
                "UserPromptSubmit",
                "PreCompact",
                "SubagentStart",
                "SubagentStop",
                "ERROR",
                "UserPromptSubmit",
                "Stop",
            ];
            for (index, source_name) in event_names.iter().enumerate() {
                let event_name = match (*source_name, provider) {
                    ("WAITING", "claude") => "Notification",
                    ("WAITING", _) => "PermissionRequest",
                    ("ERROR", "claude") => "PostToolUseFailure",
                    ("ERROR", _) => "PostToolUse",
                    (value, _) => value,
                };
                let timestamp = format!("2026-08-10T00:02:{index:02}Z");
                let mut payload = json!({
                    "session_id":"exact-source-session",
                    "hook_event_name":event_name,
                    "event_id":format!("{agent}-{index}"),
                    "timestamp":timestamp,
                    "cwd":"/Users/private/super-secret-project",
                    "tool_name":"Bash",
                    "prompt":"super-secret prompt",
                    "command":"super-secret command",
                    "tool_arguments":{"path":"/Users/private/super-secret-project"}
                });
                if *source_name == "WAITING" && provider == "claude" {
                    payload["notification_type"] = json!("idle_prompt");
                }
                if *source_name == "ERROR" && provider == "codex" {
                    payload["status"] = json!("failed");
                }
                let envelope = json!({
                    "provider":provider,
                    "received_at":format!("2026-08-10T00:03:{index:02}Z"),
                    "payload":payload,
                });
                let (session, raw, source_event_name) =
                    session_from_envelope(&envelope).expect("exact envelope should normalize");
                let observed = observed_live_event_from_envelope(
                    &envelope,
                    &session,
                    raw,
                    source_event_name,
                    format!("2026-08-10T00:04:{index:02}Z"),
                );
                database
                    .record_observed_live_event(&observed)
                    .expect("normalized lifecycle should persist");
            }

            let activity = database.live_activity().expect("live activity should load");
            assert_eq!(activity.timeline.len(), event_names.len());
            let public_json = serde_json::to_string(&activity).expect("activity should serialize");
            assert!(!public_json.contains("super-secret prompt"));
            assert!(!public_json.contains("super-secret command"));
            assert!(!public_json.contains("/Users/private"));
        }
    }

    #[test]
    fn source_timestamp_builds_a_stable_private_event_identity() {
        let envelope = |timestamp: &str| {
            json!({
                "provider":"codex",
                "received_at":"2026-08-10T00:05:10Z",
                "payload":{
                    "session_id":"timestamp-session",
                    "hook_event_name":"PreToolUse",
                    "timestamp":timestamp,
                    "cwd":"/Users/private/project",
                    "tool_name":"Bash",
                    "tool_arguments":{"command":"super-secret command"}
                }
            })
        };
        let observed = |envelope: &Value| {
            let (session, raw, event_name) =
                session_from_envelope(envelope).expect("exact envelope should normalize");
            observed_live_event_from_envelope(
                envelope,
                &session,
                raw,
                event_name,
                "2026-08-10T00:05:11Z".into(),
            )
        };
        let first = observed(&envelope("2026-08-10T00:05:00Z"));
        let replay = observed(&envelope("2026-08-10T00:05:00Z"));
        let next = observed(&envelope("2026-08-10T00:05:01Z"));

        assert_eq!(first.source_event_id, replay.source_event_id);
        assert_ne!(first.source_event_id, next.source_event_id);
        let identity = first
            .source_event_id
            .expect("derived identity should exist");
        assert!(!identity.contains("super-secret"));
        assert!(!identity.contains("/Users/private"));
    }

    #[test]
    fn waiting_replay_without_source_id_or_time_keeps_a_stable_fingerprint() {
        let envelope = |received_at: &str| {
            json!({
                "provider":"claude",
                "received_at":received_at,
                "payload":{
                    "session_id":"claude-waiting",
                    "hook_event_name":"Notification",
                    "notification_type":"idle_prompt",
                    "cwd":"/Users/private/project"
                }
            })
        };
        let first_envelope = envelope("2026-08-09T10:00:01Z");
        let second_envelope = envelope("2026-08-09T10:00:02Z");
        let (first_session, first_raw, first_name) =
            session_from_envelope(&first_envelope).expect("first replay session");
        let (second_session, second_raw, second_name) =
            session_from_envelope(&second_envelope).expect("second replay session");

        let first = observed_live_event_from_envelope(
            &first_envelope,
            &first_session,
            first_raw,
            first_name,
            "2026-08-09T10:00:03Z".into(),
        );
        let second = observed_live_event_from_envelope(
            &second_envelope,
            &second_session,
            second_raw,
            second_name,
            "2026-08-09T10:00:04Z".into(),
        );

        assert_eq!(first.source_event_id, None);
        assert_eq!(
            first.source_event_fingerprint,
            second.source_event_fingerprint
        );
        assert_ne!(first.occurred_at, second.occurred_at);
        assert_ne!(first.observed_at, second.observed_at);
    }

    #[test]
    fn cursor_events_routed_through_claude_settings_do_not_create_phantom_claude_sessions() {
        let cursor_event = json!({
            "provider":"claude",
            "received_at":Utc::now().to_rfc3339(),
            "payload":{
                "hook_event_name":"sessionStart",
                "session_id":"cursor-composer",
                "conversation_id":"cursor-composer",
                "composer_mode":"agent",
                "cursor_version":"3.12.30",
                "workspace_roots":[]
            }
        });
        assert!(session_from_envelope(&cursor_event).is_none());

        let claude_event = json!({
            "provider":"claude",
            "received_at":Utc::now().to_rfc3339(),
            "payload":{
                "hook_event_name":"SessionStart",
                "session_id":"real-claude",
                "cwd":"/tmp/project"
            }
        });
        let (session, _, _) = session_from_envelope(&claude_event).expect("Claude session");
        assert_eq!(session.agent, "claude-code");
        assert_eq!(session.status, "idle");
    }

    #[test]
    fn codex_memory_subtasks_fold_into_the_parent_instance() {
        let now = Utc::now();
        let parent = LiveSession {
            id: "parent-id".into(),
            source_session_id: "parent-source".into(),
            agent: "codex".into(),
            project_label: "vibemeter".into(),
            conversation_title: None,
            status: "running".into(),
            phase: "thinking".into(),
            started_at: (now - Duration::minutes(10)).to_rfc3339(),
            updated_at: (now - Duration::seconds(8)).to_rfc3339(),
            activity_ended_at: None,
            event_order_key: String::new(),
            waiting_reason: None,
            actions: Vec::new(),
            process_id: Some(42),
            origin: Some("desktop".into()),
            pulse: WorkPulse::default(),
            jump_context: None,
        };
        let sessions = Arc::new(RwLock::new(HashMap::from([(
            parent.id.clone(),
            parent.clone(),
        )])));
        let aliases = Arc::new(RwLock::new(HashMap::new()));
        let memory_root = dirs::home_dir()
            .expect("home")
            .join(".codex/memories")
            .to_string_lossy()
            .to_string();
        let started = json!({
            "provider":"codex",
            "received_at":now.to_rfc3339(),
            "process_id":42,
            "origin":"desktop",
            "payload":{
                "session_id":"memory-child",
                "hook_event_name":"SessionStart",
                "cwd":memory_root
            }
        });
        let (child, _, _) = session_from_envelope(&started).expect("memory child");
        let folded = fold_codex_memory_activity(&sessions, &aliases, &started, child)
            .expect("memory activity should have a parent");
        assert_eq!(folded.id, parent.id);
        assert_eq!(folded.source_session_id, parent.source_session_id);
        assert_eq!(folded.project_label, "vibemeter");
        assert_eq!(folded.status, "running");
        assert_eq!(folded.phase, "reading");
        assert_eq!(folded.actions[0].kind, "memory");

        let ended = json!({
            "provider":"codex",
            "received_at":(now + Duration::seconds(4)).to_rfc3339(),
            "process_id":42,
            "origin":"desktop",
            "payload":{
                "session_id":"memory-child",
                "hook_event_name":"SessionEnd",
                "cwd":dirs::home_dir().expect("home").join(".codex/memories")
            }
        });
        let (child, _, _) = session_from_envelope(&ended).expect("memory child end");
        assert!(
            fold_codex_memory_activity(&sessions, &aliases, &ended, child).is_none(),
            "terminal memory activity must not refresh the parent as running"
        );

        sessions
            .write()
            .expect("sessions")
            .get_mut(&parent.id)
            .expect("parent")
            .status = "completed".into();
        let (late_child, _, _) = session_from_envelope(&ended).expect("late memory child");
        assert!(
            fold_codex_memory_activity(&sessions, &aliases, &ended, late_child).is_none(),
            "a remembered memory alias must not revive a completed parent"
        );
        assert!(merge_session(&sessions, folded).is_none());
        assert_eq!(
            sessions
                .read()
                .expect("sessions")
                .get(&parent.id)
                .expect("parent")
                .status,
            "completed"
        );
    }

    #[test]
    fn standalone_codex_memory_subtasks_do_not_create_instances() {
        let envelope = json!({
            "provider":"codex",
            "received_at":Utc::now().to_rfc3339(),
            "process_id":42,
            "origin":"desktop",
            "payload":{
                "session_id":"orphan-memory-child",
                "hook_event_name":"SessionStart",
                "cwd":dirs::home_dir().expect("home").join(".codex/memories")
            }
        });
        let (child, _, _) = session_from_envelope(&envelope).expect("memory child");
        assert!(
            fold_codex_memory_activity(
                &Arc::new(RwLock::new(HashMap::new())),
                &Arc::new(RwLock::new(HashMap::new())),
                &envelope,
                child,
            )
            .is_none()
        );
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
    fn codex_sidebar_title_precedes_the_original_conversation_title() {
        let directory = tempdir().expect("tempdir");
        let source_session_id = "019fb361-9f23-7412-97c1-9218c487a191";
        let database = Connection::open(directory.path().join("state_5.sqlite"))
            .expect("Codex state database");
        database
            .execute_batch(
                "CREATE TABLE threads(id TEXT PRIMARY KEY, name TEXT, title TEXT NOT NULL);
                 INSERT INTO threads(id, name, title) VALUES(
                    '019fb361-9f23-7412-97c1-9218c487a191',
                    NULL,
                    'notch 弹出栏在任务过多时，显示会不正常。总长度需要改为动态调整的'
                 );",
            )
            .expect("original conversation title");
        drop(database);
        fs::write(
            directory.path().join("session_index.jsonl"),
            format!(
                "{{\"id\":\"{source_session_id}\",\"thread_name\":\"初始标题\"}}\n\
                 {{\"id\":\"another-thread\",\"thread_name\":\"不可泄漏\"}}\n\
                 {{\"id\":\"{source_session_id}\",\"thread_name\":\"修复 Notch 弹出栏动态宽度\"}}\n"
            ),
        )
        .expect("session index");

        let titles = codex_conversation_titles_from(
            directory.path(),
            &[
                ("codex".into(), source_session_id.into()),
                ("claude-code".into(), "another-thread".into()),
            ],
        );
        assert_eq!(
            titles
                .get(&("codex".into(), source_session_id.into()))
                .map(String::as_str),
            Some("修复 Notch 弹出栏动态宽度")
        );
        assert!(!titles.contains_key(&("claude-code".into(), "another-thread".into())));
    }

    #[test]
    fn codex_metadata_listener_reads_only_whitelisted_state() {
        let timestamp = Utc::now().to_rfc3339();
        let turn = serde_json::to_vec(&json!({
            "timestamp": timestamp,
            "type": "turn_context",
            "payload": {
                "collaboration_mode": {"mode": "plan"},
                "developer_instructions": "private-text-must-not-propagate"
            }
        }))
        .expect("turn context");
        let mut mode = CodexCollaborationMode::Default;
        let signal = codex_metadata_signal(&turn, &mut mode).expect("plan signal");
        assert_eq!(mode, CodexCollaborationMode::Plan);
        assert_eq!(signal.phase, "planning");
        assert_eq!(signal.status, "running");
        assert!(!format!("{signal:?}").contains("private-text"));

        let tool = serde_json::to_vec(&json!({
            "timestamp": Utc::now().to_rfc3339(),
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "update_plan",
                "arguments": "private-arguments-must-not-propagate"
            }
        }))
        .expect("tool");
        let signal = codex_metadata_signal(&tool, &mut mode).expect("tool signal");
        assert_eq!(signal.phase, "planning");
        assert_eq!(signal.action_label, "update_plan");
        assert!(!format!("{signal:?}").contains("private-arguments"));
    }

    #[test]
    fn metadata_completion_and_tool_names_are_safely_classified() {
        let mut mode = CodexCollaborationMode::Default;
        let completed = serde_json::to_vec(&json!({
            "type": "event_msg",
            "payload": {"type": "task_complete", "last_agent_message": "private"}
        }))
        .expect("complete");
        let signal = codex_metadata_signal(&completed, &mut mode).expect("complete signal");
        assert_eq!(signal.status, "completed");
        assert_eq!(signal.phase, "completed");
        assert!(!format!("{signal:?}").contains("private"));

        let aborted = serde_json::to_vec(&json!({
            "type": "event_msg",
            "timestamp": "2026-08-04T14:06:34.278Z",
            "payload": {"type": "turn_aborted"}
        }))
        .expect("aborted turn");
        let signal = codex_metadata_signal(&aborted, &mut mode).expect("paused signal");
        assert_eq!(signal.status, "paused");
        assert_eq!(signal.phase, "paused");
        assert_eq!(signal.action_kind, "paused");
        assert_eq!(signal.waiting_reason.as_deref(), Some("turn-paused"));

        assert_eq!(
            safe_tool_name("apply_patch").as_deref(),
            Some("apply_patch")
        );
        assert!(safe_tool_name("../unsafe/path").is_none());
        assert_eq!(
            phase_for("PreToolUse", "EnterPlanMode", "running").0,
            "planning"
        );
        assert_eq!(
            phase_for("PreToolUse", "update_plan", "running").0,
            "planning"
        );
    }

    #[test]
    fn active_codex_transcript_bootstraps_live_session_without_prompt_text() {
        let directory = tempdir().expect("tempdir");
        let codex_home = directory.path().join(".codex");
        let sessions = codex_home.join("sessions/2026/08/02");
        fs::create_dir_all(&sessions).expect("sessions");
        let project = directory.path().join("sample-project");
        fs::create_dir_all(&project).expect("project");
        let path = sessions.join("rollout-active.jsonl");
        let started_at = "2026-08-02T03:51:34.430Z";
        let records = [
            json!({
                "timestamp":"2026-08-02T03:50:00Z",
                "type":"session_meta",
                "payload":{
                    "id":"019fb7b1-0bb1-7e61-8a76-e82529d1b96e",
                    "cwd":project,
                    "source":"vscode",
                    "originator":"Codex Desktop",
                    "thread_source":"user"
                }
            }),
            json!({
                "timestamp":started_at,
                "type":"event_msg",
                "payload":{"type":"task_started"}
            }),
            json!({
                "timestamp":"2026-08-02T03:51:35Z",
                "type":"event_msg",
                "payload":{"type":"user_message","message":"private-prompt-must-not-propagate"}
            }),
            json!({
                "timestamp":"2026-08-02T03:51:36Z",
                "type":"response_item",
                "payload":{
                    "type":"function_call",
                    "name":"apply_patch",
                    "arguments":"private-tool-arguments-must-not-propagate"
                }
            }),
        ];
        let fixture = records
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(&path, fixture).expect("fixture");

        let bootstrap = bootstrap_codex_transcript_from(&path, &codex_home).expect("bootstrap");
        let session = bootstrap.session.expect("active session");
        assert_eq!(
            session.source_session_id,
            "019fb7b1-0bb1-7e61-8a76-e82529d1b96e"
        );
        assert_eq!(session.project_label, "sample-project");
        assert_eq!(session.status, "running");
        assert_eq!(session.phase, "editing");
        assert_eq!(session.started_at, started_at);
        assert_eq!(session.origin.as_deref(), Some("desktop"));
        assert_eq!(session.actions[0].label, "apply_patch");
        let visible = format!("{session:?}");
        assert!(!visible.contains("private-prompt"));
        assert!(!visible.contains("private-tool-arguments"));

        let long_path = sessions.join("rollout-long-active.jsonl");
        let mut long_records = vec![records[0].to_string(), records[1].to_string()];
        for index in 0..6 {
            long_records.push(
                json!({
                    "timestamp":format!("2026-08-02T03:51:4{index}Z"),
                    "type":"event_msg",
                    "payload":{"type":"user_message","message":"x".repeat(220_000)}
                })
                .to_string(),
            );
        }
        long_records.push(records[3].to_string());
        fs::write(&long_path, long_records.join("\n") + "\n").expect("long fixture");
        let long_bootstrap = bootstrap_codex_transcript_from(&long_path, &codex_home)
            .expect("long bootstrap")
            .session
            .expect("long active session");
        assert_eq!(long_bootstrap.started_at, started_at);
        assert_eq!(long_bootstrap.actions[0].label, "apply_patch");
    }

    #[test]
    fn completed_and_subagent_codex_transcripts_do_not_bootstrap_live_sessions() {
        let directory = tempdir().expect("tempdir");
        let codex_home = directory.path().join(".codex");
        let sessions = codex_home.join("sessions/2026/08/02");
        fs::create_dir_all(&sessions).expect("sessions");
        let completed_path = sessions.join("rollout-completed.jsonl");
        let completed = [
            json!({
                "type":"session_meta",
                "payload":{
                    "id":"completed-thread",
                    "cwd":"/tmp/project",
                    "thread_source":"user"
                }
            }),
            json!({"type":"event_msg","payload":{"type":"task_started"}}),
            json!({"type":"event_msg","payload":{"type":"task_complete"}}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
            + "\n";
        fs::write(&completed_path, completed).expect("completed fixture");
        let completed = bootstrap_codex_transcript_from(&completed_path, &codex_home)
            .expect("completed bootstrap");
        assert!(completed.session.is_none());

        let paused_path = sessions.join("rollout-paused.jsonl");
        let paused = [
            json!({
                "type":"session_meta",
                "payload":{
                    "id":"paused-thread",
                    "cwd":"/tmp/project",
                    "thread_source":"user"
                }
            }),
            json!({"type":"event_msg","payload":{"type":"task_started"}}),
            json!({"type":"event_msg","payload":{"type":"turn_aborted"}}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
            + "\n";
        fs::write(&paused_path, paused).expect("paused fixture");
        let paused = bootstrap_codex_transcript_from(&paused_path, &codex_home)
            .expect("paused bootstrap")
            .session
            .expect("paused session");
        assert_eq!(paused.status, "paused");
        assert_eq!(paused.phase, "paused");
        assert_eq!(paused.waiting_reason.as_deref(), Some("turn-paused"));

        let subagent_path = sessions.join("rollout-subagent.jsonl");
        let subagent = json!({
            "type":"session_meta",
            "payload":{
                "id":"subagent-thread",
                "cwd":"/tmp/project",
                "thread_source":"subagent",
                "source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent"}}}
            }
        })
        .to_string()
            + "\n";
        fs::write(&subagent_path, subagent).expect("subagent fixture");
        assert!(bootstrap_codex_transcript_from(&subagent_path, &codex_home).is_none());
    }

    #[test]
    fn paused_codex_session_can_resume_on_a_new_turn() {
        let mut paused = jump_test_session("codex", "desktop", None);
        paused.id = "paused-session".into();
        paused.status = "paused".into();
        paused.phase = "paused".into();
        paused.started_at = "2026-08-04T13:00:00Z".into();
        paused.updated_at = "2026-08-04T13:05:00Z".into();
        paused.event_order_key = "metadata:paused".into();
        paused.waiting_reason = Some("turn-paused".into());
        let session_id = paused.id.clone();
        let sessions = Arc::new(RwLock::new(HashMap::from([(session_id.clone(), paused)])));
        let signal = metadata_signal(
            "thinking",
            "running",
            "think",
            "Thinking",
            "2026-08-04T14:07:00Z".into(),
        );

        assert!(merge_codex_metadata(&sessions, &session_id, signal, true).is_none());
        let session_guard = sessions.read().expect("sessions");
        let resumed = session_guard.get(&session_id).expect("resumed session");
        assert_eq!(resumed.status, "running");
        assert_eq!(resumed.waiting_reason, None);
        assert_eq!(resumed.started_at, "2026-08-04T14:07:00Z");
    }

    #[test]
    fn codex_pause_metadata_becomes_a_private_canonical_lifecycle_event() {
        let session = jump_test_session("codex", "desktop", None);
        let mut signal = metadata_signal(
            "paused",
            "paused",
            "paused",
            "private prompt text",
            "2026-08-10T00:06:00Z".into(),
        );
        signal.waiting_reason = Some("turn-paused".into());

        let observed = observed_live_event_from_codex_metadata(
            &session,
            &signal,
            "2026-08-10T00:06:01Z".into(),
        );

        assert_eq!(observed.event_name, "TurnPaused");
        assert_eq!(observed.status, "paused");
        assert_eq!(observed.phase.as_deref(), Some("paused"));
        assert!(!observed.payload_json.contains("private prompt text"));
    }

    #[test]
    fn waiting_hook_session_starts_a_fresh_cycle_when_work_resumes() {
        let mut waiting = jump_test_session("claude-code", "cli", None);
        waiting.id = "waiting-session".into();
        waiting.status = "waiting".into();
        waiting.phase = "needs-you".into();
        waiting.started_at = "2026-08-04T13:00:00Z".into();
        waiting.updated_at = "2026-08-04T13:05:00Z".into();
        waiting.waiting_reason = Some("Permission required".into());
        let session_id = waiting.id.clone();
        let sessions = Arc::new(RwLock::new(HashMap::from([(session_id.clone(), waiting)])));
        let mut resumed = jump_test_session("claude-code", "cli", None);
        resumed.id = session_id.clone();
        resumed.status = "running".into();
        resumed.phase = "thinking".into();
        resumed.started_at = "2026-08-04T14:00:00Z".into();
        resumed.updated_at = "2026-08-04T14:00:00Z".into();
        resumed.waiting_reason = None;

        assert!(merge_session(&sessions, resumed).is_none());
        let session_guard = sessions.read().expect("sessions");
        let resumed = session_guard.get(&session_id).expect("resumed session");
        assert_eq!(resumed.status, "running");
        assert_eq!(resumed.started_at, "2026-08-04T14:00:00Z");
    }

    #[test]
    fn repeated_waiting_events_preserve_the_first_activity_stop_time() {
        let mut running = jump_test_session("codex", "cli", None);
        running.id = "repeated-waiting".into();
        running.status = "running".into();
        running.phase = "thinking".into();
        running.started_at = "2026-08-04T13:00:00Z".into();
        running.updated_at = "2026-08-04T13:02:00Z".into();
        running.activity_ended_at = None;
        let sessions = Arc::new(RwLock::new(HashMap::from([(running.id.clone(), running)])));

        let mut first_waiting = jump_test_session("codex", "cli", None);
        first_waiting.id = "repeated-waiting".into();
        first_waiting.status = "waiting".into();
        first_waiting.phase = "needs-you".into();
        first_waiting.started_at = "2026-08-04T13:00:00Z".into();
        first_waiting.updated_at = "2026-08-04T13:03:00Z".into();
        first_waiting.activity_ended_at = Some(first_waiting.updated_at.clone());
        first_waiting.waiting_reason = Some("Permission required".into());
        merge_session(&sessions, first_waiting);

        let mut repeated_waiting = jump_test_session("codex", "cli", None);
        repeated_waiting.id = "repeated-waiting".into();
        repeated_waiting.status = "waiting".into();
        repeated_waiting.phase = "needs-you".into();
        repeated_waiting.started_at = "2026-08-04T13:00:00Z".into();
        repeated_waiting.updated_at = "2026-08-04T13:08:00Z".into();
        repeated_waiting.activity_ended_at = Some(repeated_waiting.updated_at.clone());
        repeated_waiting.waiting_reason = Some("Permission required".into());
        merge_session(&sessions, repeated_waiting);

        let guard = sessions.read().expect("sessions should lock");
        let waiting = guard.get("repeated-waiting").expect("waiting session");
        assert_eq!(waiting.updated_at, "2026-08-04T13:08:00Z");
        assert_eq!(
            waiting.activity_ended_at.as_deref(),
            Some("2026-08-04T13:03:00Z")
        );
    }

    #[test]
    fn priority_matches_product_contract() {
        assert!(priority("waiting") > priority("error"));
        assert!(priority("error") > priority("running"));
        assert!(priority("running") > priority("completed"));
        assert!(priority("running") > priority("paused"));
    }

    #[test]
    fn running_phase_updates_do_not_reorder_projects() {
        let mut first = jump_test_session("codex", "cli", None);
        first.id = "first".into();
        first.status = "running".into();
        first.phase = "thinking".into();
        first.started_at = "2026-07-31T10:00:00Z".into();
        first.updated_at = "2026-07-31T10:05:00Z".into();
        let mut second = first.clone();
        second.id = "second".into();
        second.phase = "running-tool".into();
        second.started_at = "2026-07-31T10:01:00Z".into();
        second.updated_at = "2026-07-31T10:06:00Z".into();

        let mut sessions = vec![second.clone(), first.clone()];
        sort_live_sessions(&mut sessions);
        assert_eq!(
            sessions
                .iter()
                .map(|session| session.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );

        first.phase = "running-tool".into();
        first.updated_at = "2026-07-31T10:07:00Z".into();
        second.phase = "thinking".into();
        let mut sessions = vec![second.clone(), first];
        sort_live_sessions(&mut sessions);
        assert_eq!(
            sessions
                .iter()
                .map(|session| session.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );

        sessions
            .iter_mut()
            .find(|session| session.id == "second")
            .expect("second session")
            .status = "waiting".into();
        sort_live_sessions(&mut sessions);
        assert_eq!(sessions[0].id, "second");
    }

    #[test]
    fn out_of_order_and_duplicate_hooks_converge_on_one_live_card() {
        let envelope = |sequence: u64, event_name: &str, timestamp: &str| {
            json!({
                "provider":"codex",
                "received_at":"2026-08-10T00:10:10Z",
                "payload":{
                    "session_id":"ordered-session",
                    "hook_event_name":event_name,
                    "timestamp":timestamp,
                    "sequence":sequence,
                    "cwd":"/Users/private/project"
                }
            })
        };
        let session = |value: Value| {
            session_from_envelope(&value)
                .expect("exact envelope should normalize")
                .0
        };
        let earlier = session(envelope(0, "UserPromptSubmit", "2026-08-10T00:10:00Z"));
        let same_time_lower = session(envelope(1, "UserPromptSubmit", "2026-08-10T00:10:01Z"));
        let later = session(envelope(2, "PermissionRequest", "2026-08-10T00:10:01Z"));
        let duplicate = later.clone();

        let first_order = Arc::new(RwLock::new(HashMap::new()));
        merge_session(&first_order, earlier.clone());
        merge_session(&first_order, same_time_lower.clone());
        merge_session(&first_order, later.clone());
        merge_session(&first_order, duplicate);
        merge_session(&first_order, earlier.clone());

        let reverse_order = Arc::new(RwLock::new(HashMap::new()));
        merge_session(&reverse_order, later);
        merge_session(&reverse_order, same_time_lower);
        merge_session(&reverse_order, earlier);

        let mut rendered = Vec::new();
        for sessions in [first_order, reverse_order] {
            let guard = sessions.read().expect("sessions should lock");
            assert_eq!(guard.len(), 1);
            let current = guard.values().next().expect("live card should exist");
            assert_eq!(current.status, "waiting");
            assert_eq!(current.updated_at, "2026-08-10T00:10:01Z");
            assert_eq!(current.actions.len(), 3);
            let public_json = serde_json::to_string(current).expect("card should serialize");
            assert!(!public_json.contains("eventOrderKey"));
            rendered.push(public_json);
        }
        assert_eq!(rendered[0], rendered[1]);
    }

    #[test]
    fn shared_tool_id_and_timestamp_converge_on_the_terminal_tool_state() {
        let envelope = |event_name: &str, status: Option<&str>| {
            json!({
                "provider":"codex",
                "received_at":"2026-08-10T00:11:00Z",
                "payload":{
                    "session_id":"shared-tool-card",
                    "hook_event_name":event_name,
                    "timestamp":"2026-08-10T00:11:00Z",
                    "tool_use_id":"tool-use-1",
                    "tool_name":"Bash",
                    "status":status,
                    "cwd":"/Users/private/project"
                }
            })
        };
        let session = |value: Value| {
            session_from_envelope(&value)
                .expect("tool envelope should normalize")
                .0
        };
        let started = session(envelope("PreToolUse", None));
        let failed = session(envelope("PostToolUse", Some("failed")));

        for order in [
            vec![started.clone(), failed.clone()],
            vec![failed.clone(), started.clone()],
        ] {
            let sessions = Arc::new(RwLock::new(HashMap::new()));
            for item in order {
                merge_session(&sessions, item);
            }
            let guard = sessions.read().expect("sessions should lock");
            let current = guard.values().next().expect("live card should exist");
            assert_eq!(guard.len(), 1);
            assert_eq!(current.status, "error");
            assert_eq!(current.phase, "error");
            assert_eq!(current.actions.len(), 2);
        }
    }

    #[test]
    fn equivalent_offset_timestamps_deduplicate_actions_and_stabilize_session_order() {
        let mut first = jump_test_session("codex", "cli", None);
        first.id = "a".into();
        first.started_at = "2026-08-10T00:00:00Z".into();
        first.actions = vec![LiveAction {
            kind: "tool".into(),
            label: "Bash".into(),
            occurred_at: "2026-08-10T00:00:00Z".into(),
        }];
        merge_live_actions(
            &mut first,
            vec![LiveAction {
                kind: "tool".into(),
                label: "Bash".into(),
                occurred_at: "2026-08-10T08:00:00+08:00".into(),
            }],
        );
        assert_eq!(first.actions.len(), 1);

        let mut second = first.clone();
        second.id = "b".into();
        second.started_at = "2026-08-10T08:00:00+08:00".into();
        let mut sessions = vec![second, first];
        sort_live_sessions(&mut sessions);
        assert_eq!(
            sessions
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    fn jump_test_session(
        agent: &str,
        origin: &str,
        jump_context: Option<LiveJumpContext>,
    ) -> LiveSession {
        LiveSession {
            id: "jump-test".into(),
            source_session_id: "source-thread".into(),
            agent: agent.into(),
            project_label: "vibemeter".into(),
            conversation_title: None,
            status: "completed".into(),
            phase: "completed".into(),
            started_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            activity_ended_at: None,
            event_order_key: String::new(),
            waiting_reason: None,
            actions: Vec::new(),
            process_id: Some(42),
            origin: Some(origin.into()),
            pulse: WorkPulse::default(),
            jump_context,
        }
    }

    #[test]
    fn jump_routes_distinguish_desktop_cmux_tmux_and_direct_terminals() {
        let desktop = jump_test_session("codex", "desktop", None);
        assert_eq!(jump_route(&desktop), JumpRoute::CodexDesktop);

        let cmux = jump_test_session(
            "claude-code",
            "cli",
            Some(LiveJumpContext {
                cmux_workspace_id: Some("workspace:3".into()),
                cmux_surface_id: Some("surface:7".into()),
                tmux_socket: Some("/tmp/tmux-501/default".into()),
                tmux_pane: Some("%4".into()),
                ..LiveJumpContext::default()
            }),
        );
        assert_eq!(jump_route(&cmux), JumpRoute::Cmux);

        let tmux = jump_test_session(
            "codex",
            "cli",
            Some(LiveJumpContext {
                tmux_socket: Some("/private/tmp/tmux-501/default".into()),
                tmux_pane: Some("%9".into()),
                ..LiveJumpContext::default()
            }),
        );
        assert_eq!(jump_route(&tmux), JumpRoute::Tmux);
        assert_eq!(
            jump_route(&jump_test_session("claude-code", "cli", None)),
            JumpRoute::DirectTerminal
        );
    }

    #[test]
    fn process_ancestry_recognizes_supported_terminal_hosts() {
        let terminal = parse_process_snapshot(
            "42 30 /opt/homebrew/bin/codex\n30 20 /bin/zsh\n20 1 /System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal\n",
        );
        assert_eq!(
            host_application_for_process(42, &terminal).as_deref(),
            Some("Terminal")
        );

        let cursor = parse_process_snapshot(
            "52 50 /opt/homebrew/bin/claude\n50 10 /bin/zsh\n10 1 /Applications/Cursor.app/Contents/MacOS/Cursor\n",
        );
        assert_eq!(
            host_application_for_process(52, &cursor).as_deref(),
            Some("Cursor")
        );

        let detached_tmux = parse_process_snapshot(
            "62 60 /opt/homebrew/bin/codex\n60 2 /bin/zsh\n2 1 tmux: server\n",
        );
        assert_eq!(host_application_for_process(62, &detached_tmux), None);
    }

    #[test]
    fn jump_identifiers_and_host_names_are_strictly_validated() {
        assert!(valid_tty("/dev/ttys004"));
        assert!(!valid_tty("/dev/ttys004\" & do shell script \"bad"));
        assert!(valid_tmux_pane("%12"));
        assert!(!valid_tmux_pane("main:1.2"));
        assert!(valid_target_id("workspace:4E92-1"));
        assert!(!valid_target_id("workspace 4; rm"));
        assert!(valid_socket_path("/private/tmp/tmux-501/default"));
        assert!(valid_socket_path(
            "/Users/example/.local/state/cmux/cmux.sock"
        ));
        assert!(!valid_socket_path("../../tmp/tmux.sock"));
        assert_eq!(
            host_application_name(Some("vscode"), Some("Cursor")),
            Some("Cursor")
        );
        assert_eq!(host_application_name(None, Some("Unknown")), None);
    }

    #[test]
    fn hook_jump_context_stays_backend_only_and_failed_commands_are_not_success() {
        let envelope = json!({
            "provider":"codex",
            "received_at":Utc::now().to_rfc3339(),
            "process_id":42,
            "origin":"cli",
            "jump_context":{
                "tty":"/dev/ttys004",
                "terminalKind":"cmux",
                "cmuxWorkspaceId":"workspace:2",
                "cmuxSurfaceId":"surface:8"
            },
            "payload":{
                "session_id":"jump-context",
                "hook_event_name":"Stop",
                "cwd":"/tmp/project"
            }
        });
        let (session, _, _) = session_from_envelope(&envelope).expect("session");
        let context = session.jump_context.as_ref().expect("jump context");
        assert_eq!(context.tty.as_deref(), Some("/dev/ttys004"));
        assert_eq!(context.cmux_surface_id.as_deref(), Some("surface:8"));
        assert!(
            serde_json::to_value(&session)
                .expect("serialize session")
                .get("jumpContext")
                .is_none()
        );
        let mut successful_command = Command::new("/usr/bin/true");
        let mut failed_command = Command::new("/usr/bin/false");
        assert!(run_checked(&mut successful_command, "failed").is_ok());
        assert!(run_checked(&mut failed_command, "failed").is_err());
    }

    #[test]
    fn foreground_matching_stays_provider_and_origin_specific() {
        let mut desktop = LiveSession {
            id: "desktop".into(),
            source_session_id: "thread".into(),
            agent: "codex".into(),
            project_label: "project".into(),
            conversation_title: None,
            status: "running".into(),
            phase: "thinking".into(),
            started_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            activity_ended_at: None,
            event_order_key: String::new(),
            waiting_reason: None,
            actions: Vec::new(),
            process_id: None,
            origin: Some("desktop".into()),
            pulse: WorkPulse::default(),
            jump_context: None,
        };
        desktop.pulse.lifecycle.availability = "available".into();
        desktop.pulse.lifecycle.value = Some("running".into());
        let mut cli = desktop.clone();
        cli.origin = Some("cli".into());
        assert!(source_matches_frontmost(&desktop, "Codex"));
        assert!(!source_matches_frontmost(&desktop, "Terminal"));
        assert!(source_matches_frontmost(&cli, "iTerm2"));
        assert!(!source_matches_frontmost(&cli, "VibeMeter"));
        cli.jump_context = Some(LiveJumpContext {
            terminal_kind: Some("cmux".into()),
            host_app_name: Some("cmux".into()),
            ..LiveJumpContext::default()
        });
        assert!(source_matches_frontmost(&cli, "cmux"));
        assert!(!source_matches_frontmost(&cli, "Terminal"));
        assert!(!notification_allowed_for_origin(&desktop, "completed"));
        assert!(notification_allowed_for_origin(&cli, "completed"));
        assert!(notification_allowed_for_origin(&desktop, "waiting"));
        let mut experimental = desktop.clone();
        experimental.agent = "kimi-code".into();
        experimental.pulse.lifecycle.availability = "unknown".into();
        experimental.pulse.lifecycle.value = None;
        assert!(!notification_allowed_for_origin(&experimental, "error"));
    }

    #[test]
    fn notification_timeout_finishes_before_a_claim_can_expire() {
        assert!(
            ATTENTION_NOTIFICATION_TIMEOUT
                < StdDuration::from_secs(
                    crate::database::ATTENTION_NOTIFICATION_LEASE_SECONDS as u64,
                )
        );
    }
}
