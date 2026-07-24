use crate::models::{AgentKind, TokenUsage};

#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    pub input: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub cache_write_1h: f64,
    pub output: f64,
}

pub const PRICING_VERSION: &str = "2026-07-19.1";

pub fn model_price(agent: AgentKind, model: &str) -> Option<ModelPrice> {
    let normalized = model.to_ascii_lowercase();
    match agent {
        AgentKind::Codex => match normalized.as_str() {
            "gpt-5.6" | "gpt-5.6-sol" => Some(openai_price(5.0, 30.0)),
            "gpt-5.6-terra" => Some(openai_price(2.5, 15.0)),
            "gpt-5.6-luna" => Some(openai_price(1.0, 6.0)),
            _ => None,
        },
        AgentKind::ClaudeCode => match normalized.as_str() {
            "claude-opus-4-6" => Some(anthropic_price(5.0, 25.0)),
            "claude-sonnet-4-6" => Some(anthropic_price(3.0, 15.0)),
            _ => None,
        },
        AgentKind::KimiCode | AgentKind::Cursor | AgentKind::OpenClaw | AgentKind::Hermes => None,
    }
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
}
