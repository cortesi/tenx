//! Defines an interaction style where files are sent to the model in XML-like tags, and model
//! responses are parsed from similar tags.

use super::xmlish::{self, tag};
use crate::{
    context::ContextItem,
    error::{Result, TenxError},
    session::ModelResponse,
};

use state::{Operation, Patch, ReplaceFuzzy, WriteFile};

pub const SYSTEM: &str = include_str!("./tags-system.txt");

pub const ACK: &str = "Got it.";

/// Parses a response string containing XML-like tags and returns a `Patch` struct.
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
/// `<replace>` tag for file replace:
/// ```xml
/// <replace path="/path/to/file.txt">
///     <old>Old content goes here</old>
///     <new>New content goes here</new>
/// </replace>
/// ```
///
/// The function parses these tags and populates an `Patch` struct with
/// `WriteFile` entries for `<write_file>` tags and `Replace` entries for `<replace>` tags.
/// Whitespace is trimmed from the content of all tags. Any text outside of recognized tags is
/// ignored.
pub fn parse(response: &str) -> Result<ModelResponse> {
    let mut patch = Patch::default();
    let mut lines = response.lines().map(String::from).peekable();
    let mut comment = None;

    while let Some(line) = lines.peek() {
        if let Some(tag) = xmlish::parse_open(line) {
            match tag.name.as_str() {
                "write_file" => {
                    let path = tag
                        .attributes
                        .get("path")
                        .ok_or_else(|| TenxError::ResponseParse {
                            user: "Failed to parse model response".into(),
                            model: format!(
                                "Missing path attribute in write_file tag. Line: '{line}'",
                            ),
                        })?
                        .clone();
                    let (_, content) = xmlish::parse_block("write_file", &mut lines)?;
                    patch.ops.push(Operation::Write(WriteFile {
                        path: path.into(),
                        content: content.join("\n"),
                    }));
                }
                "replace" => {
                    let path =
                        tag.attributes
                            .get("path")
                            .ok_or_else(|| TenxError::ResponseParse {
                                user: "Failed to parse model response".into(),
                                model: format!(
                                    "Missing path attribute in replace tag. Line: '{line}'",
                                ),
                            })?
                            .clone();
                    let (_, replace_content) = xmlish::parse_block("replace", &mut lines)?;
                    let mut replace_lines = replace_content.into_iter().peekable();
                    let (_, old) = xmlish::parse_block("old", &mut replace_lines)?;
                    let (_, new) = xmlish::parse_block("new", &mut replace_lines)?;
                    patch.ops.push(Operation::ReplaceFuzzy(ReplaceFuzzy {
                        path: path.into(),
                        old: old.join("\n"),
                        new: new.join("\n"),
                    }));
                }
                "comment" => {
                    let (_, content) = xmlish::parse_block("comment", &mut lines)?;
                    comment = Some(content.join("\n"));
                }
                "edit" => {
                    let (_, content) = xmlish::parse_block("edit", &mut lines)?;
                    for line in content {
                        let path = line.trim().to_string();
                        if !path.is_empty() {
                            patch.ops.push(Operation::View(path.clone().into()));
                        }
                    }
                }
                _ => {
                    lines.next();
                }
            }
        } else {
            lines.next();
        }
    }
    Ok(ModelResponse {
        patch: Some(patch),
        usage: None,
        comment,
        raw_response: Some(response.to_string()),
    })
}

pub fn render_prompt(prompt: &str) -> Result<String> {
    Ok(tag("prompt", [], prompt))
}

pub fn render_editable(path: &str, data: &str) -> Result<String> {
    Ok(tag("editable", [("path", path)], data))
}

pub fn render_comment(comment: &str) -> Result<String> {
    Ok(tag("comment", [], comment))
}

pub fn render_context(ctx: &ContextItem) -> Result<String> {
    let type_str = format!("{:?}", ctx.ty);
    Ok(tag(
        "context",
        [("name", ctx.source.as_str()), ("type", type_str.as_str())],
        &ctx.body,
    ))
}

pub fn render_patch(patch: &Patch) -> Result<String> {
    let mut rendered = String::new();
    for change in &patch.ops {
        match change {
            Operation::Write(write_file) => {
                let path_str = write_file.path.display().to_string();
                rendered.push_str(&tag(
                    "write_file",
                    [("path", path_str.as_str())],
                    &write_file.content,
                ));
            }
            Operation::ReplaceFuzzy(replace) => {
                let path_str = replace.path.display().to_string();

                let old_tag = tag("old", [], &replace.old);
                let new_tag = tag("new", [], &replace.new);
                let body = format!("{}{}", old_tag.trim_end(), new_tag);

                rendered.push_str(&tag("replace", [("path", path_str.as_str())], &body));
            }
            Operation::View(v) => {
                let path_str = v.display().to_string();
                rendered.push_str(&tag("edit", [], &path_str));
            }
            _ => {
                panic!("unsupported change type: {change:?}");
            }
        }
    }
    Ok(rendered)
}

