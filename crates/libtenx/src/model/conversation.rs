use crate::{
    config::Config,
    dialect::{Dialect, DialectProvider},
    Result, Session,
};

pub const EDITABLE_LEADIN: &str = "Here are the editable files.";
pub const CONTEXT_LEADIN: &str = "Here is some immutable context that you may not edit.";
pub const ACK: &str = "Got it.";

/// Conversation lets us extact a generic strategy for dealing with conversational
/// models, where there is a User/Assistant rlquest/response cycle.
pub trait Conversation<R> {
    fn set_system_prompt(&self, req: &mut R, prompt: String) -> Result<()>;
    fn add_user_message(&self, req: &mut R, text: String) -> Result<()>;
    fn add_agent_message(&self, req: &mut R, text: &str) -> Result<()>;
    fn add_editables(
        &self,
        req: &mut R,
        config: &Config,
        session: &Session,
        dialect: &Dialect,
        step_offset: usize,
    ) -> Result<()>;
}

/// Builds a conversation following our standard pattern
pub fn build_conversation<C, R>(
    conversation: &C,
    req: &mut R,
    config: &Config,
    session: &Session,
    dialect: &Dialect,
) -> Result<()>
where
    C: Conversation<R>,
{
    conversation.set_system_prompt(req, dialect.system())?;
    conversation.add_user_message(
        req,
        format!(
            "{}\n{}",
            CONTEXT_LEADIN,
            dialect.render_context(config, session)?
        ),
    )?;
    conversation.add_agent_message(req, ACK)?;
    for (i, step) in session.steps().iter().enumerate() {
        conversation.add_editables(req, config, session, dialect, i)?;
        conversation.add_user_message(req, dialect.render_step_request(config, session, i)?)?;
        if step.model_response.is_some() {
            conversation
                .add_agent_message(req, &dialect.render_step_response(config, session, i)?)?;
        } else if i != session.steps().len() - 1 {
            // We have no model response, but we're not the last step, so this isn't a user request
            // step just about to be sent to the model. This is presumably an error - the best we
            // can do to preserve sequencing is either omit the step entirely or add an omission
            // message from the agent. Since omitting the step will lose the user's prompt, we opt
            // for the latter.
            conversation.add_agent_message(req, "omitted due to error")?;
        }
    }
    conversation.add_editables(req, config, session, dialect, session.steps().len())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::DummyDialect;

    #[derive(Debug, PartialEq, Clone)]
    enum Message {
        User(String),
        Agent(String),
    }

    #[derive(Default)]
    struct DummyRequest {
        system_prompt: Option<String>,
        messages: Vec<Message>,
        editable_calls: Vec<usize>,
    }

    struct DummyConversation;

    impl Conversation<DummyRequest> for DummyConversation {
        fn set_system_prompt(&self, req: &mut DummyRequest, prompt: String) -> Result<()> {
            match req.system_prompt {
                None => {
                    req.system_prompt = Some(prompt);
                    Ok(())
                }
                Some(_) => panic!("system prompt already set"),
            }
        }

        fn add_user_message(&self, req: &mut DummyRequest, text: String) -> Result<()> {
            req.messages.push(Message::User(text));
            Ok(())
        }

        fn add_agent_message(&self, req: &mut DummyRequest, text: &str) -> Result<()> {
            req.messages.push(Message::Agent(text.to_string()));
            Ok(())
        }

        fn add_editables(
            &self,
            req: &mut DummyRequest,
            _config: &Config,
            _session: &Session,
            _dialect: &Dialect,
            step_offset: usize,
        ) -> Result<()> {
            req.editable_calls.push(step_offset);
            Ok(())
        }
    }

    /// Verifies that messages follow the correct conversation flow:
    /// - Starts with a user message
    /// - Strictly alternates between user and agent messages
    fn assert_flow(messages: &[Message]) {
        assert!(!messages.is_empty(), "conversation must have messages");

        for pair in messages.chunks(2) {
            match pair {
                [Message::User(_), Message::Agent(_)] => (),
                [Message::User(_)] if pair.len() == 1 => (),
                _ => panic!("conversation must consist of (user, agent) pairs, possibly ending with a user message"),
            }
        }
    }

    #[test]
    fn test_basic_conversation_flow() -> Result<()> {
        let conversation = DummyConversation {};
        let mut req = DummyRequest::default();
        let dialect = Dialect::Dummy(DummyDialect::default());
        let config = Config::default();
        let mut session = Session::default();
        session.add_prompt(
            "test_model".into(),
            crate::prompt::Prompt::User("test prompt".to_string()),
        )?;

        build_conversation(&conversation, &mut req, &config, &session, &dialect)?;

        assert!(req.system_prompt.is_some());
        assert_flow(&req.messages);

        Ok(())
    }

    #[test]
    fn test_empty_session() -> Result<()> {
        let conversation = DummyConversation {};
        let mut req = DummyRequest::default();
        let dialect = Dialect::Dummy(DummyDialect::default());
        let config = Config::default();
        let session = Session::default();

        build_conversation(&conversation, &mut req, &config, &session, &dialect)?;

        assert!(req.system_prompt.is_some());
        assert_eq!(req.editable_calls, vec![0]);
        assert_eq!(req.messages.len(), 2); // Context message and ACK only
        assert_flow(&req.messages);

        Ok(())
    }
}
