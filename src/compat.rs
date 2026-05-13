use serde::{de::DeserializeOwned, de::Error, Deserialize, Deserializer};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn deserialize_option_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let Some(value) = Option::<Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    match value {
        Value::Bool(value) => Ok(Some(value)),
        Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "" => Ok(None),
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            other => Err(D::Error::custom(format!(
                "expected boolean or string boolean, got `{other}`"
            ))),
        },
        other => Err(D::Error::custom(format!(
            "expected boolean or string boolean, got {other}"
        ))),
    }
}

pub fn deserialize_option_usize<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    let Some(value) = Option::<Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    match value {
        Value::Number(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| D::Error::custom("expected non-negative integer")),
        Value::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else {
                value.parse::<usize>().map(Some).map_err(|_| {
                    D::Error::custom(format!("expected integer string, got `{value}`"))
                })
            }
        }
        other => Err(D::Error::custom(format!(
            "expected integer or integer string, got {other}"
        ))),
    }
}

pub fn deserialize_option_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let Some(value) = Option::<Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    match value {
        Value::Number(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| D::Error::custom("expected non-negative integer")),
        Value::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else {
                value.parse::<u64>().map(Some).map_err(|_| {
                    D::Error::custom(format!("expected integer string, got `{value}`"))
                })
            }
        }
        other => Err(D::Error::custom(format!(
            "expected integer or integer string, got {other}"
        ))),
    }
}

pub fn deserialize_vec_string<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?.unwrap_or(Value::Array(Vec::new()));
    vec_string_from_value(value).map_err(D::Error::custom)
}

pub fn deserialize_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = Option::<Value>::deserialize(deserializer)?.unwrap_or(Value::Array(Vec::new()));
    match coerce_json_string(value).map_err(D::Error::custom)? {
        Value::Array(items) => items
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(D::Error::custom),
        other => Err(D::Error::custom(format!(
            "expected array or JSON array string, got {other}"
        ))),
    }
}

pub fn deserialize_map<'de, D>(deserializer: D) -> Result<BTreeMap<String, Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let value =
        Option::<Value>::deserialize(deserializer)?.unwrap_or(Value::Object(Default::default()));
    match coerce_json_string(value).map_err(D::Error::custom)? {
        Value::Object(object) => Ok(object.into_iter().collect()),
        other => Err(D::Error::custom(format!(
            "expected object or JSON object string, got {other}"
        ))),
    }
}

fn vec_string_from_value(value: Value) -> Result<Vec<String>, String> {
    match coerce_json_string(value)? {
        Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                Value::String(value) => Ok(value),
                other => Err(format!("expected string array item, got {other}")),
            })
            .collect(),
        Value::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![value.to_string()])
            }
        }
        other => Err(format!(
            "expected string array or JSON array string, got {other}"
        )),
    }
}

fn coerce_json_string(value: Value) -> Result<Value, String> {
    let Value::String(text) = value else {
        return Ok(value);
    };
    let trimmed = text.trim();
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        serde_json::from_str(trimmed)
            .map_err(|error| format!("could not parse JSON encoded string: {error}"))
    } else {
        Ok(Value::String(text))
    }
}