pub fn render_model_response(mr: &ModelResponse) -> Result<String> {
    let mut rendered = String::new();
    if let Some(comment) = &mr.comment {
        rendered.push_str(&tag("comment", [], comment));
    }
    if let Some(patch) = &mr.patch {
        rendered.push_str(render_patch(patch)?.as_str());
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::{
        session::{Action, Step},
        strategy, testutils,
    };

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_response_basic() {
        let input = indoc! {r#"
            <comment>
            This is a comment.
            </comment>
            <write_file path="/path/to/file2.txt">
            This is the content of the file.
            </write_file>
            <replace path="/path/to/file.txt">
            <old>
            Old content
            </old>
            <new>
            New content
            </new>
            </replace>
        "#};

        let expected = ModelResponse {
            patch: Some(Patch {
                ops: vec![
                    Operation::Write(WriteFile {
                        path: PathBuf::from("/path/to/file2.txt"),
                        content: "This is the content of the file.".to_string(),
                    }),
                    Operation::ReplaceFuzzy(ReplaceFuzzy {
                        path: PathBuf::from("/path/to/file.txt"),
                        old: "Old content".to_string(),
                        new: "New content".to_string(),
                    }),
                ],
            }),
            usage: None,
            comment: Some("This is a comment.".to_string()),
            raw_response: Some(input.to_string()),
        };

        let result = parse(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_edit() {
        let input = indoc! {r#"
            <comment>
            Testing edit tag
            </comment>
            <edit>
            src/main.rs
            </edit>
            <edit>
                with/leading/spaces.rs
            </edit>
        "#};

        let result = parse(input).unwrap();
        assert_eq!(
            result.patch.unwrap().ops,
            vec![
                Operation::View(PathBuf::from("src/main.rs")),
                Operation::View(PathBuf::from("with/leading/spaces.rs")),
            ]
        );
    }

    #[test]
    fn test_render_edit() -> Result<()> {
        let mut p = testutils::test_project();

        let response = ModelResponse {
            comment: Some("A comment".into()),
            patch: Some(Patch {
                ops: vec![
                    Operation::View(PathBuf::from("src/main.rs")),
                    Operation::View(PathBuf::from("src/lib.rs")),
                ],
            }),
            usage: None,
            raw_response: Some("Test response".into()),
        };

        p.session.add_action(Action::new(
            &p.config,
            strategy::Strategy::Code(strategy::Code::default()),
        )?)?;
        p.session.last_action_mut()?.add_step(Step::new(
            "test_model".into(),
            "test".into(),
            strategy::StrategyStep::Code(strategy::CodeStep::default()),
        ))?;
        if let Some(step) = p.session.last_step_mut() {
            step.model_response = Some(response);
        }

        // let result = render_step_response(&p.session, 0, 0).unwrap();
        // assert_eq!(
        //     result,
        //     indoc! {r#"
        //         <comment>
        //         A comment
        //         </comment>
        //
        //         <edit>
        //         src/main.rs
        //         </edit>
        //         <edit>
        //         src/lib.rs
        //         </edit>
        //     "#}
        // );
        Ok(())
    }

    #[test]
    fn test_parse_edit_multiline() {
        let input = indoc! {r#"
            <edit>
            /path/to/first
            /path/to/second
            </edit>
        "#};

        let result = parse(input).unwrap();
        assert_eq!(
            result.patch.unwrap().ops,
            vec![
                Operation::View(PathBuf::from("/path/to/first")),
                Operation::View(PathBuf::from("/path/to/second")),
            ]
        );
    }

    #[test]
    fn test_xml_escaping() {
        // Test escaping in attributes
        let path = "/path/with/\"quotes\"/and/<brackets>&ampersands";
        let content = "Content with <tags> & \"quotes\" and 'apostrophes'";

        let result = render_editable(path, content).unwrap();

        // Check that special characters are properly escaped
        assert!(result.contains(
            "path=\"/path/with/&quot;quotes&quot;/and/&lt;brackets&gt;&amp;ampersands\""
        ));
        assert!(result.contains("Content with <tags> & \"quotes\" and 'apostrophes'"));

        // Test escaping in comment
        let comment = "This has <xml> tags & special \"chars\"";
        let result = render_comment(comment).unwrap();
        assert!(result.contains("This has <xml> tags & special \"chars\""));
    }
}
