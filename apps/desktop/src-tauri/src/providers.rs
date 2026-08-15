use crate::errors::{AppError, AppResult};
use crate::models::{
    Provenance, ProviderAccountUsage, ProviderDailyAccountUsage, ProviderHealth, ProviderUsage,
    RateWindow,
};
use base64::Engine as _;
use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use once_cell::sync::Lazy;
use portable_pty::{Child as PtyChild, CommandBuilder, PtySize, native_pty_system};
use regex::Regex;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, RwLock, mpsc};
use std::time::{Duration, Instant};

static ANSI_ESCAPE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:\x1B\][^\x07]*(?:\x07|\x1B\\)|\x1B\[[0-?]*[ -/]*[@-~])")
        .expect("valid ANSI regex")
});
static PERCENT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)([0-9]{1,3}(?:\.[0-9]+)?)\s*%\s*(used|spent|consumed|left|remaining|available)?",
    )
    .expect("valid percentage regex")
});
static RESET_DESCRIPTION: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\bResets?\b[^\r\n]{0,120}").expect("valid reset regex"));

#[derive(Clone)]
pub struct ProviderStore {
    inner: Arc<RwLock<HashMap<String, ProviderUsage>>>,
    probe_dir: Arc<PathBuf>,
}

impl ProviderStore {
    pub fn new(probe_dir: PathBuf) -> AppResult<Self> {
        std::fs::create_dir_all(&probe_dir)?;
        let mut providers = HashMap::new();
        for provider in ["claude", "codex", "cursor"] {
            providers.insert(provider.into(), unavailable_provider(provider));
        }
        Ok(Self {
            inner: Arc::new(RwLock::new(providers)),
            probe_dir: Arc::new(probe_dir),
        })
    }

