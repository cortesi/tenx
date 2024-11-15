use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatChoice, ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
        ChatCompletionResponseMessage, CreateChatCompletionRequest,
        CreateChatCompletionRequestArgs, CreateChatCompletionResponse, FinishReason,
    },
    Client,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::trace;

use crate::{
    config::Config,
    dialect::{Dialect, DialectProvider},
    events::Event,
    model::ModelProvider,
    send_event,
    session::ModelResponse,
    Result, Session, TenxError,
};

use std::{collections::HashMap, path::PathBuf};

const CONTEXT_LEADIN: &str = "Here is some immutable context that you may not edit.\n";
const EDITABLE_LEADIN: &str =
    "Here are the editable files. You will modify only these, nothing else.\n";
const EDITABLE_UPDATE_LEADIN: &str = "Here are the updated files.";
const OMITTED_FILES_LEADIN: &str =
    "These files have been omitted since they were updated later in the conversation:";
const MAX_TOKENS: u32 = 8192;

fn render_editables_with_omitted(
    config: &Config,
    session: &Session,
    dialect: &Dialect,
    files: Vec<PathBuf>,
    omitted: Vec<PathBuf>,
) -> Result<String> {
    let mut result = dialect.render_editables(config, session, files)?;
    if !omitted.is_empty() {
        result.push_str(&format!("\n{}\n", OMITTED_FILES_LEADIN));
        for file in omitted {
            result.push_str(&format!("- {}\n", file.display()));
        }
    }
    Ok(result)
}

/// Model wrapper for OpenAI API
#[derive(Default, Debug, Clone)]
pub struct OpenAi {
    pub api_model: String,
    pub openai_key: String,
    pub streaming: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct OpenAiUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

impl From<async_openai::error::OpenAIError> for TenxError {
    fn from(e: async_openai::error::OpenAIError) -> Self {
        TenxError::Model(e.to_string())
    }
}

impl OpenAiUsage {
    pub fn values(&self) -> HashMap<String, u64> {
        let mut map = HashMap::new();
        if let Some(v) = self.prompt_tokens {
            map.insert("prompt_tokens".to_string(), v as u64);
        }
        if let Some(v) = self.completion_tokens {
            map.insert("completion_tokens".to_string(), v as u64);
        }
        if let Some(v) = self.total_tokens {
            map.insert("total_tokens".to_string(), v as u64);
        }
        map
    }
}

impl OpenAi {
    /// Creates a new OpenAi model instance
    pub fn new(api_model: String, openai_key: String, stream: bool) -> Result<Self> {
        if api_model.is_empty() {
            return Err(TenxError::Model("Empty API model name".into()));
        }
        if openai_key.is_empty() {
            return Err(TenxError::Model("Empty OpenAI API key".into()));
        }
        Ok(Self {
            api_model,
            openai_key,
            streaming: stream,
        })
    }

    async fn stream_response(
        &self,
        client: &Client<OpenAIConfig>,
        request: CreateChatCompletionRequest,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<CreateChatCompletionResponse> {
        let mut stream = client.chat().create_stream(request).await?;
        let mut full_response = String::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    for choice in response.choices {
                        if let Some(content) = &choice.delta.content {
                            full_response.push_str(content);
                            send_event(&sender, Event::Snippet(content.to_string()))?;
                        }
                    }
                }
                Err(err) => {
                    return Err(err.into());
                }
            }
        }

