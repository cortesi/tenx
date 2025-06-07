//! This module implements the Google model provider for the tenx system.
use std::collections::HashMap;

use google_genai::datatypes::{Content, GenerateContentReq, GenerateContentResponse, Part};
use serde::{Deserialize, Serialize};
use tracing::{trace, warn};

use super::Chat;

use crate::{
    checks::CheckResult,
    context::ContextItem,
    error::{Result, TenxError},
    events::*,
    model::tags,
    model::ModelProvider,
    session::ModelResponse,
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
    /// Creates a new GoogleChat configured for the given model, API key, and streaming setting.
    pub fn new(api_model: String, api_key: String, streaming: bool) -> Self {
        let request = GenerateContentReq::default()
            .model(&api_model)
            .system_instruction(Content::default().parts(vec![Part::default().text(tags::SYSTEM)]));
        Self {
            api_model,
            api_key,
            streaming,
            request,
        }
    }

    /// Helper to add or append a message with the given role.
    fn add_message_with_role(&mut self, role: &str, text: &str) -> Result<()> {
        let mut contents = self.request.contents.clone();

        // Check if we need to consolidate with the last message
        if !contents.is_empty() && contents.last().unwrap().role.as_deref() == Some(role) {
            // Append to the last message
            let last_content = contents.last_mut().unwrap();
            if let Some(parts) = &mut last_content.parts {
                if let Some(last_part) = parts.last_mut() {
                    if let Some(existing_text) = &last_part.text {
                        last_part.text = Some(format!("{}\n{}", existing_text, text.trim()));
                    } else {
                        last_part.text = Some(text.trim().to_string());
                    }
                } else {
                    parts.push(Part::default().text(text.trim()));
                }
            } else {
                last_content.parts = Some(vec![Part::default().text(text.trim())]);
            }
        } else {
            // Create a new message
            let content = Content::default()
                .parts(vec![Part::default().text(text.trim())])
                .role(role);
            contents.push(content);
        }

        self.request = self.request.clone().contents(contents);
        Ok(())
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

    fn extract_changes(&self, responses: &[GenerateContentResponse]) -> Result<ModelResponse> {
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

        let mut modresp = tags::parse(&full_text)?;
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
        // Append to the existing system instruction
        let current_system = self
            .request
            .system_instruction
            .as_ref()
            .and_then(|c| c.parts.as_ref())
            .and_then(|p| p.first())
            .and_then(|p| p.text.as_ref())
            .unwrap_or(&String::new())
            .to_string();

        let new_system = if current_system.is_empty() {
            prompt.to_string()
        } else {
            format!("{}\n{}", current_system, prompt)
        };

        self.request = self
            .request
            .clone()
            .system_instruction(Content::default().parts(vec![Part::default().text(new_system)]));
        Ok(())
    }

    fn add_user_message(&mut self, text: &str) -> Result<()> {
        self.add_message_with_role("user", text)
    }

    fn add_agent_message(&mut self, text: &str) -> Result<()> {
        self.add_message_with_role("model", text)
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

        let modresp = self.extract_changes(&responses)?;
        Ok(modresp)
    }

    fn render(&self) -> Result<String> {
        Ok(format!("{:#?}", self.request))
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
        Some(Box::new(GoogleChat::new(
            self.api_model.clone(),
            self.api_key.clone(),
            self.streaming,
        )))
    }
}
