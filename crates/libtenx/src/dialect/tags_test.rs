use super::tags::*;
use super::*;

use crate::{
    session::{Action, Step},
    strategy, testutils,
};
use state::{Change, Patch, ReplaceFuzzy, WriteFile};

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

    let result = d
        .render_step_response(&Config::default(), &p.session, 0, 0)
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
