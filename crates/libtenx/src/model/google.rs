//! This module implements the Google model provider for the tenx system.
use std::collections::HashMap;

use google_genai::datatypes::{Content, GenerateContentReq, GenerateContentResponse, Part};
use serde::{Deserialize, Serialize};
use tracing::{trace, warn};

use super::Chat;

use crate::{
    config::Config,
    dialect::{Dialect, DialectProvider},
    error::{Result, TenxError},
    events::*,
    model::conversation::{build_conversation, Conversation},
    model::ModelProvider,
    session::ModelResponse,
    session::Session,
    throttle::Throttle,
};

fn map_error(e: google_genai::error::GenAiError) -> TenxError {
    match e {
        google_genai::error::GenAiError::Remote {
            status,
            message,
            headers,
        } => {
            warn!("Google API error: {} ({})\n{:?}", message, status, headers);
            if status == 429 {
                // Look for retry-after header
                if let Some(retry_after) = headers.get("retry-after") {
                    if let Ok(secs) = retry_after.parse::<u64>() {
                        return TenxError::Throttle(Throttle::RetryAfter(secs));
                    }
                }
                TenxError::Throttle(Throttle::Backoff)
            } else {
                TenxError::Model(message)
            }
        }
        google_genai::error::GenAiError::Internal(msg) => TenxError::Model(msg),
    }
}

/// A model that interacts with the Google Generative Language API. The general design of the model
/// is to:
///
/// - Have a large, cached system prompt with many examples.
/// - Emit both the non-editable context and the editable context as pre-primed messages in the
///   prompt.
/// - Edit the conversation to keep the most up-to-date editable files frontmost.
#[derive(Default, Debug, Clone)]
pub struct Google {
    pub name: String,
    pub api_model: String,
    pub api_key: String,
    pub streaming: bool,
}

/// Usage statistics for the Google PaLM API.
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct GoogleUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

impl GoogleUsage {
    pub fn values(&self) -> HashMap<String, u64> {
        let mut map = HashMap::new();
        if let Some(input_tokens) = self.input_tokens {
            map.insert("input_tokens".to_string(), input_tokens as u64);
        }
        if let Some(output_tokens) = self.output_tokens {
            map.insert("output_tokens".to_string(), output_tokens as u64);
        }
        if let Some(total_tokens) = self.total_tokens {
            map.insert("total_tokens".to_string(), total_tokens as u64);
        }
        map
    }

    pub fn totals(&self) -> (u64, u64) {
        let input = self.input_tokens.unwrap_or(0) as u64;
        let output = self.output_tokens.unwrap_or(0) as u64;
        (input, output)
    }
}

/// A model that interacts with the Google Generative Language API specifically for chat interactions.
#[derive(Debug, Clone)]
pub struct GoogleChat {
    /// Upstream model name to use
    pub api_model: String,
    /// The Google API key
    pub api_key: String,
    /// Whether to stream responses
    pub streaming: bool,
    /// The contents request being built
    request: GenerateContentReq,
}

