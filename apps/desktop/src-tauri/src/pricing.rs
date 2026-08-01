use crate::models::{AgentKind, TokenUsage};

#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    pub input: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub cache_write_1h: f64,
    pub output: f64,
}

pub const PRICING_VERSION: &str = "2026-08-01.2";

pub fn model_price(agent: AgentKind, model: &str) -> Option<ModelPrice> {
    let normalized = normalized_model(model);

    if matches_model(&normalized, "gpt-5.6")
        || matches_model(&normalized, "gpt-5.6-sol")
        || matches_model(&normalized, "gpt-5.5")
    {
        return Some(openai_price(5.0, 30.0));
    }
    if matches_model(&normalized, "gpt-5.6-terra") {
        return Some(openai_price(2.5, 15.0));
    }
    if matches_model(&normalized, "gpt-5.6-luna") {
        return Some(openai_price(1.0, 6.0));
    }
    if matches_model(&normalized, "gpt-5.4-mini") {
        return Some(openai_price(0.75, 4.5));
    }
    if matches_model(&normalized, "gpt-5.4-nano") {
        return Some(openai_price(0.20, 1.25));
    }
    if matches_model(&normalized, "gpt-5.4") {
        return Some(openai_price(2.5, 15.0));
    }
    if matches_model(&normalized, "gpt-5.3-codex") || matches_model(&normalized, "gpt-5.2-codex") {
        return Some(openai_price(1.75, 14.0));
    }
    if matches_model(&normalized, "gpt-5.1-codex-mini") || matches_model(&normalized, "gpt-5-mini")
    {
        return Some(openai_price(0.25, 2.0));
    }
    if matches_model(&normalized, "gpt-5.1-codex")
        || matches_model(&normalized, "gpt-5.1")
        || matches_model(&normalized, "gpt-5-codex")
        || matches_model(&normalized, "gpt-5")
    {
        return Some(openai_price(1.25, 10.0));
    }
    if matches_model(&normalized, "codex-mini-latest") {
        return Some(standard_price(1.5, 0.375, 6.0));
    }

    if matches_model(&normalized, "claude-fable-5")
        || matches_model(&normalized, "claude-mythos-5")
        || matches_model(&normalized, "claude-mythos-preview")
    {
        return Some(anthropic_price(10.0, 50.0));
    }
    if matches_model(&normalized, "claude-opus-5")
        || matches_model(&normalized, "claude-opus-4-8")
        || matches_model(&normalized, "claude-opus-4-7")
        || matches_model(&normalized, "claude-opus-4-6")
    {
        return Some(anthropic_price(5.0, 25.0));
    }
    if matches_model(&normalized, "claude-sonnet-5") {
        // Anthropic's introductory Sonnet 5 rate is active through 2026-08-31.
        return Some(anthropic_price(2.0, 10.0));
    }
    if matches_model(&normalized, "claude-sonnet-4-6")
        || matches_model(&normalized, "claude-sonnet-4-5")
    {
        return Some(anthropic_price(3.0, 15.0));
    }
    if matches_model(&normalized, "claude-haiku-4-5") {
        return Some(anthropic_price(1.0, 5.0));
    }

    match normalized.as_str() {
        "deepseek-v4-flash" | "deepseek-chat" | "deepseek-reasoner" => {
            return Some(standard_price(0.14, 0.0028, 0.28));
        }
        "deepseek-v4-pro" => return Some(standard_price(0.435, 0.003625, 0.87)),
        "kimi-k3" => return Some(standard_price(3.0, 0.30, 15.0)),
        "kimi-k2.7-code" => return Some(standard_price(0.95, 0.19, 4.0)),
        "kimi-k2.7-code-highspeed" => return Some(standard_price(1.90, 0.38, 8.0)),
        "kimi-k2.6" => return Some(standard_price(0.95, 0.16, 4.0)),
        "kimi-k2.5" => return Some(standard_price(0.60, 0.10, 3.0)),
        "glm-5.1" => return Some(standard_price(1.4, 0.26, 4.4)),
        "glm-5" => return Some(standard_price(1.0, 0.20, 3.2)),
        "glm-5-turbo" => return Some(standard_price(1.2, 0.24, 4.0)),
        "glm-4.7" | "glm-4.6" | "glm-4.5" => {
            return Some(standard_price(0.6, 0.11, 2.2));
        }
        "glm-4.7-flashx" => return Some(standard_price(0.07, 0.01, 0.4)),
        "glm-4.7-flash" | "glm-4.5-flash" => return Some(standard_price(0.0, 0.0, 0.0)),
        "glm-4.5-x" => return Some(standard_price(2.2, 0.45, 8.9)),
        "glm-4.5-air" => return Some(standard_price(0.2, 0.03, 1.1)),
        "glm-4.5-airx" => return Some(standard_price(1.1, 0.22, 4.5)),
        "glm-4-32b-0414-128k" => return Some(standard_price(0.1, 0.1, 0.1)),
        "grok-4.5" | "grok-4.5-latest" => return Some(standard_price(2.0, 0.30, 6.0)),
        "grok-build-0.1" | "grok-code-fast-1" | "grok-code-fast" | "grok-code-fast-1-0825" => {
            return Some(standard_price(1.0, 0.20, 2.0));
        }
        "grok-4.3"
        | "grok-4.3-latest"
        | "grok-4.20-multi-agent-0309"
        | "grok-4.20-0309-reasoning"
        | "grok-4.20-0309-non-reasoning" => {
            return Some(standard_price(1.25, 0.20, 2.5));
        }
        "grok-4-1-fast-reasoning" | "grok-4-1-fast-non-reasoning" => {
            return Some(standard_price(0.20, 0.05, 0.50));
        }
        "composer-2.5" | "composer-2" => return Some(standard_price(0.50, 0.50, 2.50)),
        "composer-2.5-fast" | "composer-2-fast" => {
            return Some(standard_price(3.0, 3.0, 15.0));
        }
        _ => {}
    }

    if agent == AgentKind::KimiCode && normalized == "k3" {
        return Some(standard_price(3.0, 0.30, 15.0));
    }
    None
}

