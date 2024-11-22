use super::{Check, Mode};

pub fn builtin_checks() -> Vec<Check> {
    vec![
        Check {
            name: "cargo-check".to_string(),
            command: "cargo check --tests".to_string(),
            globs: vec!["*.rs".to_string()],
            default_off: false,
            fail_on_stderr: false,
            mode: Mode::Both,
        },
        Check {
            name: "cargo-test".to_string(),
            command: "cargo test -q".to_string(),
            globs: vec!["*.rs".to_string()],
            default_off: false,
            fail_on_stderr: false,
            mode: Mode::Both,
        },
        Check {
            name: "cargo-clippy".to_string(),
            command: "cargo clippy --no-deps --all --tests -q".to_string(),
            globs: vec!["*.rs".to_string()],
            default_off: true,
            fail_on_stderr: true,
            mode: Mode::Both,
        },
        Check {
            name: "cargo-fmt".to_string(),
            command: "cargo fmt --all".to_string(),
            globs: vec!["*.rs".to_string()],
            default_off: false,
            fail_on_stderr: true,
            mode: Mode::Post,
        },
        Check {
            name: "python-ruff".to_string(),
            command: "ruff check -q".to_string(),
            globs: vec!["*.py".to_string()],
            default_off: false,
            fail_on_stderr: false,
            mode: Mode::Both,
        },
    ]
}
