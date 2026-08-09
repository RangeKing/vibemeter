use crate::models::AgentKind;
use once_cell::sync::Lazy;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceCapabilityRegistry {
    #[allow(dead_code)]
    version: u64,
    sources: Vec<SourceCapability>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCapability {
    pub agent: String,
    #[allow(dead_code)]
    pub display_name: String,
    pub history_capability: SourceHistoryCapability,
    pub live_capability: SourceLiveCapability,
    #[allow(dead_code)]
    pub jump_supported: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceHistoryCapability {
    Full,
    Partial,
}

impl SourceHistoryCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceLiveCapability {
    Exact,
    Experimental,
    None,
}

impl SourceLiveCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Experimental => "experimental",
            Self::None => "none",
        }
    }
}

static REGISTRY: Lazy<SourceCapabilityRegistry> = Lazy::new(|| {
    serde_json::from_str(include_str!("../../source-capabilities.json"))
        .expect("embedded source capability registry should be valid")
});

pub fn source_capabilities() -> &'static [SourceCapability] {
    &REGISTRY.sources
}

pub fn source_capability(agent: AgentKind) -> &'static SourceCapability {
    source_capabilities()
        .iter()
        .find(|capability| capability.agent == agent.as_str())
        .expect("every AgentKind should have a source capability entry")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_rejects_unknown_capability_values() {
        let registry = serde_json::from_str::<SourceCapabilityRegistry>(
            r#"{
                "version": 1,
                "sources": [{
                    "agent": "codex",
                    "displayName": "Codex",
                    "historyCapability": "full",
                    "liveCapability": "typo",
                    "jumpSupported": true
                }]
            }"#,
        );

        assert!(registry.is_err());
    }
}
