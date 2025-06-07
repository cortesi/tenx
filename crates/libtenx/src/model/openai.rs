use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatChoice, ChatCompletionRequestAssistantMessageArgs,
        ChatCompletionRequestDeveloperMessageArgs, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, ChatCompletionResponseMessage,
        CreateChatCompletionRequest, CreateChatCompletionRequestArgs, CreateChatCompletionResponse,
        FinishReason,
    },
    Client,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::trace;

use crate::{
    checks::CheckResult,
    context::ContextItem,
    error::{Result, TenxError},
    events::{send_event, Event, EventSender},
    model::tags,
    model::{Chat, ModelProvider},
    session::ModelResponse,
    throttle::Throttle,
};

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

/// OpenAI model implementation
#[derive(Default, Debug, Clone)]
pub struct OpenAi {
    pub name: String,
    pub api_model: String,
    pub openai_key: String,
    pub api_base: String,
    pub streaming: bool,
    pub no_system_prompt: bool,
    /// For OpenAI o1 and o3 models only.
    pub reasoning_effort: Option<ReasoningEffort>,
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

/// A chat implementation for OpenAI models.
#[derive(Debug, Clone)]
pub struct OpenAiChat {
    /// Upstream model name to use
    pub api_model: String,
    /// The OpenAI API key
    pub openai_key: String,
    /// Custom API base URL
    pub api_base: String,
    /// Whether to stream responses
    pub streaming: bool,
    /// Whether to skip using system prompt
    pub no_system_prompt: bool,
    /// Reasoning effort level for o1/o3 models
    pub reasoning_effort: Option<ReasoningEffort>,
    /// The request being built
    request: CreateChatCompletionRequest,
    /// Last response from the model
    response: Option<ChatCompletionResponseMessage>,
}

impl OpenAiChat {
    /// Creates a new OpenAiChat configured for the given model, API key, and settings.
    pub fn new(
        api_model: String,
        openai_key: String,
        api_base: String,
        streaming: bool,
        no_system_prompt: bool,
        reasoning_effort: Option<ReasoningEffort>,
    ) -> Result<Self> {
        let mut ra = CreateChatCompletionRequestArgs::default();
        ra.model(&api_model);

        // Add system prompt based on configuration
        let mut messages = Vec::new();
        if no_system_prompt {
            messages.push(
                ChatCompletionRequestDeveloperMessageArgs::default()
                    .content(tags::SYSTEM)
                    .build()?
                    .into(),
            );
        } else {
            messages.push(
                ChatCompletionRequestSystemMessageArgs::default()
                    .content(tags::SYSTEM)
                    .build()?
                    .into(),
            );
        }

        ra.messages(messages);

        if let Some(ref re) = reasoning_effort {
            ra.reasoning_effort(match re {
                ReasoningEffort::Low => async_openai::types::ReasoningEffort::Low,
                ReasoningEffort::Medium => async_openai::types::ReasoningEffort::Medium,
                ReasoningEffort::High => async_openai::types::ReasoningEffort::High,
            });
        }

        Ok(Self {
            api_model,
            openai_key,
            api_base,
            streaming,
            no_system_prompt,
            reasoning_effort,
            request: ra.build()?,
            response: None,
        })
    }

    /// Helper to add or append a message with the given role.
    fn add_message_with_role(&mut self, role: async_openai::types::Role, text: &str) -> Result<()> {
        // For OpenAI, we'll just add new messages without consolidation
        // since the API structure makes it complex to modify existing messages
        let message = match role {
            async_openai::types::Role::User => ChatCompletionRequestUserMessageArgs::default()
                .content(text.trim())
                .build()?
                .into(),
            async_openai::types::Role::Assistant => {
                ChatCompletionRequestAssistantMessageArgs::default()
                    .content(text.trim())
                    .build()?
                    .into()
            }
            async_openai::types::Role::System => ChatCompletionRequestSystemMessageArgs::default()
                .content(text.trim())
                .build()?
                .into(),
            _ => return Err(TenxError::Internal("Unsupported role".into())),
        };

        self.request.messages.push(message);
        Ok(())
    }

    async fn stream_response(
        &self,
        sender: Option<EventSender>,
    ) -> Result<CreateChatCompletionResponse> {
        let openai_config = OpenAIConfig::new()
            .with_api_key(self.openai_key.clone())
            .with_api_base(&self.api_base);
        let client = Client::with_config(openai_config);

        let mut req = self.request.clone();
        req.stream = Some(true);

        let mut stream = client.chat().create_stream(req).await?;
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
                    audio: None,
                },
                finish_reason: Some(FinishReason::Stop),
                logprobs: None,
            }],
            usage: None,
        })
    }

    fn extract_changes(&self) -> Result<ModelResponse> {
        if let Some(response) = &self.response {
            if let Some(content) = &response.content {
                if content.is_empty() {
                    return Err(TenxError::Throttle(Throttle::Backoff));
                }
                return tags::parse(content);
            }
        }
        Err(TenxError::Internal("No patch to parse.".into()))
    }
}

