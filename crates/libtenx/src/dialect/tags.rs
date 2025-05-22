//! Defines an interaction style where files are sent to the model in XML-like tags, and model
//! responses are parsed from similar tags.

use super::{xmlish, DialectProvider};
use crate::{
    config::Config,
    context::ContextProvider,
    error::{Result, TenxError},
    model::Chat,
    session::{ModelResponse, Session},
};
use fs_err as fs;
use state::{Change, Patch, ReplaceFuzzy, WriteFile};

const SYSTEM: &str = include_str!("./tags-system.txt");
const REPLACE: &str = include_str!("./tags-replace.txt");
const EDIT: &str = include_str!("./tags-edit.txt");

// Constants for conversation structure
const CONTEXT_LEADIN: &str = "Here is some immutable context that you may not edit.";
const EDITABLE_LEADIN: &str = "Here are the editable files.";
const ACK: &str = "Got it.";

/// Tenx's primary code generation dialect, which uses XML-ish tags as the basic communication format with models.
#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct Tags {}

impl Tags {
    pub fn new() -> Self {
        Self {}
    }

    fn render_step_request(
        &self,
        session: &Session,
        action_offset: usize,
        step_offset: usize,
    ) -> Result<String> {
        let step = &session.actions[action_offset].steps[step_offset];
        let mut rendered = String::new();
        rendered.push_str(&format!("\n<prompt>\n{}\n</prompt>\n\n", &step.raw_prompt));
        Ok(rendered)
    }

    fn render_step_response(
        &self,
        session: &Session,
        action_offset: usize,
        step_offset: usize,
    ) -> Result<String> {
        let step = &session.actions[action_offset].steps[step_offset];
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
                        Change::ReplaceFuzzy(replace) => {
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
                        v => {
                            panic!("unsupported change type: {:?}", v);
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

impl DialectProvider for Tags {
    fn name(&self) -> &'static str {
        "tags"
    }

    fn system(&self) -> String {
        let mut out = SYSTEM.to_string();
        out.push_str(REPLACE);
        out.push_str(EDIT);
        out
    }

    fn build_chat(
        &self,
        config: &Config,
        session: &Session,
        action_offset: usize,
        chat: &mut Box<dyn Chat>,
    ) -> Result<()> {
        chat.add_system_prompt(&self.system())?;

        if !session.contexts.is_empty() {
            chat.add_user_message(CONTEXT_LEADIN)?;
            for cspec in &session.contexts {
                for ctx in cspec.context_items(config, session)? {
                    let txt = format!(
                        "<context name=\"{}\" type=\"{:?}\">\n{}\n</context>\n",
                        ctx.source, ctx.ty, ctx.body
                    );
                    chat.add_context(&ctx)?;
                }
            }
            chat.add_agent_message(ACK)?;
        }

        for (i, step) in session.actions[action_offset].steps.iter().enumerate() {
            let editables = session.editables_for_step_state(action_offset, i)?;
            if !editables.is_empty() {
                chat.add_user_message(EDITABLE_LEADIN)?;
                for path in editables {
                    let contents = fs::read_to_string(config.abspath(&path)?)?;
                    let txt = &format!(
                        "<editable path=\"{}\">\n{}</editable>\n\n",
                        path.display(),
                        contents
                    );
                    chat.add_editable(&path.display().to_string(), txt)?;
                }
                chat.add_agent_message(ACK)?;
            }

            // Add the step request
            chat.add_user_message(&self.render_step_request(session, action_offset, i)?)?;

            // Add the step response if available
            if step.model_response.is_some() {
                chat.add_agent_message(&self.render_step_response(session, action_offset, i)?)?;
            } else if i != session.actions[action_offset].steps.len() - 1 {
                // We have no model response, but we're not the last step
                chat.add_agent_message("omitted due to error")?;
            }
        }

        Ok(())
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
                        patch.changes.push(Change::ReplaceFuzzy(ReplaceFuzzy {
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
        let d = Tags {};

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
                    Change::ReplaceFuzzy(ReplaceFuzzy {
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
        p.session.last_action_mut()?.add_step(Step::new(
            "test_model".into(),
            "test".into(),
            strategy::StrategyStep::Code(strategy::CodeStep::default()),
        ))?;
        if let Some(step) = p.session.last_step_mut() {
            step.model_response = Some(response);
        }

        let result = d.render_step_response(&p.session, 0, 0).unwrap();
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
}
