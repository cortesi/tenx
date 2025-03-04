//! Defines an interaction style where files are sent to the model in XML-like tags, and model
//! responses are parsed from similar tags.

use std::path::PathBuf;

use super::{xmlish, DialectProvider};
use crate::{
    config::Config,
    context::ContextProvider,
    patch::{Change, Patch, Replace, WriteFile},
    session::{ModelResponse, Session},
    Result, TenxError,
};
use fs_err as fs;

const SYSTEM: &str = include_str!("./tags-system.txt");
const REPLACE: &str = include_str!("./tags-replace.txt");
const EDIT: &str = include_str!("./tags-edit.txt");

/// Tenx's primary code generation dialect, which uses XML-ish tags as the basic communication format with models.
#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct Tags {
    pub replace: bool,
    pub edit: bool,
}

impl Tags {
    pub fn new(replace: bool, edit: bool) -> Self {
        Self { replace, edit }
    }
}

impl DialectProvider for Tags {
    fn name(&self) -> &'static str {
        "tags"
    }

    fn system(&self) -> String {
        let mut out = SYSTEM.to_string();
        if self.replace {
            out.push_str(REPLACE);
        }
        if self.edit {
            out.push_str(EDIT);
        }
        out
    }

    fn render_context(&self, config: &Config, s: &Session) -> Result<String> {
        if self.system().is_empty() {
            return Ok("There is no non-editable context.".into());
        }

        let mut rendered = String::new();
        rendered.push_str("<context>\n");
        for cspec in s.contexts() {
            for ctx in cspec.context_items(config, s)? {
                let txt = format!(
                    "<item name=\"{}\" type=\"{:?}\">\n{}\n</item>\n",
                    ctx.source, ctx.ty, ctx.body
                );
                rendered.push_str(&txt)
            }
        }
        rendered.push_str("</context>");
        Ok(rendered)
    }

    fn render_editables(
        &self,
        config: &Config,
        _session: &Session,
        paths: Vec<PathBuf>,
    ) -> Result<String> {
        let mut rendered = String::new();
        for path in paths {
            let contents = fs::read_to_string(config.abspath(&path)?)?;
            rendered.push_str(&format!(
                "<editable path=\"{}\">\n{}</editable>\n\n",
                path.display(),
                contents
            ));
        }
        Ok(rendered)
    }

    fn render_step_request(
        &self,
        _config: &Config,
        session: &Session,
        offset: usize,
    ) -> Result<String> {
        let steps = session.steps();
        let prompt = steps
            .get(offset)
            .ok_or_else(|| TenxError::Internal("Invalid prompt offset".into()))?;
        let mut rendered = String::new();
        rendered.push_str(&format!(
            "\n<prompt>\n{}\n</prompt>\n\n",
            &prompt.raw_prompt
        ));
        Ok(rendered)
    }

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
    fn parse(&self, response: &str) -> Result<ModelResponse> {
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
                                    "Missing path attribute in write_file tag. Line: '{}'",
                                    line
                                ),
                            })?
                            .clone();
                        let (_, content) = xmlish::parse_block("write_file", &mut lines)?;
                        patch.changes.push(Change::Write(WriteFile {
                            path: path.into(),
                            content: content.join("\n"),
                        }));
                    }
                    "replace" => {
                        let path = tag
                            .attributes
                            .get("path")
                            .ok_or_else(|| TenxError::ResponseParse {
                                user: "Failed to parse model response".into(),
                                model: format!(
                                    "Missing path attribute in replace tag. Line: '{}'",
                                    line
                                ),
                            })?
                            .clone();
                        let (_, replace_content) = xmlish::parse_block("replace", &mut lines)?;
                        let mut replace_lines = replace_content.into_iter().peekable();
                        let (_, old) = xmlish::parse_block("old", &mut replace_lines)?;
                        let (_, new) = xmlish::parse_block("new", &mut replace_lines)?;
                        patch.changes.push(Change::Replace(Replace {
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
                                patch.changes.push(Change::View(path.clone().into()));
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
            operations: vec![],
            usage: None,
            comment,
            raw_response: Some(response.to_string()),
        })
    }

    fn render_step_response(
        &self,
        _config: &Config,
        session: &Session,
        offset: usize,
    ) -> Result<String> {
        let steps = session.steps();
        let step = steps
            .get(offset)
            .ok_or_else(|| TenxError::Internal("Invalid step offset".into()))?;
        if let Some(resp) = &step.model_response {
            let mut rendered = String::new();
            if let Some(comment) = &resp.comment {
                rendered.push_str(&format!("<comment>\n{}\n</comment>\n\n", comment));
            }
            if let Some(patch) = &resp.patch {
                for change in &patch.changes {
                    match change {
                        Change::Write(write_file) => {
                            rendered.push_str(&format!(
                                "<write_file path=\"{}\">\n{}\n</write_file>\n\n",
                                write_file.path.display(),
                                write_file.content
                            ));
                        }
                        Change::Replace(replace) => {
                            rendered.push_str(&format!(
                            "<replace path=\"{}\">\n<old>\n{}\n</old>\n<new>\n{}\n</new>\n</replace>\n\n",
                            replace.path.display(),
                            replace.old,
                            replace.new
                        ));
                        }
                        Change::View(v) => {
                            rendered.push_str(&format!("<edit>\n{}\n</edit>\n", v.display()));
                        }
                    }
                }
            }
            Ok(rendered)
        } else {
            Ok(String::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        session::{Action, Step},
        strategy, testutils,
    };

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_response_basic() {
        let d = Tags {
            replace: true,
            edit: false,
        };

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
                changes: vec![
                    Change::Write(WriteFile {
                        path: PathBuf::from("/path/to/file2.txt"),
                        content: "This is the content of the file.".to_string(),
                    }),
                    Change::Replace(Replace {
                        path: PathBuf::from("/path/to/file.txt"),
                        old: "Old content".to_string(),
                        new: "New content".to_string(),
                    }),
                ],
            }),
            operations: vec![],
            usage: None,
            comment: Some("This is a comment.".to_string()),
            raw_response: Some(input.to_string()),
        };

        let result = d.parse(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_edit() {
        let d = Tags::default();

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

        let result = d.parse(input).unwrap();
        assert_eq!(
            result.patch.unwrap().changes,
            vec![
                Change::View(PathBuf::from("src/main.rs")),
                Change::View(PathBuf::from("with/leading/spaces.rs")),
            ]
        );
    }

    #[test]
    fn test_render_edit() -> Result<()> {
        let mut p = testutils::test_project();

        let d = Tags::default();

        let response = ModelResponse {
            comment: Some("A comment".into()),
            patch: Some(Patch {
                changes: vec![
                    Change::View(PathBuf::from("src/main.rs")),
                    Change::View(PathBuf::from("src/lib.rs")),
                ],
            }),
            operations: vec![],
            usage: None,
            raw_response: Some("Test response".into()),
        };

        p.session.add_action(Action::new(
            &p.config,
            strategy::Strategy::Code(strategy::Code::new()),
        )?)?;
        p.session
            .last_action_mut()?
            .add_step(Step::new("test_model".into(), "test".into()))?;
        if let Some(step) = p.session.last_step_mut() {
            step.model_response = Some(response);
        }

        let result = d
            .render_step_response(&Config::default(), &p.session, 0)
            .unwrap();
        assert_eq!(
            result,
            indoc! {r#"
                <comment>
                A comment
                </comment>

                <edit>
                src/main.rs
                </edit>
                <edit>
                src/lib.rs
                </edit>
            "#}
        );
        Ok(())
    }

    #[test]
    fn test_parse_edit_multiline() {
        let d = Tags::default();

        let input = indoc! {r#"
            <edit>
            /path/to/first
            /path/to/second
            </edit>
        "#};

        let result = d.parse(input).unwrap();
        assert_eq!(
            result.patch.unwrap().changes,
            vec![
                Change::View(PathBuf::from("/path/to/first")),
                Change::View(PathBuf::from("/path/to/second")),
            ]
        );
    }

    #[test]
    fn test_render_system() {
        let tags_with_smart = Tags {
            replace: true,
            edit: false,
        };
        let tags_without_smart = Tags {
            replace: true,
            edit: false,
        };

        // Test with smart enabled
        let _system_with_smart = tags_with_smart.system();

        // Test without smart
        let _system_without_smart = tags_without_smart.system();
    }
}