    pub fn snapshot(&self) -> Vec<ProviderUsage> {
        let mut items = self
            .inner
            .read()
            .map(|providers| providers.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        items.sort_by_key(|provider| match provider.provider.as_str() {
            "claude" => 0,
            "codex" => 1,
            "cursor" => 2,
            _ => 3,
        });
        items
    }

    pub fn refresh(
        &self,
        credentials_allowed: bool,
        cursor_dashboard_usage_enabled: bool,
        use_system_proxy: bool,
    ) {
        let Some(client) = build_provider_client(use_system_proxy) else {
            return;
        };
        if (!credentials_allowed || !cursor_dashboard_usage_enabled)
            && let Ok(mut providers) = self.inner.write()
            && let Some(cursor) = providers.get_mut("cursor")
        {
            cursor.account_usage = None;
        }
        let status_clients = build_status_clients(use_system_proxy);
        let claude_health = fetch_status(
            &status_clients,
            "https://status.claude.com/api/v2/status.json",
            "https://status.claude.com",
        );
        let codex_health = fetch_status(
            &status_clients,
            "https://status.openai.com/api/v2/status.json",
            "https://status.openai.com",
        );
        let cursor_health = fetch_status(
            &status_clients,
            "https://status.cursor.com/api/v2/status.json",
            "https://status.cursor.com",
        );

        let codex = if credentials_allowed {
            fetch_codex_usage().map(|mut usage| {
                usage.health = codex_health.clone();
                usage
            })
        } else {
            Err(AppError::ProviderUnavailable(
                "CLI permission is required".into(),
            ))
        };
        self.store_result("codex", codex, codex_health);

        let claude = if credentials_allowed {
            fetch_claude_usage(&client, &self.probe_dir).map(|mut usage| {
                usage.health = claude_health.clone();
                usage
            })
        } else {
            Err(AppError::ProviderUnavailable(
                "credentials permission is required".into(),
            ))
        };
        self.store_result("claude", claude, claude_health);

        // Cursor's local conversation files intentionally do not contain token
        // counters. When the user has enabled credential reads, mirror the
        // Cursor desktop client session into Cursor's own read-only usage API.
        let cursor = if credentials_allowed {
            let cached_account_usage = cursor_dashboard_usage_enabled
                .then(|| self.cached_cursor_account_usage())
                .flatten();
            fetch_cursor_usage(
                &client,
                cursor_dashboard_usage_enabled,
                cached_account_usage,
            )
            .map(|mut usage| {
                usage.health = cursor_health.clone();
                usage
            })
        } else {
            Err(AppError::ProviderUnavailable(
                "credentials permission is required".into(),
            ))
        };
        self.store_result("cursor", cursor, cursor_health);
    }

    fn cached_cursor_account_usage(&self) -> Option<ProviderAccountUsage> {
        let usage = self
            .inner
            .read()
            .ok()?
            .get("cursor")?
            .account_usage
            .clone()?;
        if usage.period_end != Local::now().date_naive().format("%Y-%m-%d").to_string() {
            return None;
        }
        let fetched_at = DateTime::parse_from_rfc3339(&usage.fetched_at)
            .ok()?
            .with_timezone(&Utc);
        let age = Utc::now().signed_duration_since(fetched_at);
        (age >= chrono::Duration::zero() && age < chrono::Duration::hours(1)).then_some(usage)
    }

    fn store_result(
        &self,
        provider: &str,
        result: AppResult<ProviderUsage>,
        health: ProviderHealth,
    ) {
        let Ok(mut providers) = self.inner.write() else {
            return;
        };
        match result {
            Ok(usage) => {
                providers.insert(provider.into(), usage);
            }
            Err(_) => {
                let mut previous = providers
                    .get(provider)
                    .cloned()
                    .unwrap_or_else(|| unavailable_provider(provider));
                previous.health = health;
                previous.stale = previous.available;
                previous.error_key = Some(
                    match provider {
                        "claude" => "providers.claude.unavailable",
                        "cursor" => "providers.cursor.unavailable",
                        _ => "providers.codex.unavailable",
                    }
                    .into(),
                );
                providers.insert(provider.into(), previous);
            }
        }
    }
}

fn unavailable_provider(provider: &str) -> ProviderUsage {
    ProviderUsage {
        provider: provider.into(),
        available: false,
        source: "unavailable".into(),
        windows: Vec::new(),
        credits: None,
        account_usage: None,
        health: ProviderHealth {
            state: "unknown".into(),
            description: String::new(),
            checked_at: None,
            status_url: match provider {
                "claude" => "https://status.claude.com",
                "cursor" => "https://status.cursor.com",
                _ => "https://status.openai.com",
            }
            .into(),
        },
        refreshed_at: None,
        stale: false,
        error_key: Some(format!("providers.{provider}.unavailable")),
    }
}

fn fetch_cursor_account_usage(
    client: &reqwest::blocking::Client,
    cookie: &str,
) -> Option<ProviderAccountUsage> {
    let period_end = Local::now().date_naive();
    let period_start = period_end.checked_sub_signed(chrono::Duration::days(364))?;
    let start_at = Local
        .from_local_datetime(&period_start.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    let start_date = start_at.timestamp_millis().to_string();
    let mut pages = Vec::<Vec<Value>>::new();
    let mut expected = None::<usize>;
    let mut completed = false;
    for page in 1..=200_u64 {
        let payload = client
            .post("https://cursor.com/api/dashboard/get-filtered-usage-events")
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Origin", "https://cursor.com")
            .header("Cookie", cookie)
            .json(&json!({
                "page": page,
                "pageSize": 1000,
                "startDate": start_date,
                "endDate": Value::Null,
            }))
            .send()
            .ok()?
            .error_for_status()
            .ok()?
            .json::<Value>()
            .ok()?;
        let events = payload.get("usageEventsDisplay")?.as_array()?;
        if let Some(count) = cursor_nonnegative_integer(payload.get("totalUsageEventsCount"))
            .and_then(|value| usize::try_from(value).ok())
        {
            if expected.is_some_and(|current| current != count) {
                return None;
            }
            expected = Some(count);
        }
        if events.is_empty() {
            completed = true;
            break;
        }
        pages.push(events.clone());
        if events.len() < 1000 {
            completed = true;
            break;
        }
    }
    if !completed {
        return None;
    }
    let raw_count = pages.iter().map(Vec::len).sum::<usize>();
    if expected.is_some_and(|count| raw_count < count) {
        return None;
    }
    let events = if let Some(expected) = expected.filter(|count| raw_count > *count) {
        let mut removals_remaining = raw_count - expected;
        let mut reconciled = pages.first().cloned().unwrap_or_default();
        for index in 1..pages.len() {
            let overlap = cursor_boundary_overlap(&pages[index - 1], &pages[index]);
            let removal_count = overlap.min(removals_remaining);
            reconciled.extend(pages[index].iter().skip(removal_count).cloned());
            removals_remaining -= removal_count;
        }
        if removals_remaining != 0 || reconciled.len() != expected {
            return None;
        }
        reconciled
    } else {
        pages.into_iter().flatten().collect()
    };
    cursor_account_usage_from_events(&events, period_start, period_end)
}

fn cursor_nonnegative_integer(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    if let Some(value) = value.as_u64() {
        return Some(value);
    }
    if let Some(value) = value.as_i64() {
        return u64::try_from(value).ok();
    }
    if let Some(value) = value.as_f64() {
        return (value.is_finite()
            && value >= 0.0
            && value.fract() == 0.0
            && value <= u64::MAX as f64)
            .then_some(value as u64);
    }
    value.as_str().and_then(|value| {
        value.parse::<u64>().ok().or_else(|| {
            value.parse::<f64>().ok().and_then(|value| {
                (value.is_finite()
                    && value >= 0.0
                    && value.fract() == 0.0
                    && value <= u64::MAX as f64)
                    .then_some(value as u64)
            })
        })
    })
}

fn cursor_nonnegative_double(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    let parsed = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))?;
    (parsed.is_finite() && parsed >= 0.0).then_some(parsed)
}

#[derive(Debug)]
struct CursorDailyAccumulator {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    api_cost_usd: Option<f64>,
    metered_cost_usd: Option<f64>,
    request_count: u64,
    token_request_count: u64,
}

impl Default for CursorDailyAccumulator {
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            api_cost_usd: Some(0.0),
            metered_cost_usd: Some(0.0),
            request_count: 0,
            token_request_count: 0,
        }
    }
}

fn checked_optional_cost(current: Option<f64>, value: Option<f64>) -> Option<f64> {
    let total = current? + value?;
    total.is_finite().then_some(total)
}

