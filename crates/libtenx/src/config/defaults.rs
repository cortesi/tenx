use std::env;

use super::config::*;

const DEFAULT_RETRY_LIMIT: usize = 16;

const ANTHROPIC_API_KEY: &str = "ANTHROPIC_API_KEY";
const ANTHROPIC_CLAUDE_SONNET: &str = "claude-3-5-sonnet-latest";
const ANTHROPIC_CLAUDE_HAIKU: &str = "claude-3-5-haiku-latest";

const OPENAI_API_KEY: &str = "OPENAI_API_KEY";
const OPENAI_GPT_O1_PREVIEW: &str = "o1-preview";
const OPENAI_GPT_O1_MINI: &str = "o1-mini";
const OPENAI_GPT4O: &str = "gpt-4o";
const OPENAI_GPT4O_MINI: &str = "gpt-4o-mini";

const DEEPINFRA_API_KEY: &str = "DEEPINFRA_API_KEY";
const DEEPINFRA_API_BASE: &str = "https://api.deepinfra.com/v1/openai";

const XAI_API_KEY: &str = "XAI_API_KEY";
const XAI_API_BASE: &str = "https://api.x.ai/v1";
const XAI_DEFAULT_GROK: &str = "grok-beta";

const GOOGLEAI_API_KEY: &str = "GOOGLEAI_API_KEY";
const GOOGLEAI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/openai";
const GOOGLEAI_GEMINI_EXP: &str = "gemini-exp-1114";

pub fn default_config() -> Config {
    let mut models = Vec::new();

    if env::var(ANTHROPIC_API_KEY).is_ok() {
        models.extend_from_slice(&[
            ModelConfig::Claude {
                name: "sonnet".to_string(),
                api_model: ANTHROPIC_CLAUDE_SONNET.to_string(),
                key: "".to_string(),
                key_env: ANTHROPIC_API_KEY.to_string(),
            },
            ModelConfig::Claude {
                name: "haiku".to_string(),
                api_model: ANTHROPIC_CLAUDE_HAIKU.to_string(),
                key: "".to_string(),
                key_env: ANTHROPIC_API_KEY.to_string(),
            },
        ]);
    }

    if env::var(DEEPINFRA_API_KEY).is_ok() {
        models.push(ModelConfig::OpenAi {
            name: "qwen-coder".to_string(),
            api_model: "Qwen/Qwen2.5-Coder-32B-Instruct".to_string(),
            key: "".to_string(),
            key_env: DEEPINFRA_API_KEY.to_string(),
            api_base: DEEPINFRA_API_BASE.to_string(),
            can_stream: true,
            no_system_prompt: false,
        });
    }

    if env::var(OPENAI_API_KEY).is_ok() {
        models.extend_from_slice(&[
            ModelConfig::OpenAi {
                name: "o1".to_string(),
                api_model: OPENAI_GPT_O1_PREVIEW.to_string(),
                key: "".to_string(),
                key_env: OPENAI_API_KEY.to_string(),
                api_base: crate::model::OPENAI_API_BASE.to_string(),
                can_stream: false,
                no_system_prompt: true,
            },
            ModelConfig::OpenAi {
                name: "o1-mini".to_string(),
                api_model: OPENAI_GPT_O1_MINI.to_string(),
                key: "".to_string(),
                key_env: OPENAI_API_KEY.to_string(),
                api_base: crate::model::openai::OPENAI_API_BASE.to_string(),
                can_stream: false,
                no_system_prompt: true,
            },
            ModelConfig::OpenAi {
                name: "gpt4o".to_string(),
                api_model: OPENAI_GPT4O.to_string(),
                key: "".to_string(),
                key_env: OPENAI_API_KEY.to_string(),
                api_base: crate::model::openai::OPENAI_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
            },
            ModelConfig::OpenAi {
                name: "gpt4o-mini".to_string(),
                api_model: OPENAI_GPT4O_MINI.to_string(),
                key: "".to_string(),
                key_env: OPENAI_API_KEY.to_string(),
                api_base: crate::model::openai::OPENAI_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
            },
        ]);
    }

    if env::var(XAI_API_KEY).is_ok() {
        models.push(ModelConfig::OpenAi {
            name: "grok".to_string(),
            api_model: XAI_DEFAULT_GROK.to_string(),
            key: "".to_string(),
            key_env: XAI_API_KEY.to_string(),
            api_base: XAI_API_BASE.to_string(),
            can_stream: true,
            no_system_prompt: false,
        });
    }

    if env::var(GOOGLEAI_API_KEY).is_ok() {
        models.push(ModelConfig::OpenAi {
            name: "gemini".to_string(),
            api_model: GOOGLEAI_GEMINI_EXP.to_string(),
            key: "".to_string(),
            key_env: GOOGLEAI_API_KEY.to_string(),
            api_base: GOOGLEAI_API_BASE.to_string(),
            can_stream: false,
            no_system_prompt: false,
        });
    }

    Config {
        include: Include::Git,
        exclude: Vec::new(),
        models,
        session_store_dir: home_config_dir().join("state"),
        retry_limit: DEFAULT_RETRY_LIMIT,
        default_dialect: ConfigDialect::default(),
        dummy_model: None,
        dummy_dialect: None,
        tags: Tags::default(),
        ops: Ops::default(),
        default_context: DefaultContext::default(),
        default_model: None,
        full: false,
        checks: Checks::default(),
        project_root: ProjectRoot::default(),
        test_cwd: None,
        no_stream: false,
    }
}