#[async_trait]
impl Chat for OpenAiChat {
    fn add_system_prompt(&mut self, prompt: &str) -> Result<()> {
        // For simplicity, we'll just add new system messages
        // The initial system prompt is already added in the constructor
        if self.no_system_prompt {
            self.request.messages.push(
                ChatCompletionRequestDeveloperMessageArgs::default()
                    .content(prompt)
                    .build()?
                    .into(),
            );
        } else {
            self.request.messages.push(
                ChatCompletionRequestSystemMessageArgs::default()
                    .content(prompt)
                    .build()?
                    .into(),
            );
        }
        Ok(())
    }

    fn add_user_message(&mut self, text: &str) -> Result<()> {
        self.add_message_with_role(async_openai::types::Role::User, text)
    }

    fn add_agent_message(&mut self, text: &str) -> Result<()> {
        self.add_message_with_role(async_openai::types::Role::Assistant, text)
    }

    fn add_context(&mut self, ctx: &ContextItem) -> Result<()> {
        self.add_user_message(&tags::render_context(ctx)?)
    }

    fn add_editable(&mut self, path: &str, data: &str) -> Result<()> {
        self.add_user_message(&tags::render_editable(path, data)?)
    }

    fn add_agent_patch(&mut self, patch: &crate::model::Patch) -> Result<()> {
        self.add_agent_message(&tags::render_patch(patch)?)
    }

    fn add_agent_comment(&mut self, comment: &str) -> Result<()> {
        self.add_agent_message(&tags::render_comment(comment)?)
    }

    fn add_user_prompt(&mut self, prompt: &str) -> Result<()> {
        self.add_user_message(&tags::render_prompt(prompt)?)
    }

    fn add_user_check_results(&mut self, results: &[CheckResult]) -> Result<()> {
        if !results.is_empty() {
            let rendered = tags::render_check_results(results)?;
            self.add_user_message(&rendered)?;
        }
        Ok(())
    }

    fn add_user_patch_failure(
        &mut self,
        patch_failures: &[crate::model::PatchFailure],
    ) -> Result<()> {
        if !patch_failures.is_empty() {
            let rendered = tags::render_patch_failures(patch_failures)?;
            self.add_user_message(&rendered)?;
        }
        Ok(())
    }

    async fn send(&mut self, sender: Option<EventSender>) -> Result<ModelResponse> {
        if self.openai_key.is_empty() {
            return Err(TenxError::Model("No OpenAI key configured.".into()));
        }
        if self.api_model.is_empty() {
            return Err(TenxError::Model("Empty API model name".into()));
        }

        self.request.model = self.api_model.clone();
        if let Some(ref re) = self.reasoning_effort {
            self.request.reasoning_effort = Some(match re {
                ReasoningEffort::Low => async_openai::types::ReasoningEffort::Low,
                ReasoningEffort::Medium => async_openai::types::ReasoningEffort::Medium,
                ReasoningEffort::High => async_openai::types::ReasoningEffort::High,
            });
        }

        trace!("Sending request: {:?}", self.request);

        let resp = if self.streaming {
            self.stream_response(sender.clone()).await?
        } else {
            let openai_config = OpenAIConfig::new()
                .with_api_key(self.openai_key.clone())
                .with_api_base(&self.api_base);
            let client = Client::with_config(openai_config);

            let resp = client.chat().create(self.request.clone()).await?;
            if let Some(content) = resp.choices[0].message.content.as_ref() {
                send_event(&sender, Event::ModelResponse(content.to_string()))?;
            }
            resp
        };

        trace!("Got response: {:?}", resp);

        // Store response for future reference
        if let Some(choice) = resp.choices.first() {
            self.response = Some(choice.message.clone());
        }

        let mut modresp = self.extract_changes()?;

        if let Some(usage) = resp.usage {
            modresp.usage = Some(super::Usage::OpenAi(OpenAiUsage {
                prompt_tokens: Some(usage.prompt_tokens),
                completion_tokens: Some(usage.completion_tokens),
                total_tokens: Some(usage.total_tokens),
            }));
        }

        Ok(modresp)
    }

    fn render(&self) -> Result<String> {
        Ok(format!("{:?}", self.request))
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

    fn chat(&self) -> Option<Box<dyn Chat>> {
        match OpenAiChat::new(
            self.api_model.clone(),
            self.openai_key.clone(),
            self.api_base.clone(),
            self.streaming,
            self.no_system_prompt,
            self.reasoning_effort.clone(),
        ) {
            Ok(chat) => Some(Box::new(chat)),
            Err(_) => None,
        }
    }
}