fn cursor_account_usage_from_events(
    events: &[Value],
    period_start: NaiveDate,
    period_end: NaiveDate,
) -> Option<ProviderAccountUsage> {
    let mut daily = BTreeMap::<(String, String), CursorDailyAccumulator>::new();
    for event in events {
        let Some(timestamp) = cursor_nonnegative_integer(event.get("timestamp"))
            .filter(|value| *value > 0)
            .and_then(|value| i64::try_from(value).ok())
            .and_then(DateTime::<Utc>::from_timestamp_millis)
        else {
            continue;
        };
        let date = timestamp
            .with_timezone(&Local)
            .format("%Y-%m-%d")
            .to_string();
        let model = event
            .get("model")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown")
            .to_owned();
        let accumulator = daily.entry((date, model)).or_default();
        accumulator.request_count = accumulator.request_count.checked_add(1)?;
        accumulator.metered_cost_usd = checked_optional_cost(
            accumulator.metered_cost_usd,
            cursor_nonnegative_double(event.get("chargedCents")).map(|value| value / 100.0),
        );

        let Some(usage) = event.get("tokenUsage") else {
            // Cursor can report metered requests without token details. They
            // still participate in the metered total and request count.
            continue;
        };
        let input_tokens = cursor_nonnegative_integer(usage.get("inputTokens")).unwrap_or(0);
        let output_tokens = cursor_nonnegative_integer(usage.get("outputTokens")).unwrap_or(0);
        let cache_write_tokens =
            cursor_nonnegative_integer(usage.get("cacheWriteTokens")).unwrap_or(0);
        let cache_read_tokens =
            cursor_nonnegative_integer(usage.get("cacheReadTokens")).unwrap_or(0);
        let event_total = input_tokens
            .checked_add(output_tokens)?
            .checked_add(cache_write_tokens)?
            .checked_add(cache_read_tokens)?;
        if event_total == 0 {
            continue;
        }
        accumulator.input_tokens = accumulator.input_tokens.checked_add(input_tokens)?;
        accumulator.output_tokens = accumulator.output_tokens.checked_add(output_tokens)?;
        accumulator.cache_write_tokens = accumulator
            .cache_write_tokens
            .checked_add(cache_write_tokens)?;
        accumulator.cache_read_tokens = accumulator
            .cache_read_tokens
            .checked_add(cache_read_tokens)?;
        accumulator.token_request_count = accumulator.token_request_count.checked_add(1)?;
        accumulator.api_cost_usd = checked_optional_cost(
            accumulator.api_cost_usd,
            cursor_nonnegative_double(usage.get("totalCents")).map(|value| value / 100.0),
        );
    }
    Some(ProviderAccountUsage {
        period_start: period_start.format("%Y-%m-%d").to_string(),
        period_end: period_end.format("%Y-%m-%d").to_string(),
        fetched_at: Utc::now().to_rfc3339(),
        scope: "account".into(),
        daily: daily
            .into_iter()
            .map(|((date, model), accumulator)| ProviderDailyAccountUsage {
                date,
                model,
                input_tokens: accumulator.input_tokens,
                output_tokens: accumulator.output_tokens,
                cache_read_tokens: accumulator.cache_read_tokens,
                cache_write_tokens: accumulator.cache_write_tokens,
                api_cost_usd: accumulator.api_cost_usd,
                metered_cost_usd: accumulator.metered_cost_usd,
                request_count: accumulator.request_count,
                token_request_count: accumulator.token_request_count,
            })
            .collect(),
        account_fingerprint: String::new(),
    })
}

fn cursor_boundary_overlap(previous: &[Value], current: &[Value]) -> usize {
    let limit = previous.len().min(current.len());
    (1..=limit)
        .rev()
        .find(|count| previous[previous.len() - *count..] == current[..*count])
        .unwrap_or(0)
}

fn cursor_session_cookie(user_id: &str, access_token: &str) -> String {
    format!("WorkosCursorSessionToken={user_id}%3A%3A{access_token}")
}

fn fetch_cursor_usage(
    client: &reqwest::blocking::Client,
    include_dashboard_usage: bool,
    cached_account_usage: Option<ProviderAccountUsage>,
) -> AppResult<ProviderUsage> {
    let home = std::env::var("HOME").map_err(|_| {
        AppError::ProviderUnavailable("Cursor home directory is unavailable".into())
    })?;
    let path = PathBuf::from(home)
        .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb");
    let connection =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| {
            AppError::ProviderUnavailable("Cursor desktop sign-in was not found".into())
        })?;
    let access_token: String = connection
        .query_row(
            "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|_| {
            AppError::ProviderUnavailable("Cursor desktop sign-in was not found".into())
        })?;
    let subject = jwt_claim(&access_token, "sub")
        .ok_or_else(|| AppError::ProviderUnavailable("Cursor desktop sign-in is invalid".into()))?;
    let user_id = subject
        .rsplit('|')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::ProviderUnavailable("Cursor desktop sign-in is invalid".into()))?;
    let cookie = cursor_session_cookie(user_id, &access_token);
    let account_fingerprint = crate::privacy::stable_hash(&format!("cursor:{user_id}"));
    let summary = client
        .get("https://cursor.com/api/usage-summary")
        .header("Accept", "application/json")
        .header("Cookie", &cookie)
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.json::<Value>())
        .map_err(|_| AppError::ProviderUnavailable("Cursor account usage is unavailable".into()))?;
    let used_percent = summary
        .pointer("/individualUsage/plan/totalPercentUsed")
        .and_then(Value::as_f64)
        .or_else(|| {
            summary
                .pointer("/individualUsage/plan/apiPercentUsed")
                .and_then(Value::as_f64)
        })
        .map(|value| value.clamp(0.0, 100.0));
    let reset_at = summary
        .get("billingCycleEnd")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(ProviderUsage {
        provider: "cursor".into(),
        available: true,
        source: "cursor-desktop-session".into(),
        windows: vec![RateWindow {
            id: "cursor-plan".into(),
            label: "Cursor plan".into(),
            used_percent,
            reset_at,
            reset_description: None,
            provenance: Provenance::Observed,
        }],
        credits: None,
        account_usage: if include_dashboard_usage {
            cached_account_usage
                .filter(|usage| usage.account_fingerprint == account_fingerprint)
                .or_else(|| {
                    fetch_cursor_account_usage(client, &cookie).map(|mut usage| {
                        usage.account_fingerprint = account_fingerprint;
                        usage
                    })
                })
        } else {
            None
        },
        health: unavailable_provider("cursor").health,
        refreshed_at: Some(Utc::now().to_rfc3339()),
        stale: false,
        error_key: None,
    })
}

