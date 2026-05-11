use rmcp::model::{SamplingMessage, SamplingMessageContent};
use serde::de::DeserializeOwned;

pub(crate) fn message_text(message: &SamplingMessage) -> Result<String, String> {
    let parts = message
        .content
        .iter()
        .filter_map(|content| match content {
            SamplingMessageContent::Text(text) => Some(text.text.trim()),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return Err("sampling response did not contain text content".to_string());
    }
    Ok(parts.join("\n"))
}

pub(crate) fn parse_sampled_json<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    let payload = extract_json_payload(text)?;
    serde_json::from_str(payload).map_err(|error| format!("sampled JSON was invalid: {error}"))
}

fn extract_json_payload(text: &str) -> Result<&str, String> {
    let trimmed = strip_code_fence(text.trim());
    let object_start = trimmed.find('{');
    let array_start = trimmed.find('[');
    let Some(start) = [object_start, array_start].into_iter().flatten().min() else {
        return Err("sampling response did not include a JSON object or array".to_string());
    };
    let object_end = trimmed.rfind('}');
    let array_end = trimmed.rfind(']');
    let Some(end) = [object_end, array_end].into_iter().flatten().max() else {
        return Err("sampling response included incomplete JSON".to_string());
    };
    if end < start {
        return Err("sampling response JSON markers were invalid".to_string());
    }
    Ok(trimmed[start..=end].trim())
}

fn strip_code_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    let rest = rest
        .strip_prefix("json")
        .or_else(|| rest.strip_prefix("JSON"))
        .unwrap_or(rest);
    let rest = rest.trim_start_matches(|ch| ch == '\n' || ch == '\r' || ch == ' ');
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_from_fenced_markdown() {
        let parsed: serde_json::Value =
            parse_sampled_json("```json\n{\"value\": 1}\n```").expect("json");

        assert_eq!(parsed["value"], 1);
    }

    #[test]
    fn parses_json_embedded_in_text() {
        let parsed: serde_json::Value =
            parse_sampled_json("Here is the payload:\n{\"value\": 2}\nThanks").expect("json");

        assert_eq!(parsed["value"], 2);
    }

    #[test]
    fn parses_json_with_trailing_text() {
        let parsed: serde_json::Value = parse_sampled_json("{\"drafts\": []}\nDone").expect("json");

        assert!(parsed["drafts"].as_array().expect("drafts").is_empty());
    }
}
