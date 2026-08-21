use crate::models::{AgentKind, TokenUsage};
use once_cell::sync::Lazy;
use serde::Deserialize;

#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    pub input: f64,
    pub cache_read: f64,
    pub cache_write: Option<f64>,
    pub cache_write_1h: Option<f64>,
    pub output: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PricingCatalog {
    generated_at: String,
    models: Vec<GeneratedModelPrice>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedModelPrice {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    currency: String,
    input: f64,
    cache_read: f64,
    cache_write: Option<f64>,
    cache_write_1h: Option<f64>,
    output: f64,
}

static CATALOG: Lazy<PricingCatalog> = Lazy::new(|| {
    serde_json::from_str(include_str!("../pricing.generated.json"))
        .expect("pricing.generated.json must be valid")
});

pub fn pricing_version() -> &'static str {
    CATALOG.generated_at.as_str()
}

pub fn model_price(_agent: AgentKind, model: &str) -> Option<ModelPrice> {
    let normalized = normalized_model(model);
    let entry = CATALOG
        .models
        .iter()
        .find(|entry| {
            entry.name == normalized || entry.aliases.iter().any(|alias| alias == &normalized)
        })
        .or_else(|| {
            CATALOG.models.iter().find(|entry| {
                is_versioned_match(&normalized, &entry.name)
                    || entry
                        .aliases
                        .iter()
                        .any(|alias| is_versioned_match(&normalized, alias))
            })
        })?;

    // The UI contract is USD. Keep non-USD source prices in the generated
    // catalog for auditability, but do not invent an exchange rate here.
    if !entry.currency.eq_ignore_ascii_case("USD") {
        return None;
    }
    Some(ModelPrice {
        input: entry.input,
        cache_read: entry.cache_read,
        cache_write: entry.cache_write,
        cache_write_1h: entry.cache_write_1h,
        output: entry.output,
    })
}

fn normalized_model(model: &str) -> String {
    let lowercase = model.trim().to_ascii_lowercase();
    let leaf = lowercase.rsplit('/').next().unwrap_or_default();
    let leaf = leaf
        .find("anthropic.")
        .map(|index| &leaf[index + "anthropic.".len()..])
        .unwrap_or(leaf);
    let mut normalized = String::with_capacity(leaf.len());
    let mut separator = false;
    for character in leaf.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !normalized.is_empty() {
                normalized.push('-');
            }
            normalized.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    normalized
}

fn is_versioned_match(model: &str, base: &str) -> bool {
    model
        .strip_prefix(base)
        .is_some_and(|suffix| suffix.starts_with('-'))
}

pub fn estimate_cost(agent: AgentKind, model: &str, usage: &TokenUsage) -> Option<f64> {
    let price = model_price(agent, model)?;
    let cache_write = match (usage.cache_write_tokens, price.cache_write) {
        (0, _) => 0.0,
        (_, Some(unit_price)) => usage.cache_write_tokens as f64 * unit_price,
        (_, None) => return None,
    };
    let cache_write_1h = match (usage.cache_write_1h_tokens, price.cache_write_1h) {
        (0, _) => 0.0,
        (_, Some(unit_price)) => usage.cache_write_1h_tokens as f64 * unit_price,
        (_, None) => return None,
    };
    let cost = usage.input_tokens as f64 * price.input
        + usage.cache_read_tokens as f64 * price.cache_read
        + cache_write
        + cache_write_1h
        + usage.output_tokens as f64 * price.output;
    Some(cost / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpt_56_cost_keeps_cached_input_separate() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_read_tokens: 2_000_000,
            ..TokenUsage::default()
        };
        let cost = estimate_cost(AgentKind::Codex, "gpt-5.6-sol", &usage).expect("price");
        assert!((cost - 9.0).abs() < f64::EPSILON);
    }

    #[test]
    fn official_alias_resolves_to_the_same_catalog_price() {
        let direct = model_price(AgentKind::Codex, "gpt-5.6-sol").expect("price");
        let alias = model_price(AgentKind::Codex, "gpt-5.6").expect("price");
        assert_eq!(direct.input, alias.input);
        assert_eq!(direct.cache_read, alias.cache_read);
        assert_eq!(direct.output, alias.output);
    }

    #[test]
    fn current_coding_provider_models_have_prices_and_aliases() {
        let cases = [
            (AgentKind::Codex, "openai-codex/gpt-5.6"),
            (AgentKind::Codex, "gpt-5.6-terra-2026-03-17"),
            (AgentKind::ClaudeCode, "claude-opus-5"),
            (AgentKind::ClaudeCode, "anthropic.claude-haiku-4-5-20251001"),
            (AgentKind::OpenClaw, "us.anthropic.claude-opus-4-8-v1:0"),
            (AgentKind::OpenClaw, "deepseek/deepseek-v4-pro"),
            (AgentKind::Hermes, "zai/glm-5.3"),
            (AgentKind::OpenClaw, "xai/grok-build-0.1"),
            (AgentKind::GrokBuild, "grok-4.6"),
            (AgentKind::Cursor, "composer-2.5-fast"),
        ];
        for (agent, model) in cases {
            assert!(
                model_price(agent, model).is_some(),
                "missing price for {model}"
            );
        }
        assert!(model_price(AgentKind::Cursor, "auto").is_none());
        assert!(model_price(AgentKind::Hermes, "unknown-model").is_none());
        assert!(model_price(AgentKind::KimiCode, "kimi-code/k3").is_none());
    }

    #[test]
    fn uses_current_openai_terra_and_luna_rates() {
        let terra = model_price(AgentKind::Codex, "gpt-5.6-terra").expect("price");
        assert_eq!(
            (terra.input, terra.cache_read, terra.output),
            (2.0, 0.2, 12.0)
        );
        let luna = model_price(AgentKind::Codex, "gpt-5.6-luna").expect("price");
        assert!((luna.input - 0.2).abs() < f64::EPSILON);
        assert!((luna.cache_read - 0.02).abs() < 1e-12);
        assert!((luna.output - 1.2).abs() < f64::EPSILON);
    }

    #[test]
    fn does_not_guess_an_unpublished_cache_write_rate() {
        let usage = TokenUsage {
            cache_write_tokens: 1,
            ..TokenUsage::default()
        };
        assert!(estimate_cost(AgentKind::GrokBuild, "grok-4.6", &usage).is_none());
        assert!(estimate_cost(AgentKind::Cursor, "composer-2.5", &usage).is_none());
        assert!(estimate_cost(AgentKind::ClaudeCode, "claude-sonnet-4-6", &usage).is_some());
    }
}