fn jwt_claim(token: &str, claim: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let padded = format!("{}{}", payload, "=".repeat((4 - payload.len() % 4) % 4));
    let bytes = base64::engine::general_purpose::URL_SAFE
        .decode(padded)
        .ok()?;
    serde_json::from_slice::<Value>(&bytes)
        .ok()?
        .get(claim)?
        .as_str()
        .map(str::to_owned)
}

fn build_http_client(
    proxy_url: Option<&str>,
    timeout: Duration,
) -> Option<reqwest::blocking::Client> {
    let builder = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("vibemeter/", env!("CARGO_PKG_VERSION")));
    if let Some(proxy_url) = proxy_url {
        // Proxy use is opt-in for provider quota and status requests. The URL is
        // discovered from the active macOS network service and is never persisted.
        builder.proxy(reqwest::Proxy::all(proxy_url).ok()?)
    } else {
        builder
    }
    .build()
    .ok()
}

fn build_provider_client(use_system_proxy: bool) -> Option<reqwest::blocking::Client> {
    #[cfg(target_os = "macos")]
    if use_system_proxy {
        for proxy_url in macos_proxy_candidates(false) {
            if let Some(client) = build_http_client(Some(&proxy_url), Duration::from_secs(15)) {
                return Some(client);
            }
        }
    }
    build_http_client(None, Duration::from_secs(10))
}

fn build_status_clients(use_system_proxy: bool) -> Vec<reqwest::blocking::Client> {
    let mut clients = Vec::new();
    if let Some(client) = build_http_client(None, Duration::from_secs(6)) {
        clients.push(client);
    }
    if use_system_proxy {
        for proxy_url in status_proxy_candidates() {
            if let Some(client) = build_http_client(Some(&proxy_url), Duration::from_secs(12)) {
                clients.push(client);
            }
        }
    }
    clients
}

fn status_proxy_candidates() -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Ok(value) = std::env::var(key) {
            push_proxy_candidate(&mut candidates, &mut seen, &value);
        }
    }
    #[cfg(target_os = "macos")]
    for candidate in macos_proxy_candidates(true) {
        push_proxy_candidate(&mut candidates, &mut seen, &candidate);
    }
    candidates
}

fn push_proxy_candidate(candidates: &mut Vec<String>, seen: &mut HashSet<String>, raw: &str) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    let candidate = if trimmed.starts_with("socks://") {
        trimmed.replacen("socks://", "socks5h://", 1)
    } else if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("http://{trimmed}")
    };
    let Ok(parsed) = url::Url::parse(&candidate) else {
        return;
    };
    if parsed.host_str().is_none()
        || !matches!(parsed.scheme(), "http" | "https" | "socks5" | "socks5h")
    {
        return;
    }
    if seen.insert(candidate.clone()) {
        candidates.push(candidate);
    }
}

#[cfg(target_os = "macos")]
fn macos_proxy_candidates(allow_disabled_loopback: bool) -> Vec<String> {
    let Some(services) = Command::new("/usr/sbin/networksetup")
        .arg("-listallnetworkservices")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
    else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for service in services
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("An asterisk"))
        .map(|line| line.trim_start_matches('*').trim())
    {
        for (command, scheme) in [
            ("-getsecurewebproxy", "http"),
            ("-getwebproxy", "http"),
            ("-getsocksfirewallproxy", "socks5h"),
        ] {
            let Some(output) = Command::new("/usr/sbin/networksetup")
                .args([command, service])
                .output()
                .ok()
                .filter(|output| output.status.success())
            else {
                continue;
            };
            if let Some(candidate) = parse_networksetup_proxy(
                &String::from_utf8_lossy(&output.stdout),
                scheme,
                allow_disabled_loopback,
            ) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

#[cfg(target_os = "macos")]
fn parse_networksetup_proxy(
    output: &str,
    scheme: &str,
    allow_disabled_loopback: bool,
) -> Option<String> {
    let mut enabled = false;
    let mut server = None;
    let mut port = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "Enabled" => enabled = value.trim().eq_ignore_ascii_case("yes"),
            "Server" => server = Some(value.trim()),
            "Port" => port = value.trim().parse::<u16>().ok().filter(|port| *port > 0),
            _ => {}
        }
    }
    let server = server.filter(|server| !server.is_empty())?;
    // Browser proxy extensions commonly leave a loopback listener configured
    // while the macOS system-proxy switch is off. Direct access has already
    // been attempted, so this disabled loopback entry is a safe fallback for
    // the public, credential-free status request.
    let loopback = matches!(server, "127.0.0.1" | "localhost" | "::1");
    if !enabled && !(allow_disabled_loopback && loopback) {
        return None;
    }
    let host = if server.contains(':') && !server.starts_with('[') {
        format!("[{server}]")
    } else {
        server.to_owned()
    };
    Some(format!("{scheme}://{host}:{}", port?))
}

