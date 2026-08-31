use crate::models::{AgentKind, SourceCapabilitiesDto};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceCapabilityRegistry {
    version: u64,
    sources: Vec<SourceCapabilities>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCapabilities {
    pub agent: String,
    pub display_name: String,
    pub history: SignalCapability,
    pub live_lifecycle: SignalCapability,
    pub jump: SignalCapability,
    pub delegation: SignalCapability,
    pub handoff: SignalCapability,
    pub subagent: SignalCapability,
    pub memory_read: SignalCapability,
    pub memory_write: SignalCapability,
    pub skill_use: SignalCapability,
    pub evaluation: SignalCapability,
}

impl SourceCapabilities {
    pub fn to_dto(&self) -> SourceCapabilitiesDto {
        SourceCapabilitiesDto {
            history: self.history.as_str().into(),
            live_lifecycle: self.live_lifecycle.as_str().into(),
            jump: self.jump.as_str().into(),
            delegation: self.delegation.as_str().into(),
            handoff: self.handoff.as_str().into(),
            subagent: self.subagent.as_str().into(),
            memory_read: self.memory_read.as_str().into(),
            memory_write: self.memory_write.as_str().into(),
            skill_use: self.skill_use.as_str().into(),
            evaluation: self.evaluation.as_str().into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SignalCapability {
    Exact,
    Derived,
    Partial,
    Unavailable,
}

impl SignalCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Derived => "derived",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn history_level(self) -> &'static str {
        match self {
            Self::Exact | Self::Derived => "full",
            Self::Partial | Self::Unavailable => "partial",
        }
    }

    pub fn live_level(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Derived | Self::Partial => "experimental",
            Self::Unavailable => "none",
        }
    }
}

static REGISTRY: Lazy<SourceCapabilityRegistry> = Lazy::new(|| {
    let registry: SourceCapabilityRegistry =
        serde_json::from_str(include_str!("../../../source-capabilities.json"))
            .expect("embedded source capability registry should be valid");
    assert_eq!(
        registry.version, 2,
        "source capabilities must use schema v2"
    );
    registry
});

pub fn source_capabilities() -> &'static [SourceCapabilities] {
    &REGISTRY.sources
}

pub fn source_capability(agent: AgentKind) -> &'static SourceCapabilities {
    source_capabilities()
        .iter()
        .find(|capability| capability.agent == agent.as_str())
        .expect("every AgentKind should have a source capability entry")
}

pub fn source_capability_for_name(agent: &str) -> Option<&'static SourceCapabilities> {
    source_capabilities()
        .iter()
        .find(|capability| capability.agent == agent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_rejects_unknown_signal_values() {
        let registry = serde_json::from_str::<SourceCapabilityRegistry>(
            r#"{
                "version": 2,
                "sources": [{
                    "agent": "codex",
                    "displayName": "Codex",
                    "history": "exact",
                    "liveLifecycle": "exact",
                    "jump": "exact",
                    "delegation": "typo",
                    "handoff": "partial",
                    "subagent": "exact",
                    "memoryRead": "partial",
                    "memoryWrite": "unavailable",
                    "skillUse": "derived",
                    "evaluation": "derived"
                }]
            }"#,
        );
        assert!(registry.is_err());
    }

    #[test]
    fn registry_covers_every_agent_once_and_keeps_unsupported_delegation_truthful() {
        let agents = source_capabilities()
            .iter()
            .map(|source| source.agent.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(agents.len(), source_capabilities().len());
        for agent in [
            AgentKind::ClaudeCode,
            AgentKind::Codex,
            AgentKind::DeepSeekHarness,
            AgentKind::KimiCode,
            AgentKind::GrokBuild,
            AgentKind::Cursor,
            AgentKind::OpenClaw,
            AgentKind::Hermes,
            AgentKind::ZCode,
        ] {
            assert!(agents.contains(agent.as_str()));
        }
        for agent in [
            "deepseek-harness",
            "kimi-code",
            "grok-build",
            "cursor",
            "openclaw",
            "hermes",
            "zcode",
        ] {
            assert_eq!(
                source_capability_for_name(agent).map(|item| item.delegation),
                Some(SignalCapability::Unavailable)
            );
        }
    }

    #[test]
    fn memory_capabilities_match_only_the_signals_adapters_can_observe() {
        let codex = source_capability_for_name("codex").expect("Codex capability");
        assert_eq!(codex.memory_read, SignalCapability::Partial);
        assert_eq!(codex.memory_write, SignalCapability::Unavailable);

        for agent in [
            "claude-code",
            "deepseek-harness",
            "kimi-code",
            "grok-build",
            "cursor",
            "openclaw",
            "hermes",
            "zcode",
        ] {
            let capability = source_capability_for_name(agent).expect("registered capability");
            assert_eq!(capability.memory_read, SignalCapability::Unavailable);
            assert_eq!(capability.memory_write, SignalCapability::Unavailable);
        }
    }
}
