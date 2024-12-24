use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatChoice, ChatCompletionRequestAssistantMessageArgs,
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
    events::{send_event, Event},
    model::{
        conversation::{build_conversation, Conversation, ACK, EDITABLE_LEADIN},
        ModelProvider,
    },
    session::ModelResponse,
    session::Session,
    Result, TenxError,
};

use std::collections::HashMap;

/// OpenAI model implementation
#[derive(Default, Debug, Clone)]
pub struct OpenAi {
    pub name: String,
    pub api_model: String,
    pub openai_key: String,
    pub api_base: String,
    pub streaming: bool,
    pub no_system_prompt: bool,
}

impl Conversation<CreateChatCompletionRequest> for OpenAi {
    fn set_system_prompt(
        &self,
        req: &mut CreateChatCompletionRequest,
        prompt: String,
    ) -> Result<()> {
        if self.no_system_prompt {
            req.messages.push(
                ChatCompletionRequestUserMessageArgs::default()
                    .content(prompt)
                    .build()?
                    .into(),
            );
            req.messages.push(
                ChatCompletionRequestAssistantMessageArgs::default()
                    .content(ACK)
                    .build()?
                    .into(),
            );
        } else {
            req.messages.push(
                ChatCompletionRequestSystemMessageArgs::default()
                    .content(prompt)
                    .build()?
                    .into(),
            );
        }
        Ok(())
    }

    fn add_user_message(&self, req: &mut CreateChatCompletionRequest, text: String) -> Result<()> {
        req.messages.push(
            ChatCompletionRequestUserMessageArgs::default()
                .content(text)
                .build()?
                .into(),
        );
        Ok(())
    }

    fn add_agent_message(&self, req: &mut CreateChatCompletionRequest, text: &str) -> Result<()> {
        req.messages.push(
            ChatCompletionRequestAssistantMessageArgs::default()
                .content(text)
                .build()?
                .into(),
        );
        Ok(())
    }

    fn add_editables(
        &self,
        req: &mut CreateChatCompletionRequest,
        config: &Config,
        session: &Session,
        dialect: &Dialect,
        step_offset: usize,
    ) -> Result<()> {
        let editables = session.editables_for_step(step_offset)?;
        if !editables.is_empty() {
            self.add_user_message(
                req,
                format!(
                    "{}\n{}",
                    EDITABLE_LEADIN,
                    dialect.render_editables(config, session, editables)?
                ),
            )?;
            self.add_agent_message(req, ACK)?;
        }
        Ok(())
    }
}

/// OpenAI-specific usage information.
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct OpenAiUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

impl From<async_openai::error::OpenAIError> for TenxError {
    fn from(e: async_openai::error::OpenAIError) -> Self {
        if let async_openai::error::OpenAIError::Reqwest(ref e) = e {
            if let Some(status) = e.status() {
                if status == 429 || status == 529 {
                    return TenxError::Throttle(crate::throttle::Throttle::Backoff);
                }
            }
        }
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

    pub fn totals(&self) -> (u64, u64) {
        (
            self.prompt_tokens.unwrap_or(0) as u64,
            self.completion_tokens.unwrap_or(0) as u64,
        )
    }
}

impl OpenAi {
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
            usage: None,
        })
    }

    fn request(
        &self,
        config: &Config,
        session: &Session,
        dialect: &Dialect,
    ) -> Result<CreateChatCompletionRequest> {
        let mut req = CreateChatCompletionRequestArgs::default()
            .model(&self.api_model)
            .messages(Vec::new())
            .build()?;
        build_conversation(self, &mut req, config, session, dialect)?;
        Ok(req)
    }
}

#[async_trait]
impl ModelProvider for OpenAi {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn api_model(&self) -> String {
        self.api_model.clone()
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
        if self.api_model.is_empty() {
            return Err(TenxError::Model("Empty API model name".into()));
        }

        if !session.should_continue() {
            return Err(TenxError::Internal("No prompt to process.".into()));
        }

        let dialect = config.dialect()?;
        let openai_config = OpenAIConfig::new()
            .with_api_key(self.openai_key.clone())
            .with_api_base(&self.api_base);
        let client = Client::with_config(openai_config);
        let mut req = self.request(config, session, &dialect)?;

        trace!("Sending request: {:#?}", req);
        let resp = if self.streaming {
            req.stream = Some(true);
            self.stream_response(&client, req, sender).await?
        } else {
            let resp = client.chat().create(req).await?;
            if let Some(content) = resp.choices[0].message.content.as_ref() {
                send_event(&sender, Event::ModelResponse(content.to_string()))?;
            }
            resp
        };
        trace!("Got response: {:#?}", resp);

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