fn fetch_status(
    clients: &[reqwest::blocking::Client],
    endpoint: &str,
    link: &str,
) -> ProviderHealth {
    let checked_at = Utc::now().to_rfc3339();
    for client in clients {
        let payload = client
            .get(endpoint)
            .send()
            .and_then(|response| response.error_for_status())
            .and_then(|response| response.json::<Value>());
        if let Ok(payload) = payload
            && let Some(health) = status_from_json(&payload, link, &checked_at)
        {
            return health;
        }
    }
    // Some hosted status services temporarily change or block their JSON
    // route while the public page remains available. Use visible page language
    // only as a final fallback; never guess a healthy state from HTTP 200 alone.
    for client in clients {
        let page = client
            .get(link)
            .send()
            .and_then(|response| response.error_for_status())
            .and_then(|response| response.text());
        if let Ok(page) = page
            && let Some((state, description)) = status_from_html(&page)
        {
            return ProviderHealth {
                state: state.into(),
                description: description.into(),
                checked_at: Some(checked_at),
                status_url: link.into(),
            };
        }
    }
    ProviderHealth {
        state: "unknown".into(),
        description: String::new(),
        checked_at: Some(checked_at),
        status_url: link.into(),
    }
}

fn status_from_json(payload: &Value, link: &str, checked_at: &str) -> Option<ProviderHealth> {
    let indicator = payload
        .pointer("/status/indicator")
        .and_then(Value::as_str)?
        .to_ascii_lowercase();
    let state = match indicator.as_str() {
        "none" | "operational" | "ok" | "up" => "operational",
        "minor" | "degraded" | "degraded_performance" | "partial_outage" => "minor",
        "major" | "major_outage" | "outage" => "major",
        "critical" => "critical",
        _ => return None,
    };
    Some(ProviderHealth {
        state: state.into(),
        description: payload
            .pointer("/status/description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .take(160)
            .collect(),
        checked_at: Some(checked_at.into()),
        status_url: link.into(),
    })
}

fn status_from_html(page: &str) -> Option<(&'static str, &'static str)> {
    let page = page.to_ascii_lowercase();
    if page.contains("critical outage") || page.contains("critical incident") {
        Some(("critical", "Status page reports a critical incident"))
    } else if page.contains("major outage") || page.contains("major incident") {
        Some(("major", "Status page reports a major incident"))
    } else if page.contains("currently experiencing issues")
        || page.contains("degraded performance")
        || page.contains("partial outage")
        || page.contains("active incident")
    {
        Some(("minor", "Status page reports active issues"))
    } else if page.contains("all systems operational") {
        Some(("operational", "All systems operational"))
    } else {
        None
    }
}

fn fetch_codex_usage() -> AppResult<ProviderUsage> {
    let binary = codex_binary()
        .ok_or_else(|| AppError::ProviderUnavailable("a working Codex CLI was not found".into()))?;
    let mut child = Command::new(binary)
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let probe_result: AppResult<Value> = (|| {
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

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut result = None;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let Ok(line) = receiver.recv_timeout(remaining.min(Duration::from_millis(500))) else {
                continue;
            };
            let Ok(payload) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match payload.get("id").and_then(Value::as_i64) {
                Some(1) => {
                    write_json_line(&mut stdin, &json!({"method":"initialized","params":{}}))?;
                    write_json_line(
                        &mut stdin,
                        &json!({"id":2,"method":"account/rateLimits/read","params":{}}),
                    )?;
                }
                Some(2) => {
                    result = payload.get("result").cloned();
                    break;
                }
                _ => {}
            }
        }
        result.ok_or_else(|| AppError::ProviderUnavailable("Codex quota request timed out".into()))
    })();
    let _ = child.kill();
    let _ = child.wait();
    let payload = probe_result?;
    codex_usage_from_value(&payload)
}

fn codex_usage_from_value(payload: &Value) -> AppResult<ProviderUsage> {
    let mut windows = Vec::new();
    let snapshots = payload
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if snapshots.is_empty() {
        if let Some(snapshot) = payload.get("rateLimits") {
            append_codex_snapshot(&mut windows, "codex", None, snapshot);
        }
    } else {
        for (id, snapshot) in snapshots {
            append_codex_snapshot(
                &mut windows,
                &id,
                snapshot.get("limitName").and_then(Value::as_str),
                &snapshot,
            );
        }
    }
    let credits = payload
        .pointer("/rateLimits/credits/balance")
        .and_then(|value| value.as_str().and_then(|value| value.parse::<f64>().ok()));
    if windows.is_empty() {
        return Err(AppError::ProviderUnavailable(
            "Codex returned no quota windows".into(),
        ));
    }
    Ok(ProviderUsage {
        provider: "codex".into(),
        available: true,
        source: "codex-app-server".into(),
        windows,
        credits,
        account_usage: None,
        health: unavailable_provider("codex").health,
        refreshed_at: Some(Utc::now().to_rfc3339()),
        stale: false,
        error_key: None,
    })
}

fn append_codex_snapshot(
    destination: &mut Vec<RateWindow>,
    id: &str,
    name: Option<&str>,
    snapshot: &Value,
) {
    for (lane, value) in [
        ("primary", snapshot.get("primary")),
        ("secondary", snapshot.get("secondary")),
    ] {
        let Some(window) = value.filter(|value| !value.is_null()) else {
            continue;
        };
        let minutes = window.get("windowDurationMins").and_then(Value::as_i64);
        let label = name.map(ToString::to_string).unwrap_or_else(|| {
            if minutes.is_some_and(|value| value <= 360) {
                "quota.session".into()
            } else if minutes.is_some_and(|value| value >= 10_000) {
                "quota.weekly".into()
            } else {
                "quota.window".into()
            }
        });
        destination.push(RateWindow {
            id: format!("{id}-{lane}"),
            label,
            used_percent: window.get("usedPercent").and_then(Value::as_f64),
            reset_at: window
                .get("resetsAt")
                .and_then(Value::as_i64)
                .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
                .map(|value| value.to_rfc3339()),
            reset_description: None,
            provenance: Provenance::Observed,
        });
    }
}

