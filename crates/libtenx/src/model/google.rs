//! This module implements the Google model provider for the tenx system.
use std::collections::HashMap;

use google_genai::datatypes::{Content, GenerateContentReq, Part};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::trace;

use crate::{
    config::Config,
    dialect::{Dialect, DialectProvider},
    events::*,
    model::conversation::{build_conversation, Conversation, ACK, EDITABLE_LEADIN},
    model::ModelProvider,
    session::ModelResponse,
    session::Session,
    Result, TenxError,
};

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

impl Google {
    async fn stream_response(
        &mut self,
        params: GenerateContentReq,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<Vec<google_genai::datatypes::GenerateContentResponse>> {
        use futures_util::StreamExt;
        let mut stream = google_genai::generate_content_stream(&self.api_key, params)
            .await
            .map_err(|e| TenxError::Model(e.to_string()))?;

        let mut responses = Vec::new();
        while let Some(response) = stream.next().await {
            let response = response.map_err(|e| TenxError::Model(e.to_string()))?;
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
        sender: &Option<mpsc::Sender<Event>>,
        response: &google_genai::datatypes::GenerateContentResponse,
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
        responses: &[&google_genai::datatypes::GenerateContentResponse],
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
    fn set_system_prompt(&self, _messages: &mut Vec<Content>, _prompt: String) -> Result<()> {
        Ok(())
    }

    fn add_user_message(&self, messages: &mut Vec<Content>, text: String) -> Result<()> {
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

    fn add_editables(
        &self,
        messages: &mut Vec<Content>,
        config: &Config,
        session: &Session,
        dialect: &Dialect,
        step_offset: usize,
    ) -> Result<()> {
        let editables = session.editables_for_step(step_offset)?;
        if !editables.is_empty() {
            self.add_user_message(
                messages,
                format!(
                    "{}\n{}",
                    EDITABLE_LEADIN,
                    dialect.render_editables(config, session, editables)?
                ),
            )?;
            self.add_agent_message(messages, ACK)?;
        }
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

    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<mpsc::Sender<Event>>,
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
                .map_err(|e| TenxError::Model(e.to_string()))?;

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
