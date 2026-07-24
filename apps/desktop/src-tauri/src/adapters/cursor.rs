use crate::adapters::common;
use crate::models::ParseState;
use serde_json::Value;

/// Cursor stores local agent transcripts as a compact role/message JSONL stream.
/// The files do not reliably include per-message timestamps, so ingestion supplies
/// the file modification time as the session anchor.
pub fn parse_record(state: &mut ParseState, record: &Value) {
    let role = record.get("role").and_then(Value::as_str);
    let message = record.get("message").unwrap_or(record);
    let text = message.get("content").and_then(common::text_from_message);

    match role {
        Some("user") => common::consider_title(state, text.as_deref()),
        Some("assistant") => common::consider_result(state, text.as_deref()),
        _ => common::mark_unknown(state),
    }

    // Cursor tool calls are represented as content items in newer transcript
    // versions. Record them when they are present without inventing activity for
    // older, text-only transcripts.
    if let Some(items) = message.get("content").and_then(Value::as_array) {
        for item in items {
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
            if item_type.contains("tool") {
                let name = item
                    .get("name")
                    .or_else(|| item.get("tool_name"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                common::record_tool(
                    state,
                    name,
                    item.get("input").or_else(|| item.get("arguments")),
                    None,
                );
            }
        }
    }
}
