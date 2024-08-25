//! Simple helpers for parsing xml-ish content

use crate::error::{Result, TenxError};
use std::collections::HashMap;

/// Represents an XML-like tag with a name and attributes.
#[derive(Debug)]
pub struct Tag {
    pub name: String,
    pub attributes: HashMap<String, String>,
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
    let trimmed = line.trim_start();
    if !trimmed.starts_with('<') {
        return None;
    }

    let end = trimmed.find('>')?;
    let content = &trimmed[1..end];
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

/// Checks if the given line contains a well-formed close tag for the specified tag name.
pub fn is_close(line: &str, tag_name: &str) -> bool {
    line.contains(&format!("</{}>", tag_name))
}

/// Parses a block of XML-like content, starting with an opening tag and ending with a matching closing tag.
pub fn parse_block<I>(tag_name: &str, lines: &mut I) -> Result<(Tag, Vec<String>)>
where
    I: Iterator<Item = String>,
{
    let opening_line = lines
        .next()
        .ok_or_else(|| TenxError::ResponseParse("Expected opening tag".into()))?;
    let tag = parse_open(&opening_line)
        .ok_or_else(|| TenxError::ResponseParse("Invalid opening tag".into()))?;

    if tag.name != tag_name {
        return Err(TenxError::ResponseParse(format!(
            "Expected tag {}, found {}",
            tag_name, tag.name
        )));
    }

    let mut contents = Vec::new();
    if let Some(first_content) = opening_line.split('>').nth(1) {
        if !first_content.trim().is_empty() {
            contents.push(first_content.to_string());
        }
    }

    for line in lines {
        if is_close(&line, tag_name) {
            if let Some(last_content) = line.split('<').next() {
                if !last_content.trim().is_empty() {
                    contents.push(last_content.to_string());
                }
            }
            return Ok((tag, contents));
        }
        contents.push(line);
    }

    Err(TenxError::ResponseParse(format!(
        "Closing tag not found for {}",
        tag_name
    )))
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
            ("<tag>trailing content", Some(("tag", vec![]))),
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
                    assert_eq!(tag.name, name, "Failed for input: {}", input);
                    assert_eq!(
                        tag.attributes.len(),
                        attrs.len(),
                        "Failed for input: {}",
                        input
                    );
                    for (k, v) in attrs {
                        assert_eq!(
                            tag.attributes.get(k),
                            Some(&v.to_string()),
                            "Failed for input: {}",
                            input
                        );
                    }
                }
                None => assert!(result.is_none(), "Failed for input: {}", input),
            }
        }
    }

    #[test]
    fn test_is_close() {
        assert!(is_close("</tag>", "tag"));
        assert!(is_close(" </tag> ", "tag"));
        assert!(is_close("leading content</tag>", "tag"));
        assert!(is_close("leading content</tag>trailing content", "tag"));
        assert!(!is_close("<tag>", "tag"));
        assert!(!is_close("</tag>", "other"));
        assert!(!is_close("< /tag>", "tag"));
        assert!(!is_close("</tag >", "tag"));
        assert!(!is_close("</tag attr=\"value\">", "tag"));
    }

    #[test]
    fn test_parse_block() {
        let input = vec![
            "<test attr=\"value\">",
            "Content line 1",
            "Content line 2",
            "</test>",
        ];
        let mut iter = input.into_iter().map(String::from);
        let result = parse_block("test", &mut iter);
        assert!(result.is_ok());
        let (tag, contents) = result.unwrap();
        assert_eq!(tag.name, "test");
        assert_eq!(tag.attributes.get("attr"), Some(&"value".to_string()));
        assert_eq!(contents, vec!["Content line 1", "Content line 2"]);
    }

    #[test]
    fn test_parse_block_with_leading_and_trailing_data() {
        let input = vec![
            "<test attr=\"value\">leading data",
            "Content line 1",
            "Content line 2",
            "trailing data</test>",
        ];
        let mut iter = input.into_iter().map(String::from);
        let result = parse_block("test", &mut iter);
        assert!(result.is_ok(), "parse_block failed: {:?}", result);
        let (tag, contents) = result.unwrap();
        assert_eq!(tag.name, "test");
        assert_eq!(tag.attributes.get("attr"), Some(&"value".to_string()));
        assert_eq!(
            contents,
            vec![
                "leading data",
                "Content line 1",
                "Content line 2",
                "trailing data",
            ],
            "Contents mismatch"
        );
    }

    #[test]
    fn test_parse_block_with_nested_tags() {
        let input = vec![
            "<outer>",
            "  <inner>",
            "    Inner content",
            "  </inner>",
            "  Outer content",
            "</outer>",
        ];
        let mut iter = input.into_iter().map(String::from);
        let result = parse_block("outer", &mut iter);
        assert!(result.is_ok());
        let (tag, contents) = result.unwrap();
        assert_eq!(tag.name, "outer");
        assert_eq!(
            contents,
            vec![
                "  <inner>",
                "    Inner content",
                "  </inner>",
                "  Outer content",
            ]
        );
    }
}
