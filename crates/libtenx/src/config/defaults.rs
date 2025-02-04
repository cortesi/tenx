use std::env;
use std::path::{Path, PathBuf};

use super::config::*;

const DEFAULT_STEP_LIMIT: usize = 16;

const ANTHROPIC_API_KEY: &str = "ANTHROPIC_API_KEY";
const ANTHROPIC_CLAUDE_SONNET: &str = "claude-3-5-sonnet-latest";
const ANTHROPIC_CLAUDE_HAIKU: &str = "claude-3-5-haiku-latest";

const OPENAI_API_BASE: &str = "https://api.openai.com/v1";
const OPENAI_API_KEY: &str = "OPENAI_API_KEY";
const OPENAI_GPT_O1_MINI: &str = "o1-mini";
const OPENAI_GPT_O1: &str = "o1";
const OPENAI_GPT_O3_MINI: &str = "o3-mini";
const OPENAI_GPT4O: &str = "gpt-4o";
const OPENAI_GPT4O_MINI: &str = "gpt-4o-mini";

const DEEPINFRA_API_KEY: &str = "DEEPINFRA_API_KEY";
const DEEPINFRA_API_BASE: &str = "https://api.deepinfra.com/v1/openai";

const DEEPSEEK_API_KEY: &str = "DEEPSEEK_API_KEY";
const DEEPSEEK_API_BASE: &str = "https://api.deepseek.com";

const XAI_API_KEY: &str = "XAI_API_KEY";
const XAI_API_BASE: &str = "https://api.x.ai/v1";
const XAI_DEFAULT_GROK: &str = "grok-beta";

const GOOGLEAI_API_KEY: &str = "GOOGLEAI_API_KEY";
const GOOGLEAI_GEMINI_15_PRO: &str = "gemini-1.5-pro";
const GOOGLEAI_GEMINI_15_FLASH: &str = "gemini-1.5-flash";
const GOOGLEAI_GEMINI_15_FLASH_8B: &str = "gemini-1.5-flash-8b";
const GOOGLEAI_GEMINI_FLASH_EXP: &str = "gemini-2.0-flash-exp";
const GOOGLEAI_GEMINI_EXP: &str = "gemini-exp-1206";
const GOOGLEAI_GEMINI_THINKING_EXP: &str = "gemini-2.0-flash-thinking-exp-01-21";

const GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";
const GROQ_LLAMA33_70B: &str = "llama-3.3-70b-versatile";
const GROQ_LLAMA31_8B_INSTANT: &str = "llama-3.1-8b-instant";
const GROQ_DEEPSEEK_R1: &str = "deepseek-r1-distill-llama-70b";
const GROQ_API_KEY: &str = "GROQ_API_KEY";

/// Returns true if the directory is a git repository
fn is_git_repo(dir: &Path) -> bool {
    dir.join(".git").is_dir()
}

