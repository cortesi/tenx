use crate::{Result, TenxError};

pub struct Merge {
    pub path: String,
    pub content: String,
}

pub struct File {
    pub path: String,
    pub content: String,
}

pub struct Response {
    merges: Vec<Merge>,
    files: Vec<File>,
}

pub fn parse_response(response: &str) -> Result<Response> {
    let lines = response.lines();
    let mut response = Response {
        merges: Vec::new(),
        files: Vec::new(),
    };

    let mut current_block: Option<(String, String, bool)> = None; // (path, content, is_merge)

    for line in lines {
        if line.starts_with("<file path=\"") || line.starts_with("<merge path=\"") {
            let is_merge = line.starts_with("<merge");
            let path = line
                .split('"')
                .nth(1)
                .ok_or_else(|| TenxError::ParseError("Invalid path attribute".to_string()))?
                .to_string();
            current_block = Some((path, String::new(), is_merge));
        } else if line == "</file>" || line == "</merge>" {
            if let Some((path, content, is_merge)) = current_block.take() {
                if is_merge {
                    response.merges.push(Merge { path, content });
                } else {
                    response.files.push(File { path, content });
                }
            }
        } else if let Some((_, content, _)) = &mut current_block {
            content.push_str(line);
            content.push('\n');
        }
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_parse_response_basic() {
        let input = indoc! {r#"
            Some text before
            <file path="src/main.rs">
            fn main() {
                println!("Hello, world!");
            }
            </file>
            Some text in between
            <merge path="src/lib.rs">
            pub fn add(a: i32, b: i32) -> i32 {
                a + b
            }
            </merge>
            Some text after
        "#};

        let result = parse_response(input).expect("Failed to parse response");

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.merges.len(), 1);

        let file = &result.files[0];
        assert_eq!(file.path, "src/main.rs");
        assert_eq!(
            file.content,
            "fn main() {\n    println!(\"Hello, world!\");\n}\n"
        );

        let merge = &result.merges[0];
        assert_eq!(merge.path, "src/lib.rs");
        assert_eq!(
            merge.content,
            "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n"
        );
    }

    #[test]
    fn test_parse_response_multiple_and_empty_blocks() {
        let input = indoc! {r#"
            <file path="src/main.rs">
            fn main() {
                println!("Hello, world!");
            }
            </file>
            <merge path="src/lib.rs">
            pub fn add(a: i32, b: i32) -> i32 {
                a + b
            }
            </merge>
            <file path="src/empty_file.rs">
            </file>
            <merge path="src/constants.rs">
            pub const PI: f64 = 3.14159265359;
            </merge>
        "#};

        let result = parse_response(input).expect("Failed to parse response");

        assert_eq!(result.files.len(), 2);
        assert_eq!(result.merges.len(), 2);

        // Check first file
        let file1 = &result.files[0];
        assert_eq!(file1.path, "src/main.rs");
        assert_eq!(
            file1.content,
            "fn main() {\n    println!(\"Hello, world!\");\n}\n"
        );

        // Check second file (empty)
        let file2 = &result.files[1];
        assert_eq!(file2.path, "src/empty_file.rs");
        assert_eq!(file2.content, "");

        // Check first merge
        let merge1 = &result.merges[0];
        assert_eq!(merge1.path, "src/lib.rs");
        assert_eq!(
            merge1.content,
            "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n"
        );

        // Check second merge
        let merge2 = &result.merges[1];
        assert_eq!(merge2.path, "src/constants.rs");
        assert_eq!(merge2.content, "pub const PI: f64 = 3.14159265359;\n");
    }
}
