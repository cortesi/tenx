//! Simple helpers for parsing xml-ish content

use std::collections::HashMap;

/// Represents an XML-like tag with a name and attributes.
pub struct Tag {
    name: String,
    attributes: HashMap<String, String>,
}

impl Tag {
    /// Creates a new Tag with the given name and attributes.
    fn new(name: String, attributes: HashMap<String, String>) -> Self {
        Tag { name, attributes }
    }
}

/// Attempts to parse a line containing an XML-like opening tag.
///
/// Returns Some(Tag) if successful, None otherwise.
pub fn parse_open(line: &str) -> Option<Tag> {
    let trimmed = line.trim();
    if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
        return None;
    }

    let content = &trimmed[1..trimmed.len() - 1];
    let mut parts = content.split_whitespace();

    let name = parts.next()?.to_string();
    let mut attributes = HashMap::new();

    for attr in parts {
        let mut kv = attr.splitn(2, '=');
        if let (Some(key), Some(value)) = (kv.next(), kv.next()) {
            let cleaned_value = value.trim_matches('"');
            attributes.insert(key.to_string(), cleaned_value.to_string());
        }
    }

    Some(Tag::new(name, attributes))
}

/// Checks if the given line is a well-formed close tag for the specified tag name.
pub fn is_close(line: &str, tag_name: &str) -> bool {
    let trimmed = line.trim();
    trimmed == format!("</{}>", tag_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_open() {
        let test_cases = vec![
            ("< tag >", Some(("tag", vec![]))),
            ("<tag>", Some(("tag", vec![]))),
            (
                "<tag attr1=\"value1\">",
                Some(("tag", vec![("attr1", "value1")])),
            ),
            (
                "<tag attr1=\"value1\" attr2=\"value2\">",
                Some(("tag", vec![("attr1", "value1"), ("attr2", "value2")])),
            ),
            (" <tag> ", Some(("tag", vec![]))),
            ("not a tag", None),
            ("<>", None),
            ("<tag", None),
            ("tag>", None),
        ];

        for (input, expected) in test_cases {
            let result = parse_open(input);
            match expected {
                Some((name, attrs)) => {
                    let tag = result.unwrap();
                    assert_eq!(tag.name, name);
                    assert_eq!(tag.attributes.len(), attrs.len());
                    for (k, v) in attrs {
                        assert_eq!(tag.attributes.get(k), Some(&v.to_string()));
                    }
                }
                None => assert!(result.is_none()),
            }
        }
    }

    #[test]
    fn test_is_close() {
        assert!(is_close("</tag>", "tag"));
        assert!(is_close(" </tag> ", "tag"));
        assert!(!is_close("<tag>", "tag"));
        assert!(!is_close("</tag>", "other"));
        assert!(!is_close("< /tag>", "tag"));
        assert!(!is_close("</tag >", "tag"));
        assert!(!is_close("</tag attr=\"value\">", "tag"));
    }
}