/// Finds the root directory based on a specified working directory, git repo root, or .tenx.conf
/// file.
fn find_project_root(current_dir: &Path) -> PathBuf {
    let mut dir = current_dir.to_path_buf();
    loop {
        if is_git_repo(&dir) || dir.join(PROJECT_CONFIG_FILE).is_file() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    current_dir.to_path_buf()
}

/// Returns the default set of model configurations based on available API keys
fn default_models() -> Vec<Model> {
    let mut models = Vec::new();

    if env::var(ANTHROPIC_API_KEY).is_ok() {
        models.extend_from_slice(&[
            Model::Claude {
                name: "sonnet".to_string(),
                api_model: ANTHROPIC_CLAUDE_SONNET.to_string(),
                key: "".to_string(),
                key_env: ANTHROPIC_API_KEY.to_string(),
            },
            Model::Claude {
                name: "haiku".to_string(),
                api_model: ANTHROPIC_CLAUDE_HAIKU.to_string(),
                key: "".to_string(),
                key_env: ANTHROPIC_API_KEY.to_string(),
            },
        ]);
    }
    if env::var(DEEPSEEK_API_KEY).is_ok() {
        models.extend_from_slice(&[
            Model::OpenAi {
                name: "deepseek3".to_string(),
                api_model: "deepseek-chat".to_string(),
                key: "".to_string(),
                key_env: DEEPSEEK_API_KEY.to_string(),
                api_base: DEEPSEEK_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
                reasoning_effort: None,
            },
            Model::OpenAi {
                name: "deepseek-reasoner".to_string(),
                api_model: "deepseek-reasoner".to_string(),
                key: "".to_string(),
                key_env: DEEPSEEK_API_KEY.to_string(),
                api_base: DEEPSEEK_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
                reasoning_effort: None,
            },
        ]);
    }

    if env::var(DEEPINFRA_API_KEY).is_ok() {
        models.extend_from_slice(&[
            Model::OpenAi {
                name: "qwen".to_string(),
                api_model: "Qwen/Qwen2.5-Coder-32B-Instruct".to_string(),
                key: "".to_string(),
                key_env: DEEPINFRA_API_KEY.to_string(),
                api_base: DEEPINFRA_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
                reasoning_effort: None,
            },
            Model::OpenAi {
                name: "llama-8b-turbo".to_string(),
                api_model: "meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo".to_string(),
                key: "".to_string(),
                key_env: DEEPINFRA_API_KEY.to_string(),
                api_base: DEEPINFRA_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
                reasoning_effort: None,
            },
            Model::OpenAi {
                name: "llama-70b".to_string(),
                api_model: "meta-llama/Meta-Llama-3.1-70B-Instruct".to_string(),
                key: "".to_string(),
                key_env: DEEPINFRA_API_KEY.to_string(),
                api_base: DEEPINFRA_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
                reasoning_effort: None,
            },
            Model::OpenAi {
                name: "llama33-70b".to_string(),
                api_model: "meta-llama/Llama-3.3-70B-Instruct".to_string(),
                key: "".to_string(),
                key_env: DEEPINFRA_API_KEY.to_string(),
                api_base: DEEPINFRA_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
                reasoning_effort: None,
            },
            Model::OpenAi {
                name: "qwq".to_string(),
                api_model: "Qwen/QwQ-32B-Preview".to_string(),
                key: "".to_string(),
                key_env: DEEPINFRA_API_KEY.to_string(),
                api_base: DEEPINFRA_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
                reasoning_effort: None,
            },
        ]);
    }

    if env::var(OPENAI_API_KEY).is_ok() {
        models.extend_from_slice(&[
            Model::OpenAi {
                name: "o1".to_string(),
                api_model: OPENAI_GPT_O1.to_string(),
                key: "".to_string(),
                key_env: OPENAI_API_KEY.to_string(),
                api_base: OPENAI_API_BASE.to_string(),
                can_stream: false,
                no_system_prompt: true,
                reasoning_effort: None,
            },
            Model::OpenAi {
                name: "o1-mini".to_string(),
                api_model: OPENAI_GPT_O1_MINI.to_string(),
                key: "".to_string(),
                key_env: OPENAI_API_KEY.to_string(),
                api_base: OPENAI_API_BASE.to_string(),
                can_stream: false,
                no_system_prompt: true,
                reasoning_effort: None,
            },
            Model::OpenAi {
                name: "o3-mini-low".to_string(),
                api_model: OPENAI_GPT_O3_MINI.to_string(),
                key: "".to_string(),
                key_env: OPENAI_API_KEY.to_string(),
                api_base: OPENAI_API_BASE.to_string(),
                can_stream: false,
                no_system_prompt: true,
                reasoning_effort: Some(ReasoningEffort::Low),
            },
            Model::OpenAi {
                name: "o3-mini-medium".to_string(),
                api_model: OPENAI_GPT_O3_MINI.to_string(),
                key: "".to_string(),
                key_env: OPENAI_API_KEY.to_string(),
                api_base: OPENAI_API_BASE.to_string(),
                can_stream: false,
                no_system_prompt: true,
                reasoning_effort: Some(ReasoningEffort::Medium),
            },
            Model::OpenAi {
                name: "o3-mini-high".to_string(),
                api_model: OPENAI_GPT_O3_MINI.to_string(),
                key: "".to_string(),
                key_env: OPENAI_API_KEY.to_string(),
                api_base: OPENAI_API_BASE.to_string(),
                can_stream: false,
                no_system_prompt: true,
                reasoning_effort: Some(ReasoningEffort::High),
            },
            Model::OpenAi {
                name: "gpt4o".to_string(),
                api_model: OPENAI_GPT4O.to_string(),
                key: "".to_string(),
                key_env: OPENAI_API_KEY.to_string(),
                api_base: OPENAI_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
                reasoning_effort: None,
            },
            Model::OpenAi {
                name: "gpt4o-mini".to_string(),
                api_model: OPENAI_GPT4O_MINI.to_string(),
                key: "".to_string(),
                key_env: OPENAI_API_KEY.to_string(),
                api_base: OPENAI_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
                reasoning_effort: None,
            },
        ]);
    }

    if env::var(GROQ_API_KEY).is_ok() {
        models.extend_from_slice(&[
            Model::OpenAi {
                name: "groq-llama33-70b".to_string(),
                api_model: GROQ_LLAMA33_70B.to_string(),
                key: "".to_string(),
                key_env: GROQ_API_KEY.to_string(),
                api_base: GROQ_BASE_URL.to_string(),
                can_stream: true,
                no_system_prompt: true,
                reasoning_effort: None,
            },
            Model::OpenAi {
                name: "groq-llama31-8b".to_string(),
                api_model: GROQ_LLAMA31_8B_INSTANT.to_string(),
                key: "".to_string(),
                key_env: GROQ_API_KEY.to_string(),
                api_base: GROQ_BASE_URL.to_string(),
                can_stream: true,
                no_system_prompt: true,
                reasoning_effort: None,
            },
            Model::OpenAi {
                name: "groq-deepseek-r1".to_string(),
                api_model: GROQ_DEEPSEEK_R1.to_string(),
                key: "".to_string(),
                key_env: GROQ_API_KEY.to_string(),
                api_base: GROQ_BASE_URL.to_string(),
                can_stream: true,
                no_system_prompt: true,
                reasoning_effort: None,
            },
        ]);
    }

    if env::var(XAI_API_KEY).is_ok() {
        models.push(Model::OpenAi {
            name: "grok".to_string(),
            api_model: XAI_DEFAULT_GROK.to_string(),
            key: "".to_string(),
            key_env: XAI_API_KEY.to_string(),
            api_base: XAI_API_BASE.to_string(),
            can_stream: true,
            no_system_prompt: false,
            reasoning_effort: None,
        });
    }

    if env::var(GOOGLEAI_API_KEY).is_ok() {
        models.extend_from_slice(&[
            Model::Google {
                name: "gemini-exp".to_string(),
                api_model: GOOGLEAI_GEMINI_EXP.to_string(),
                key: "".to_string(),
                key_env: GOOGLEAI_API_KEY.to_string(),
                can_stream: false,
            },
            Model::Google {
                name: "gemini-flash-exp".to_string(),
                api_model: GOOGLEAI_GEMINI_FLASH_EXP.to_string(),
                key: "".to_string(),
                key_env: GOOGLEAI_API_KEY.to_string(),
                can_stream: false,
            },
            Model::Google {
                name: "gemini-flash-thinking-exp".to_string(),
                api_model: GOOGLEAI_GEMINI_THINKING_EXP.to_string(),
                key: "".to_string(),
                key_env: GOOGLEAI_API_KEY.to_string(),
                can_stream: false,
            },
            Model::Google {
                name: "gemini-15pro".to_string(),
                api_model: GOOGLEAI_GEMINI_15_PRO.to_string(),
                key: "".to_string(),
                key_env: GOOGLEAI_API_KEY.to_string(),
                can_stream: false,
            },
            Model::Google {
                name: "gemini-15flash".to_string(),
                api_model: GOOGLEAI_GEMINI_15_FLASH.to_string(),
                key: "".to_string(),
                key_env: GOOGLEAI_API_KEY.to_string(),
                can_stream: false,
            },
            Model::Google {
                name: "gemini-15flash8b".to_string(),
                api_model: GOOGLEAI_GEMINI_15_FLASH_8B.to_string(),
                key: "".to_string(),
                key_env: GOOGLEAI_API_KEY.to_string(),
                can_stream: false,
            },
        ]);
    }

    models
}

/// Returns the default set of check configurations
fn default_checks() -> Checks {
    Checks {
        builtin: vec![
            CheckConfig {
                name: "cargo-check".to_string(),
                command: "cargo check --tests".to_string(),
                globs: vec!["*.rs".to_string()],
                default_off: false,
                fail_on_stderr: false,
                mode: CheckMode::Both,
            },
            CheckConfig {
                name: "cargo-test".to_string(),
                command: "cargo test -q".to_string(),
                globs: vec!["*.rs".to_string()],
                default_off: false,
                fail_on_stderr: false,
                mode: CheckMode::Both,
            },
            CheckConfig {
                name: "cargo-clippy".to_string(),
                command: "cargo clippy --no-deps --all --tests -q".to_string(),
                globs: vec!["*.rs".to_string()],
                default_off: true,
                fail_on_stderr: true,
                mode: CheckMode::Both,
            },
            CheckConfig {
                name: "cargo-fmt".to_string(),
                command: "cargo fmt --all".to_string(),
                globs: vec!["*.rs".to_string()],
                default_off: false,
                fail_on_stderr: true,
                mode: CheckMode::Post,
            },
            CheckConfig {
                name: "ruff-check".to_string(),
                command: "ruff check -q".to_string(),
                globs: vec!["*.py".to_string()],
                default_off: false,
                fail_on_stderr: false,
                mode: CheckMode::Both,
            },
            CheckConfig {
                name: "ruff-format".to_string(),
                command: "ruff format -q".to_string(),
                globs: vec!["*.py".to_string()],
                default_off: false,
                fail_on_stderr: false,
                mode: CheckMode::Post,
            },
        ],
        ..Default::default()
    }
}

/// Constructs the Tenx default configuration. This takes into account the presence of various API
/// keys in env variables and sets up the default models accordingly.
pub fn default_config<P: AsRef<Path>>(current_dir: P) -> Config {
    Config {
        models: Models {
            default: "sonnet".to_string(),
            builtin: default_models(),
            ..Default::default()
        },
        context: Context {
            project_map: true,
            ..Default::default()
        },
        dialect: Dialect { edit: true },
        project: {
            let root = find_project_root(current_dir.as_ref());
            Project {
                include: vec![],
                root,
            }
        },
        tags: Tags { replace: true },
        session_store_dir: home_config_dir().join("state"),
        step_limit: DEFAULT_STEP_LIMIT,
        checks: default_checks(),
        ..Default::default()
    }
}
