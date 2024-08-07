use crate::{Result, TenxError};
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::Reader;

#[derive(Debug)]
pub struct Diff {
    pub path: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug)]
pub struct WriteFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug)]
pub struct Response {
    merges: Vec<Diff>,
    files: Vec<WriteFile>,
}

/// Parses a response string containing XML-like tags and returns a `Response` struct.
///
/// The input string should contain one or more of the following tags:
///
/// `<write_file>` tag for file content:
/// ```xml
/// <write_file path="/path/to/file.txt">
///     File content goes here
/// </write_file>
/// ```
///
/// `<diff>` tag for file differences:
/// ```xml
/// <diff path="/path/to/diff_file.txt">
///     <old>Old content goes here</old>
///     <new>New content goes here</new>
/// </diff>
/// ```
///
/// The function parses these tags and populates a `Response` struct with
/// `WriteFile` entries for `<write_file>` tags and `Diff` entries for `<diff>` tags.
/// Whitespace is trimmed from the content of all tags. Any text outside of recognized tags is
/// ignored.
pub fn parse_response(response: &str) -> Result<Response> {
    let mut reader = Reader::from_str(response);
    reader.config_mut().trim_text(true);

    let mut response = Response {
        merges: Vec::new(),
        files: Vec::new(),
    };

    let mut buf = Vec::new();
    let mut current_tag = String::new();
    let mut current_path = String::new();
    let mut current_old = String::new();
    let mut current_new = String::new();
    let mut in_old = false;
    let mut in_new = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                println!("Start tag: {:?}", std::str::from_utf8(name.as_ref()));
                match name.as_ref() {
                    b"write_file" | b"diff" => {
                        current_tag = std::str::from_utf8(name.as_ref())
                            .map_err(|e| TenxError::ParseError(e.to_string()))?
                            .to_string();
                        current_path = get_path_attribute(e)?;
                        println!("Current tag: {}, Path: {}", current_tag, current_path);
                    }
                    b"old" => {
                        in_old = true;
                        println!("Entering <old> tag");
                    }
                    b"new" => {
                        in_new = true;
                        println!("Entering <new> tag");
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                let content = e
                    .unescape()
                    .map_err(|e| TenxError::ParseError(e.to_string()))?;
                println!("Text content: {:?}", content);
                match current_tag.as_str() {
                    "write_file" => {
                        response.files.push(WriteFile {
                            path: current_path.clone(),
                            content: content.trim().to_string(),
                        });
                        println!("Added write_file: {:?}", response.files.last().unwrap());
                    }
                    "diff" => {
                        if in_old {
                            current_old = content.trim().to_string();
                            println!("Set old content: {:?}", current_old);
                        } else if in_new {
                            current_new = content.trim().to_string();
                            println!("Set new content: {:?}", current_new);
                        }
                    }
                    _ => {} // Discard text outside of recognized tags
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                println!("End tag: {:?}", std::str::from_utf8(name.as_ref()));
                match name.as_ref() {
                    b"diff" => {
                        response.merges.push(Diff {
                            path: current_path.clone(),
                            old: current_old.clone(),
                            new: current_new.clone(),
                        });
                        println!("Added diff: {:?}", response.merges.last().unwrap());
                        current_old.clear();
                        current_new.clear();
                    }
                    b"old" => {
                        in_old = false;
                        println!("Exiting <old> tag");
                    }
                    b"new" => {
                        in_new = false;
                        println!("Exiting <new> tag");
                    }
                    _ => {}
                }
                if name.as_ref() == b"write_file" || name.as_ref() == b"diff" {
                    current_tag.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(TenxError::ParseError(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(response)
}

fn get_path_attribute(e: &BytesStart) -> Result<String> {
    let path_attr = e
        .attributes()
        .find(|a| a.as_ref().map(|a| a.key == QName(b"path")).unwrap_or(false))
        .ok_or_else(|| TenxError::ParseError("Missing path attribute".to_string()))?;

    let path_value = path_attr
        .map_err(|e| TenxError::ParseError(e.to_string()))?
        .unescape_value()
        .map_err(|e| TenxError::ParseError(e.to_string()))?;

    Ok(path_value.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_response_basic() {
        let input = r#"
            ignored
            <write_file path="/path/to/file.txt">
                This is the content of the file.
            </write_file>
            ignored
            <diff path="/path/to/diff_file.txt">
                <old>Old content</old>
                <new>New content</new>
            </diff>
            ignored
        "#;

        let result = parse_response(input).unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "/path/to/file.txt");
        assert_eq!(
            result.files[0].content.trim(),
            "This is the content of the file."
        );

        assert_eq!(result.merges.len(), 1);
        assert_eq!(result.merges[0].path, "/path/to/diff_file.txt");
        assert_eq!(result.merges[0].old.trim(), "Old content");
        assert_eq!(result.merges[0].new.trim(), "New content");
    }
}
