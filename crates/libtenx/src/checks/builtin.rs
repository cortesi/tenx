use super::shell::Shell;
use super::Check;

pub fn builtin_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(Shell {
            name: "cargo-check".to_string(),
            command: "cargo check --tests".to_string(),
            globs: vec!["*.rs".to_string()],
            default_off: false,
            fail_on_stderr: false,
            mode: super::Mode::Both,
        }),
        Box::new(Shell {
            name: "cargo-test".to_string(),
            command: "cargo test -q".to_string(),
            globs: vec!["*.rs".to_string()],
            default_off: false,
            fail_on_stderr: false,
            mode: super::Mode::Both,
        }),
        Box::new(Shell {
            name: "cargo-clippy".to_string(),
            command: "cargo clippy --no-deps --all --tests -q".to_string(),
            globs: vec!["*.rs".to_string()],
            default_off: true,
            fail_on_stderr: true,
            mode: super::Mode::Both,
        }),
        Box::new(Shell {
            name: "python-ruff".to_string(),
            command: "ruff check -q".to_string(),
            globs: vec!["*.py".to_string()],
            default_off: false,
            fail_on_stderr: false,
            mode: super::Mode::Both,
        }),
    ]
}
