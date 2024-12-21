//! This module implements the Google model provider for the tenx system.
use std::collections::HashMap;

use googleapis_tonic_google_ai_generativelanguage_v1::google::ai::generativelanguage::v1::{
    self as gl, Part,
};
use http_body::Body;
use tonic::{transport::Channel, Status};

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

const MAX_TOKENS: i32 = 8192;

impl From<tonic::transport::Error> for TenxError {
    fn from(error: tonic::transport::Error) -> Self {
        TenxError::Model(error.to_string())
    }
}

impl From<Status> for TenxError {
    fn from(error: Status) -> Self {
        TenxError::Model(error.to_string())
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

impl Google {
    async fn stream_response<T>(
        &mut self,
        client: &mut gl::generative_service_client::GenerativeServiceClient<T>,
        req: gl::GenerateContentRequest,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<gl::GenerateContentResponse>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        T::ResponseBody: Body<Data = tonic::codegen::Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
    {
        let mut stream = client.stream_generate_content(req).await?.into_inner();

        let mut final_response = None;
        while let Some(response) = stream.message().await? {
            if let Some(candidate) = response.candidates.first() {
                if let Some(content) = &candidate.content {
                    for part in &content.parts {
                        if let Some(gl::part::Data::Text(text)) = &part.data {
                            send_event(&sender, Event::Snippet(text.clone()))?;
                        }
                    }
                }
            }
            final_response = Some(response);
        }

        final_response.ok_or_else(|| TenxError::Model("No response received from stream".into()))
    }

    fn extract_changes(
        &self,
        dialect: &Dialect,
        response: &gl::GenerateContentResponse,
    ) -> Result<ModelResponse> {
        if let Some(candidate) = response.candidates.first() {
            if let Some(content) = &candidate.content {
                for part in &content.parts {
                    if let Some(gl::part::Data::Text(text)) = &part.data {
                        return dialect.parse(text);
                    }
                }
            }
        }
        Err(TenxError::Internal("No patch to parse.".into()))
    }

    fn request(
        &self,
        config: &Config,
        session: &Session,
        dialect: &Dialect,
    ) -> Result<gl::GenerateContentRequest> {
        let mut messages = Vec::new();
        build_conversation(self, &mut messages, config, session, dialect)?;

        Ok(gl::GenerateContentRequest {
            model: self.api_model.clone(),
            contents: messages,
            safety_settings: Vec::new(),
            generation_config: Some(gl::GenerationConfig {
                max_output_tokens: Some(MAX_TOKENS),
                temperature: Some(0.7),
                top_p: Some(0.95),
                top_k: Some(40),
                candidate_count: Some(1),
                stop_sequences: Vec::new(),
                ..Default::default()
            }),
        })
    }
}

impl Conversation<Vec<gl::Content>> for Google {
    fn set_system_prompt(&self, messages: &mut Vec<gl::Content>, prompt: String) -> Result<()> {
        messages.push(gl::Content {
            parts: vec![Part {
                data: Some(gl::part::Data::Text(prompt)),
            }],
            role: "system".to_string(),
        });
        Ok(())
    }

    fn add_user_message(&self, messages: &mut Vec<gl::Content>, text: String) -> Result<()> {
        messages.push(gl::Content {
            parts: vec![Part {
                data: Some(gl::part::Data::Text(text)),
            }],
            role: "user".to_string(),
        });
        Ok(())
    }

    fn add_agent_message(&self, messages: &mut Vec<gl::Content>, text: &str) -> Result<()> {
        messages.push(gl::Content {
            parts: vec![Part {
                data: Some(gl::part::Data::Text(text.to_string())),
            }],
            role: "assistant".to_string(),
        });
        Ok(())
    }

    fn add_editables(
        &self,
        messages: &mut Vec<gl::Content>,
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
        trace!("Sending request: {:?}", req);

        let channel = Channel::from_static("https://generativelanguage.googleapis.com")
            .tls_config(tonic::transport::ClientTlsConfig::new())?
            .connect()
            .await?;
        let api_key = self.api_key.clone();
        let mut client = gl::generative_service_client::GenerativeServiceClient::with_interceptor(
            channel,
            move |mut req: tonic::Request<()>| {
                req.metadata_mut()
                    .insert("x-goog-api-key", api_key.parse().unwrap());
                Ok(req)
            },
        );

        let resp = if self.streaming {
            self.stream_response(&mut client, req.clone(), sender)
                .await?
        } else {
            let resp = client.generate_content(req.clone()).await?.into_inner();
            if let Some(candidate) = resp.candidates.first() {
                if let Some(content) = &candidate.content {
                    if let Some(part) = content.parts.first() {
                        if let Some(gl::part::Data::Text(text)) = &part.data {
                            send_event(&sender, Event::ModelResponse(text.clone()))?;
                        }
                    }
                }
            }
            resp
        };

        trace!("Got response: {:?}", resp);
        let mut modresp = self.extract_changes(&dialect, &resp)?;

        if let Some(metadata) = resp.usage_metadata {
            modresp.usage = Some(super::Usage::Google(GoogleUsage {
                input_tokens: Some(metadata.prompt_token_count as u32),
                output_tokens: Some(metadata.candidates_token_count as u32),
                total_tokens: Some(metadata.total_token_count as u32),
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