impl GoogleChat {
    fn emit_event(
        &self,
        sender: &Option<EventSender>,
        response: &GenerateContentResponse,
    ) -> Result<()> {
        if let Some(candidates) = &response.candidates {
            if let Some(candidate) = candidates.first() {
                if let Some(content) = &candidate.content {
                    if let Some(parts) = &content.parts {
                        for part in parts {
                            if let Some(text) = &part.text {
                                send_event(sender, Event::Snippet(text.clone()))?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn stream_response(
        &self,
        api_key: String,
        req: &GenerateContentReq,
        sender: Option<EventSender>,
    ) -> Result<Vec<GenerateContentResponse>> {
        use futures_util::StreamExt;
        let mut stream = google_genai::generate_content_stream(&api_key, req.clone())
            .await
            .map_err(map_error)?;

        let mut responses = Vec::new();
        while let Some(response) = stream.next().await {
            let response = response.map_err(map_error)?;
            self.emit_event(&sender, &response)?;
            responses.push(response);
        }

        if responses.is_empty() {
            return Err(TenxError::Model("No response received from stream".into()));
        }
        Ok(responses)
    }

    fn extract_changes(
        &self,
        dialect: &Dialect,
        responses: &[GenerateContentResponse],
    ) -> Result<ModelResponse> {
        let mut full_text = String::new();
        let mut total_prompt_tokens = 0;
        let mut total_candidate_tokens = 0;
        let mut total_tokens = 0;

        for response in responses {
            if let Some(candidates) = &response.candidates {
                if let Some(candidate) = candidates.first() {
                    if let Some(content) = &candidate.content {
                        if let Some(parts) = &content.parts {
                            for part in parts {
                                if let Some(text) = &part.text {
                                    full_text.push_str(text);
                                }
                            }
                        }
                    }
                }
            }
            if let Some(metadata) = &response.usage_metadata {
                total_prompt_tokens += metadata.prompt_token_count.unwrap_or(0);
                total_candidate_tokens += metadata.candidates_token_count.unwrap_or(0);
                total_tokens += metadata.total_token_count.unwrap_or(0);
            }
        }

        if full_text.is_empty() {
            return Err(TenxError::Throttle(Throttle::Backoff));
        }

        let mut modresp = dialect.parse(&full_text)?;
        modresp.usage = Some(super::Usage::Google(GoogleUsage {
            input_tokens: Some(total_prompt_tokens as u32),
            output_tokens: Some(total_candidate_tokens as u32),
            total_tokens: Some(total_tokens as u32),
        }));

        Ok(modresp)
    }
}

#[async_trait::async_trait]
impl Chat for GoogleChat {
    fn add_system_prompt(&mut self, prompt: &str) -> Result<()> {
        self.request = self
            .request
            .clone()
            .system_instruction(Content::default().parts(vec![Part::default().text(prompt)]));
        Ok(())
    }

    fn add_user_message(&mut self, text: &str) -> Result<()> {
        let content = Content::default()
            .parts(vec![Part::default().text(text)])
            .role("user");
        let mut contents = self.request.contents.clone();
        contents.push(content);
        self.request = self.request.clone().contents(contents);
        Ok(())
    }

    fn add_agent_message(&mut self, text: &str) -> Result<()> {
        let content = Content::default()
            .parts(vec![Part::default().text(text)])
            .role("model");

        let mut contents = self.request.contents.clone();
        contents.push(content);
        self.request = self.request.clone().contents(contents);
        Ok(())
    }

    fn add_context(&mut self, name: &str, data: &str) -> Result<()> {
        // Add context as a user message with a clear marker
        self.add_user_message(&format!("<context name=\"{}\">{}\\</context>", name, data))
    }

    fn add_editable(&mut self, path: &str, data: &str) -> Result<()> {
        // Add editable content as a user message with a clear marker
        self.add_user_message(&format!(
            "<editable path=\"{}\">{}\\</editable>",
            path, data
        ))
    }

    async fn send(&mut self, sender: Option<EventSender>) -> Result<ModelResponse> {
        if self.api_key.is_empty() {
            return Err(TenxError::Model(
                "No API key configured for Google model.".into(),
            ));
        }

        self.request = self.request.clone().model(&self.api_model);

        trace!("Sending request: {:#?}", self.request);

        let responses = if self.streaming {
            self.stream_response(self.api_key.clone(), &self.request, sender.clone())
                .await?
        } else {
            let resp = google_genai::generate_content(&self.api_key, self.request.clone())
                .await
                .map_err(map_error)?;

            self.emit_event(&sender, &resp)?;
            vec![resp]
        };

        trace!("Got responses: {:#?}", responses);

        // Get dialect from config
        let config = Config::default();
        let dialect = config.dialect()?;

        let modresp = self.extract_changes(&dialect, &responses)?;
        Ok(modresp)
    }

    fn render(&self) -> Result<String> {
        Ok(format!("{:#?}", self.request))
    }
}

impl Google {
    async fn stream_response(
        &mut self,
        params: GenerateContentReq,
        sender: Option<EventSender>,
    ) -> Result<Vec<GenerateContentResponse>> {
        use futures_util::StreamExt;
        let mut stream = google_genai::generate_content_stream(&self.api_key, params)
            .await
            .map_err(map_error)?;

        let mut responses = Vec::new();
        while let Some(response) = stream.next().await {
            let response = response.map_err(map_error)?;
            self.emit_event(&sender, &response)?;
            responses.push(response);
        }

        if responses.is_empty() {
            return Err(TenxError::Model("No response received from stream".into()));
        }
        Ok(responses)
    }

    fn emit_event(
        &self,
        sender: &Option<EventSender>,
        response: &GenerateContentResponse,
    ) -> Result<()> {
        if let Some(candidates) = &response.candidates {
            if let Some(candidate) = candidates.first() {
                if let Some(content) = &candidate.content {
                    if let Some(parts) = &content.parts {
                        for part in parts {
                            if let Some(text) = &part.text {
                                send_event(sender, Event::ModelResponse(text.clone()))?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn extract_changes(
        &self,
        dialect: &Dialect,
        responses: &[&GenerateContentResponse],
    ) -> Result<ModelResponse> {
        let mut full_text = String::new();
        let mut total_prompt_tokens = 0;
        let mut total_candidate_tokens = 0;
        let mut total_tokens = 0;

        for response in responses {
            if let Some(candidates) = &response.candidates {
                if let Some(candidate) = candidates.first() {
                    if let Some(content) = &candidate.content {
                        if let Some(parts) = &content.parts {
                            for part in parts {
                                if let Some(text) = &part.text {
                                    full_text.push_str(text);
                                }
                            }
                        }
                    }
                }
            }
            if let Some(metadata) = &response.usage_metadata {
                total_prompt_tokens += metadata.prompt_token_count.unwrap_or(0);
                total_candidate_tokens += metadata.candidates_token_count.unwrap_or(0);
                total_tokens += metadata.total_token_count.unwrap_or(0);
            }
        }

        if full_text.is_empty() {
            return Err(TenxError::Internal("No patch to parse.".into()));
        }

        let mut modresp = dialect.parse(&full_text)?;
        modresp.usage = Some(super::Usage::Google(GoogleUsage {
            input_tokens: Some(total_prompt_tokens as u32),
            output_tokens: Some(total_candidate_tokens as u32),
            total_tokens: Some(total_tokens as u32),
        }));

        Ok(modresp)
    }

    fn request(
        &self,
        config: &Config,
        session: &Session,
        dialect: &Dialect,
    ) -> Result<GenerateContentReq> {
        let mut messages = Vec::new();
        build_conversation(self, &mut messages, config, session, dialect)?;
        Ok(GenerateContentReq::default()
            .model(&self.api_model)
            .contents(messages)
            .system_instruction(
                Content::default().parts(vec![Part::default().text(dialect.system())]),
            ))
    }
}

impl Conversation<Vec<Content>> for Google {
    fn set_system_prompt(&self, _messages: &mut Vec<Content>, _prompt: &str) -> Result<()> {
        Ok(())
    }

    fn add_user_message(&self, messages: &mut Vec<Content>, text: &str) -> Result<()> {
        messages.push(
            Content::default()
                .parts(vec![Part::default().text(text)])
                .role("user"),
        );
        Ok(())
    }

    fn add_agent_message(&self, messages: &mut Vec<Content>, text: &str) -> Result<()> {
        messages.push(
            Content::default()
                .parts(vec![Part::default().text(text.to_string())])
                .role("model"),
        );
        Ok(())
    }
}

#[async_trait::async_trait]
impl ModelProvider for Google {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn api_model(&self) -> String {
        self.api_model.clone()
    }

    fn chat(&self) -> Option<Box<dyn Chat>> {
        Some(Box::new(GoogleChat {
            api_model: self.api_model.clone(),
            api_key: self.api_key.clone(),
            streaming: self.streaming,
            request: GenerateContentReq::default(),
        }))
    }

    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<EventSender>,
    ) -> Result<ModelResponse> {
        if self.api_key.is_empty() {
            return Err(TenxError::Model(
                "No API key configured for Google model.".into(),
            ));
        }

        if !session.should_continue() {
            return Err(TenxError::Internal("No prompt to process.".into()));
        }

        let dialect = config.dialect()?;
        let req = self.request(config, session, &dialect)?;
        trace!("Sending request: {:#?}", req);

        let responses = if self.streaming {
            self.stream_response(req.clone(), sender).await?
        } else {
            let resp = google_genai::generate_content(&self.api_key, req.clone())
                .await
                .map_err(map_error)?;

            self.emit_event(&sender, &resp)?;
            vec![resp]
        };

        trace!("Got responses: {:#?}", responses);
        let modresp = self.extract_changes(&dialect, &responses.iter().collect::<Vec<_>>())?;

        Ok(modresp)
    }

    fn render(&self, config: &Config, session: &Session) -> Result<String> {
        let dialect = config.dialect()?;
        let req = self.request(config, session, &dialect)?;
        Ok(format!("{:?}", req))
    }
}
