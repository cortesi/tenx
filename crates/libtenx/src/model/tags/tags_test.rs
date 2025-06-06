use std::path::PathBuf;

use indoc::indoc;
use pretty_assertions::assert_eq;

use state::{Operation, Patch, ReplaceFuzzy, WriteFile};

use crate::{
    error::Result,
    model::ModelResponse,
    session::{Action, Step},
    strategy, testutils,
};

use super::tags::*;

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
        strategy::StrategyState::Code(strategy::CodeState::default()),
    ))?;
    if let Some(step) = p.session.last_step_mut() {
        step.model_response = Some(response);
    }

    // let result = d
    //     .render_step_response(&Config::default(), &p.session, 0, 0)
    //     .unwrap();
    // assert_eq!(
    //     result,
    //     indoc! {r#"
    //             <comment>
    //             A comment
    //             </comment>
    //
    //             <edit>
    //             src/main.rs
    //             </edit>
    //             <edit>
    //             src/lib.rs
    //             </edit>
    //         "#}
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