pub(crate) fn write_json_line(writer: &mut impl Write, value: &Value) -> AppResult<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub(crate) fn codex_binary() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from(
        "/Applications/ChatGPT.app/Contents/Resources/codex",
    )];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Applications/ChatGPT.app/Contents/Resources/codex"));
        candidates.push(home.join(".local/bin/codex"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/codex"));
    candidates.push(PathBuf::from("/usr/local/bin/codex"));
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn fetch_claude_usage(
    client: &reqwest::blocking::Client,
    probe_dir: &Path,
) -> AppResult<ProviderUsage> {
    if let Some(token) = load_claude_oauth_token() {
        let response = client
            .get("https://api.anthropic.com/api/oauth/usage")
            .bearer_auth(token)
            .header("anthropic-beta", "oauth-2025-04-20")
            .send()?
            .error_for_status()?
            .json::<Value>()?;
        if let Ok(usage) = claude_oauth_usage_from_value(&response) {
            return Ok(usage);
        }
    }
    probe_claude_cli(probe_dir)
}

fn load_claude_oauth_token() -> Option<String> {
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".claude/.credentials.json");
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(payload) = serde_json::from_slice::<Value>(&bytes)
            && let Some(token) = token_from_credentials(&payload)
        {
            return Some(token);
        }
    }
    let output = Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice::<Value>(&output.stdout)
        .ok()
        .and_then(|payload| token_from_credentials(&payload))
}

fn token_from_credentials(payload: &Value) -> Option<String> {
    payload
        .pointer("/claudeAiOauth/accessToken")
        .or_else(|| payload.pointer("/claudeAiOauth/access_token"))
        .or_else(|| payload.get("accessToken"))
        .or_else(|| payload.get("access_token"))
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
}

fn claude_oauth_usage_from_value(payload: &Value) -> AppResult<ProviderUsage> {
    let mut windows = Vec::new();
    for (field, id, label) in [
        ("five_hour", "session", "quota.session"),
        ("seven_day", "weekly", "quota.weekly"),
        ("seven_day_opus", "opus-weekly", "quota.opusWeekly"),
        ("seven_day_sonnet", "sonnet-weekly", "quota.sonnetWeekly"),
    ] {
        if let Some(value) = payload.get(field).filter(|value| !value.is_null()) {
            windows.push(RateWindow {
                id: id.into(),
                label: label.into(),
                used_percent: value
                    .get("utilization")
                    .or_else(|| value.get("used_percent"))
                    .and_then(Value::as_f64),
                reset_at: value
                    .get("resets_at")
                    .or_else(|| value.get("reset_at"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                reset_description: None,
                provenance: Provenance::Observed,
            });
        }
    }
    if windows.is_empty() {
        return Err(AppError::ProviderUnavailable(
            "Claude returned no quota windows".into(),
        ));
    }
    Ok(ProviderUsage {
        provider: "claude".into(),
        available: true,
        source: "claude-oauth".into(),
        windows,
        credits: None,
        account_usage: None,
        health: unavailable_provider("claude").health,
        refreshed_at: Some(Utc::now().to_rfc3339()),
        stale: false,
        error_key: None,
    })
}

fn probe_claude_cli(probe_dir: &Path) -> AppResult<ProviderUsage> {
    let binary = claude_binary()
        .ok_or_else(|| AppError::ProviderUnavailable("Claude CLI was not found".into()))?;
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 42,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| AppError::ProviderUnavailable(error.to_string()))?;
    let mut command = CommandBuilder::new(binary);
    command.args([
        "--safe-mode",
        "--allowed-tools",
        "",
        "--permission-mode",
        "dontAsk",
    ]);
    command.cwd(probe_dir);
    command.env("TERM", "xterm-256color");
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| AppError::ProviderUnavailable(error.to_string()))?;
    drop(pair.slave);
    let probe_result: AppResult<Vec<u8>> = (|| {
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| AppError::ProviderUnavailable(error.to_string()))?;
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|error| AppError::ProviderUnavailable(error.to_string()))?;
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut chunk = [0_u8; 8192];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if sender.send(chunk[..count].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        std::thread::sleep(Duration::from_millis(1200));
        writer.write_all(b"/usage\r")?;
        writer.flush()?;
        let deadline = Instant::now() + Duration::from_secs(14);
        let mut capture = Vec::new();
        let mut last_enter = Instant::now();
        while Instant::now() < deadline && capture.len() < 2 * 1024 * 1024 {
            if let Ok(chunk) = receiver.recv_timeout(Duration::from_millis(300)) {
                capture.extend_from_slice(&chunk);
            }
            let text = strip_terminal_codes(&String::from_utf8_lossy(&capture));
            if text.to_ascii_lowercase().contains("current session") && PERCENT.is_match(&text) {
                std::thread::sleep(Duration::from_millis(700));
                while let Ok(chunk) = receiver.try_recv() {
                    capture.extend_from_slice(&chunk);
                }
                break;
            }
            if last_enter.elapsed() >= Duration::from_secs(2) {
                let _ = writer.write_all(b"\r");
                let _ = writer.flush();
                last_enter = Instant::now();
            }
        }
        let _ = writer.write_all(&[3]);
        let _ = writer.flush();
        Ok(capture)
    })();
    terminate_pty_child(child.as_mut());
    let capture = probe_result?;
    let text = strip_terminal_codes(&String::from_utf8_lossy(&capture));
    claude_cli_usage_from_text(&text)
}