        #[allow(deprecated)]
        Ok(CreateChatCompletionResponse {
            id: "stream".to_string(),
            object: "chat.completion".to_string(),
            created: 0,
            model: self.api_model.clone(),
            system_fingerprint: None,
            service_tier: None,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatCompletionResponseMessage {
                    role: async_openai::types::Role::Assistant,
                    content: Some(full_response),
                    tool_calls: None,
                    refusal: None,
                    function_call: None,
                },
                finish_reason: Some(FinishReason::Stop),
                logprobs: None,
            }],
            usage: Some(async_openai::types::CompletionUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                completion_tokens_details: None,
                prompt_tokens_details: None,
            }),
        })
    }

    fn request(
        &self,
        config: &Config,
        session: &Session,
        dialect: &Dialect,
    ) -> Result<CreateChatCompletionRequest> {
        let mut messages: Vec<ChatCompletionRequestMessage> = Vec::new();

        messages.push(
            ChatCompletionRequestSystemMessageArgs::default()
                .content(dialect.system())
                .build()?
                .into(),
        );

        messages.push(
            ChatCompletionRequestUserMessageArgs::default()
                .content(format!(
                    "{}\n{}",
                    CONTEXT_LEADIN,
                    dialect.render_context(config, session)?
                ))
                .build()?
                .into(),
        );

        messages.push(
            ChatCompletionRequestAssistantMessageArgs::default()
                .content("Got it")
                .build()?
                .into(),
        );

        messages.push(
            ChatCompletionRequestUserMessageArgs::default()
                .content(format!("{}\n{}", EDITABLE_LEADIN, {
                    let (included, omitted) = session.partition_modified(session.editable(), 0);
                    render_editables_with_omitted(config, session, dialect, included, omitted)?
                }))
                .build()?
                .into(),
        );

        messages.push(
            ChatCompletionRequestAssistantMessageArgs::default()
                .content("Got it")
                .build()?
                .into(),
        );

        for (i, s) in session.steps().iter().enumerate() {
            messages.push(
                ChatCompletionRequestUserMessageArgs::default()
                    .content(dialect.render_step_request(config, session, i)?)
                    .build()?
                    .into(),
            );

            if let Some(resp) = &s.model_response {
                if let Some(patch) = &resp.patch {
                    messages.push(
                        ChatCompletionRequestAssistantMessageArgs::default()
                            .content(dialect.render_step_response(config, session, i)?)
                            .build()?
                            .into(),
                    );

                    messages.push(
                        ChatCompletionRequestUserMessageArgs::default()
                            .content(format!("{}\n{}", EDITABLE_UPDATE_LEADIN, {
                                let (included, omitted) =
                                    session.partition_modified(&patch.changed_files(), i);
                                render_editables_with_omitted(
                                    config, session, dialect, included, omitted,
                                )?
                            }))
                            .build()?
                            .into(),
                    );

                    messages.push(
                        ChatCompletionRequestAssistantMessageArgs::default()
                            .content("Got it.")
                            .build()?
                            .into(),
                    );
                }
            }
        }

        Ok(CreateChatCompletionRequestArgs::default()
            .model(&self.api_model)
            .messages(messages)
            .max_tokens(MAX_TOKENS)
            .stream(true)
            .build()?)
    }
}

#[async_trait]
impl ModelProvider for OpenAi {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<ModelResponse> {
        if self.openai_key.is_empty() {
            return Err(TenxError::Model("No OpenAI key configured.".into()));
        }

        if !session.should_continue() {
            return Err(TenxError::Internal("No prompt to process.".into()));
        }

        let dialect = config.dialect()?;
        let openai_config = OpenAIConfig::new().with_api_key(self.openai_key.clone());
        let client = Client::with_config(openai_config);
        let mut req = self.request(config, session, &dialect)?;

        trace!("Sending request: {:?}", req);
        let resp = if self.streaming {
            self.stream_response(&client, req, sender).await?
        } else {
            req.stream = Some(false);
            client.chat().create(req).await?
        };
        trace!("Got response: {:?}", resp);

        let mut modresp = if let Some(content) = resp.choices[0].message.content.as_ref() {
            dialect.parse(content)?
        } else {
            return Err(TenxError::Model("Empty response from OpenAI".into()));
        };

        if let Some(usage) = resp.usage {
            modresp.usage = Some(super::Usage::OpenAi(OpenAiUsage {
                prompt_tokens: Some(usage.prompt_tokens),
                completion_tokens: Some(usage.completion_tokens),
                total_tokens: Some(usage.total_tokens),
            }));
        }

        Ok(modresp)
    }

    fn render(&self, config: &Config, session: &Session) -> Result<String> {
        let dialect = config.dialect()?;
        let req = self.request(config, session, &dialect)?;
        Ok(format!("{:?}", req))
    }
}
