use std::collections::BTreeSet;

pub(crate) const CLOSE_TRAILER: &str = "Platypus-Closes";
pub(crate) const VERIFICATION_TRAILER: &str = "Platypus-Verification";

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct PlatypusTrailers {
    pub closes: BTreeSet<String>,
    pub verification_present: bool,
}

pub(crate) fn parse_platypus_trailers(message: &str) -> PlatypusTrailers {
    let mut trailers = PlatypusTrailers::default();

    for line in message.lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.eq_ignore_ascii_case(CLOSE_TRAILER) {
            trailers.closes.extend(parse_item_ids(value));
        } else if key.eq_ignore_ascii_case(VERIFICATION_TRAILER) && !value.is_empty() {
            trailers.verification_present = true;
        }
    }

    trailers
}

fn parse_item_ids(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split([',', ' '])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_uppercase())
        .filter(|value| valid_item_id_shape(value))
}

fn valid_item_id_shape(value: &str) -> bool {
    let Some((prefix, number)) = value.split_once('-') else {
        return false;
    };
    !prefix.is_empty()
        && prefix
            .chars()
            .all(|character| character.is_ascii_uppercase())
        && number.len() == 3
        && number.chars().all(|character| character.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_platypus_trailers_from_one_block() {
        let parsed = parse_platypus_trailers(
            "Subject\n\nPlatypus-Closes: MCP-001, MCP-002\nPlatypus-Verification: make check",
        );

        assert!(parsed.closes.contains("MCP-001"));
        assert!(parsed.closes.contains("MCP-002"));
        assert!(parsed.verification_present);
    }

    #[test]
    fn parses_platypus_trailers_split_across_paragraphs() {
        let parsed = parse_platypus_trailers(
            "Subject\n\nPlatypus-Closes: MCP-019\n\nPlatypus-Verification: make check",
        );

        assert_eq!(parsed.closes.into_iter().collect::<Vec<_>>(), ["MCP-019"]);
        assert!(parsed.verification_present);
    }

    #[test]
    fn ignores_non_item_values_and_empty_verification() {
        let parsed = parse_platypus_trailers(
            "Subject\n\nPlatypus-Closes: not-an-id MCP-1234 MCP-003\nPlatypus-Verification:",
        );

        assert_eq!(parsed.closes.into_iter().collect::<Vec<_>>(), ["MCP-003"]);
        assert!(!parsed.verification_present);
    }
}