fn terminate_pty_child(child: &mut dyn PtyChild) {
    #[cfg(unix)]
    if let Some(child) = child.downcast_mut::<std::process::Child>() {
        let _ = std::process::Child::kill(child);
        let _ = std::process::Child::wait(child);
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn claude_cli_usage_from_text(text: &str) -> AppResult<ProviderUsage> {
    let mut windows = Vec::new();
    for (label, id, localized_label) in [
        ("Current session", "session", "quota.session"),
        ("Current week (all models)", "weekly", "quota.weekly"),
        ("Current week (Opus)", "opus-weekly", "quota.opusWeekly"),
        (
            "Current week (Sonnet)",
            "sonnet-weekly",
            "quota.sonnetWeekly",
        ),
    ] {
        if let Some(window) = parse_cli_window(text, label, id, localized_label) {
            windows.push(window);
        }
    }
    if windows.is_empty() {
        return Err(AppError::ProviderUnavailable(
            "Claude CLI did not expose numeric quota data".into(),
        ));
    }
    Ok(ProviderUsage {
        provider: "claude".into(),
        available: true,
        source: "claude-cli".into(),
        windows,
        credits: None,
        account_usage: None,
        health: unavailable_provider("claude").health,
        refreshed_at: Some(Utc::now().to_rfc3339()),
        stale: false,
        error_key: None,
    })
}

fn parse_cli_window(
    text: &str,
    label: &str,
    id: &str,
    localized_label: &str,
) -> Option<RateWindow> {
    let lower = text.to_ascii_lowercase();
    let start = lower.rfind(&label.to_ascii_lowercase())?;
    let tail = &text[start..];
    let boundary = tail
        .get(label.len()..)
        .and_then(|rest| rest.to_ascii_lowercase().find("current "))
        .map(|index| label.len() + index)
        .unwrap_or_else(|| tail.len().min(1200));
    let section = &tail[..boundary.min(tail.len())];
    let captures = PERCENT.captures(section)?;
    let raw = captures
        .get(1)?
        .as_str()
        .parse::<f64>()
        .ok()?
        .clamp(0.0, 100.0);
    let qualifier = captures
        .get(2)
        .map(|value| value.as_str().to_ascii_lowercase())
        .unwrap_or_default();
    let used = if matches!(qualifier.as_str(), "left" | "remaining" | "available") {
        100.0 - raw
    } else {
        raw
    };
    let reset_description = RESET_DESCRIPTION
        .find(section)
        .map(|value| value.as_str().trim().chars().take(120).collect());
    Some(RateWindow {
        id: id.into(),
        label: localized_label.into(),
        used_percent: Some(used),
        reset_at: None,
        reset_description,
        provenance: Provenance::Observed,
    })
}

fn strip_terminal_codes(value: &str) -> String {
    ANSI_ESCAPE
        .replace_all(value, "")
        .replace('\r', "\n")
        .chars()
        .map(|character| {
            if character.is_control() && character != '\n' && character != '\t' {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn claude_binary() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/claude"));
        candidates.push(home.join(".claude/local/claude"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/claude"));
    candidates.push(PathBuf::from("/usr/local/bin/claude"));
    candidates.into_iter().find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remaining_and_used_claude_windows() {
        let fixture = "Settings: Usage\nCurrent session\n82% left\nResets 5pm\nCurrent week (all models)\n24% used\nResets Jul 22 at 3pm";
        let usage = claude_cli_usage_from_text(fixture).expect("Claude usage");
        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[0].used_percent, Some(18.0));
        assert_eq!(usage.windows[1].used_percent, Some(24.0));
    }

    #[cfg(unix)]
    #[test]
    fn pty_probe_child_is_reaped_after_termination() {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 4,
                cols: 20,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("pty");
        let mut command = CommandBuilder::new("/bin/sleep");
        command.arg("30");
        let mut child = pair.slave.spawn_command(command).expect("child");
        let process_id = child.process_id().expect("process id");
        terminate_pty_child(child.as_mut());

        let status = Command::new("/bin/ps")
            .args(["-p", &process_id.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("ps status");
        assert!(
            !status.success(),
            "terminated child should not remain a zombie"
        );
    }

    #[test]
    fn maps_codex_rate_limit_response_without_account_identity() {
        let fixture = json!({
            "rateLimitsByLimitId": {
                "codex": {
                    "primary": {"usedPercent": 31, "windowDurationMins": 300, "resetsAt": 1785000000},
                    "secondary": {"usedPercent": 12, "windowDurationMins": 10080, "resetsAt": 1785300000}
                }
            }
        });
        let usage = codex_usage_from_value(&fixture).expect("Codex usage");
        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[0].label, "quota.session");
        assert_eq!(usage.windows[1].label, "quota.weekly");
    }

    #[test]
    fn cursor_events_build_daily_token_and_cost_rows_from_mixed_number_encodings() {
        let timestamp = Utc
            .with_ymd_and_hms(2026, 7, 24, 12, 0, 0)
            .single()
            .expect("timestamp")
            .timestamp_millis();
        let events = vec![
            json!({
                "timestamp": timestamp.to_string(),
                "model": "composer-1",
                "chargedCents": "2",
                "tokenUsage": {
                    "inputTokens": "120",
                    "outputTokens": 30.0,
                    "cacheWriteTokens": "5.0",
                    "cacheReadTokens": 45,
                    "totalCents": "3"
                }
            }),
            json!({"timestamp": timestamp, "model": "composer-1", "kind": "usage-based", "chargedCents": 2.4}),
            json!({
                "timestamp": timestamp,
                "model": "composer-1",
                "chargedCents": 0.6,
                "tokenUsage": {
                    "inputTokens": 10,
                    "outputTokens": 2,
                    "cacheWriteTokens": 0,
                    "cacheReadTokens": 3,
                    "totalCents": 0.5
                }
            }),
        ];
        let usage = cursor_account_usage_from_events(
            &events,
            NaiveDate::from_ymd_opt(2026, 7, 1).expect("start"),
            NaiveDate::from_ymd_opt(2026, 7, 24).expect("end"),
        )
        .expect("usage");
        assert_eq!(usage.daily.len(), 1);
        let row = &usage.daily[0];
        assert_eq!(
            row.input_tokens + row.output_tokens + row.cache_read_tokens + row.cache_write_tokens,
            215
        );
        assert_eq!(row.request_count, 3);
        assert_eq!(row.token_request_count, 2);
        assert!((row.api_cost_usd.expect("API cost") - 0.035).abs() < f64::EPSILON);
        assert!((row.metered_cost_usd.expect("metered cost") - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn cursor_events_keep_metered_only_rows_and_do_not_invent_token_requests() {
        let timestamp = Utc
            .with_ymd_and_hms(2026, 7, 24, 12, 0, 0)
            .single()
            .expect("timestamp")
            .timestamp_millis();
        let usage = cursor_account_usage_from_events(
            &[
                json!({"timestamp": timestamp, "kind": "request", "chargedCents": 1.2}),
                json!({"timestamp": timestamp, "tokenUsage": {"inputTokens": "0", "outputTokens": 0}, "chargedCents": 0}),
            ],
            NaiveDate::from_ymd_opt(2026, 7, 1).expect("start"),
            NaiveDate::from_ymd_opt(2026, 7, 24).expect("end"),
        )
        .expect("usage");
        assert_eq!(usage.daily.len(), 1);
        assert_eq!(usage.daily[0].token_request_count, 0);
        assert_eq!(usage.daily[0].request_count, 2);
        assert_eq!(usage.daily[0].metered_cost_usd, Some(0.012));
    }

    #[test]
    fn cursor_events_fail_cost_fields_closed_when_any_valid_event_is_incomplete() {
        let timestamp = Utc
            .with_ymd_and_hms(2026, 7, 24, 12, 0, 0)
            .single()
            .expect("timestamp")
            .timestamp_millis();
        let usage = cursor_account_usage_from_events(
            &[
                json!({
                    "timestamp": timestamp,
                    "model": "composer-1",
                    "tokenUsage": {"inputTokens": 10, "totalCents": 1},
                    "chargedCents": 1
                }),
                json!({
                    "timestamp": timestamp,
                    "model": "composer-1",
                    "tokenUsage": {"outputTokens": 5}
                }),
            ],
            NaiveDate::from_ymd_opt(2026, 7, 1).expect("start"),
            NaiveDate::from_ymd_opt(2026, 7, 24).expect("end"),
        )
        .expect("usage");
        assert_eq!(usage.daily[0].api_cost_usd, None);
        assert_eq!(usage.daily[0].metered_cost_usd, None);
    }

    #[test]
    fn cursor_pagination_reconciles_only_exact_boundary_overlap() {
        let previous = vec![json!({"timestamp": 1}), json!({"timestamp": 2})];
        let current = vec![json!({"timestamp": 2}), json!({"timestamp": 3})];
        assert_eq!(cursor_boundary_overlap(&previous, &current), 1);
        assert_eq!(
            cursor_boundary_overlap(&previous, &[json!({"timestamp": 3})]),
            0
        );
    }

    #[test]
    fn cursor_desktop_cookie_uses_the_dashboard_session_separator() {
        assert_eq!(
            cursor_session_cookie("user-1", "token"),
            "WorkosCursorSessionToken=user-1%3A%3Atoken"
        );
    }

    #[test]
    fn maps_hosted_status_payloads_without_treating_unknown_schema_as_health() {
        let health = status_from_json(
            &json!({"status":{"indicator":"none","description":"All Systems Operational"}}),
            "https://status.openai.com",
            "2026-07-24T00:00:00Z",
        )
        .expect("known hosted status payload");
        assert_eq!(health.state, "operational");
        assert_eq!(health.description, "All Systems Operational");
        assert!(
            status_from_json(
                &json!({"status":{"indicator":"mystery"}}),
                "https://status.openai.com",
                "2026-07-24T00:00:00Z",
            )
            .is_none()
        );
    }

    #[test]
    fn uses_status_page_language_only_as_an_explicit_fallback() {
        assert_eq!(
            status_from_html("<h1>All Systems Operational</h1>"),
            Some(("operational", "All systems operational"))
        );
        assert_eq!(
            status_from_html("<p>We’re currently experiencing issues</p>"),
            Some(("minor", "Status page reports active issues"))
        );
        assert_eq!(status_from_html("<html><body>OpenAI</body></html>"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn accepts_disabled_loopback_proxy_as_public_status_fallback() {
        let fixture =
            "Enabled: No\nServer: 127.0.0.1\nPort: 7892\nAuthenticated Proxy Enabled: 0\n";
        assert_eq!(
            parse_networksetup_proxy(fixture, "http", true),
            Some("http://127.0.0.1:7892".into())
        );
        assert_eq!(parse_networksetup_proxy(fixture, "http", false), None);
        let remote = "Enabled: No\nServer: proxy.example.com\nPort: 8080\n";
        assert_eq!(parse_networksetup_proxy(remote, "http", true), None);
    }
}