fn normalized_model(model: &str) -> String {
    let normalized = model.trim().to_ascii_lowercase().replace('_', "-");
    let leaf = normalized.rsplit('/').next().unwrap_or(&normalized);
    let leaf = leaf
        .find("anthropic.")
        .map(|index| &leaf[index + "anthropic.".len()..])
        .unwrap_or(leaf);
    leaf.replace("claude-opus-4.6", "claude-opus-4-6")
        .replace("claude-sonnet-4.6", "claude-sonnet-4-6")
}

fn matches_model(model: &str, base: &str) -> bool {
    model == base
        || model.strip_prefix(base).is_some_and(|suffix| {
            let dashed = suffix.strip_prefix('-').is_some_and(|value| {
                value
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_digit())
                    || value.starts_with("v1")
            });
            let vertex = suffix
                .strip_prefix('@')
                .and_then(|value| value.chars().next())
                .is_some_and(|character| character.is_ascii_digit());
            dashed || vertex
        })
}

fn openai_price(input: f64, output: f64) -> ModelPrice {
    ModelPrice {
        input,
        cache_read: input * 0.1,
        cache_write: input * 1.25,
        cache_write_1h: input * 1.25,
        output,
    }
}

fn anthropic_price(input: f64, output: f64) -> ModelPrice {
    ModelPrice {
        input,
        cache_read: input * 0.1,
        cache_write: input * 1.25,
        cache_write_1h: input * 2.0,
        output,
    }
}

fn standard_price(input: f64, cache_read: f64, output: f64) -> ModelPrice {
    ModelPrice {
        input,
        cache_read,
        cache_write: input,
        cache_write_1h: input,
        output,
    }
}

pub fn estimate_cost(agent: AgentKind, model: &str, usage: &TokenUsage) -> Option<f64> {
    let price = model_price(agent, model)?;
    let cost = usage.input_tokens as f64 * price.input
        + usage.cache_read_tokens as f64 * price.cache_read
        + usage.cache_write_tokens as f64 * price.cache_write
        + usage.cache_write_1h_tokens as f64 * price.cache_write_1h
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
    fn historical_gpt_55_uses_its_published_api_equivalent_rate() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_read_tokens: 2_000_000,
            ..TokenUsage::default()
        };
        let cost = estimate_cost(AgentKind::Codex, "gpt-5.5", &usage).expect("price");
        assert!((cost - 9.0).abs() < f64::EPSILON);
    }

    #[test]
    fn current_coding_provider_models_have_prices_and_aliases() {
        let cases = [
            (AgentKind::Codex, "openai-codex/gpt-5.4"),
            (AgentKind::Codex, "gpt-5.4-mini-2026-03-17"),
            (AgentKind::ClaudeCode, "claude-opus-5"),
            (AgentKind::ClaudeCode, "anthropic.claude-haiku-4-5-20251001"),
            (AgentKind::OpenClaw, "us.anthropic.claude-opus-4-8-v1:0"),
            (AgentKind::OpenClaw, "deepseek/deepseek-v4-pro"),
            (AgentKind::KimiCode, "kimi-code/k3"),
            (AgentKind::Hermes, "zai/glm-5.1"),
            (AgentKind::OpenClaw, "xai/grok-build-0.1"),
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
    }
}
