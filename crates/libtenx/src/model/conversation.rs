use crate::{
    config::Config,
    dialect::{Dialect, DialectProvider},
    error::Result,
    session::Session,
};

pub const EDITABLE_LEADIN: &str = "Here are the editable files.";
pub const CONTEXT_LEADIN: &str = "Here is some immutable context that you may not edit.";
pub const ACK: &str = "Got it.";

/// Conversation lets us extact a generic strategy for dealing with conversational
/// models, where there is a User/Assistant request/response cycle.
pub trait Conversation<R> {
    fn set_system_prompt(&self, req: &mut R, prompt: &str) -> Result<()>;
    fn add_user_message(&self, req: &mut R, text: &str) -> Result<()>;
    fn add_agent_message(&self, req: &mut R, text: &str) -> Result<()>;
}

fn add_editables<C, R>(
    conversation: &C,
    req: &mut R,
    config: &Config,
    session: &Session,
    dialect: &Dialect,
    action_idx: usize,
    step_idx: usize,
) -> Result<()>
where
    C: Conversation<R>,
{
    // For empty sessions or invalid offsets, skip adding editables
    if session.actions.is_empty() {
        return Ok(());
    }

    // Handle cases where we need to safely get editables
    let editables = if action_idx < session.actions.len() {
        let action = &session.actions[action_idx];
        // If this is a valid step in the action or just at the end
        if step_idx < action.steps.len() {
            session.editables_for_step_state(action_idx, step_idx)?
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    if !editables.is_empty() {
        conversation.add_user_message(
            req,
            &format!(
                "{}\n{}",
                EDITABLE_LEADIN,
                dialect.render_editables(config, session, editables)?
            ),
        )?;
        conversation.add_agent_message(req, ACK)?;
    }

    Ok(())
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
    conversation.set_system_prompt(req, &dialect.system())?;
    conversation.add_user_message(
        req,
        &format!(
            "{}\n{}",
            CONTEXT_LEADIN,
            dialect.render_context(config, session)?
        ),
    )?;
    conversation.add_agent_message(req, ACK)?;
    if !session.actions.is_empty() {
        let last_action = session.actions.len() - 1;
        for (i, step) in session.actions[last_action].steps.iter().enumerate() {
            add_editables(conversation, req, config, session, dialect, last_action, i)?;
            conversation.add_user_message(
                req,
                &dialect.render_step_request(config, session, last_action, i)?,
            )?;
            if step.model_response.is_some() {
                conversation.add_agent_message(
                    req,
                    &dialect.render_step_response(config, session, last_action, i)?,
                )?;
            } else if i != session.actions[last_action].steps.len() - 1 {
                // We have no model response, but we're not the last step, so this isn't a user request
                // step just about to be sent to the model. This is presumably an error - the best we
                // can do to preserve sequencing is either omit the step entirely or add an omission
                // message from the agent. Since omitting the step will lose the user's prompt, we opt
                // for the latter.
                conversation.add_agent_message(req, "omitted due to error")?;
            }
        }
        add_editables(
            conversation,
            req,
            config,
            session,
            dialect,
            last_action,
            session.actions[last_action].steps.len(),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dialect::DummyDialect,
        session::{Action, Step},
        strategy, testutils,
    };

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
        fn set_system_prompt(&self, req: &mut DummyRequest, prompt: &str) -> Result<()> {
            match req.system_prompt {
                None => {
                    req.system_prompt = Some(prompt.into());
                    Ok(())
                }
                Some(_) => panic!("system prompt already set"),
            }
        }

        fn add_user_message(&self, req: &mut DummyRequest, text: &str) -> Result<()> {
            req.messages.push(Message::User(text.into()));
            Ok(())
        }

        fn add_agent_message(&self, req: &mut DummyRequest, text: &str) -> Result<()> {
            req.messages.push(Message::Agent(text.to_string()));
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
        let mut p = testutils::test_project();

        let conversation = DummyConversation {};
        let mut req = DummyRequest::default();
        let dialect = Dialect::Dummy(DummyDialect::default());

        p.session.add_action(Action::new(
            &p.config,
            strategy::Strategy::Code(strategy::Code::new()),
        )?)?;
        p.session.last_action_mut()?.add_step(Step::new(
            "test_model".into(),
            "test prompt".to_string(),
            strategy::StrategyStep::Code(strategy::CodeStep::default()),
        ))?;

        build_conversation(&conversation, &mut req, &p.config, &p.session, &dialect)?;

        assert!(req.system_prompt.is_some());
        assert_flow(&req.messages);

        Ok(())
    }

    #[test]
    fn test_empty_session() -> Result<()> {
        let p = testutils::test_project();

        let conversation = DummyConversation {};
        let mut req = DummyRequest::default();
        let dialect = Dialect::Dummy(DummyDialect::default());

        build_conversation(&conversation, &mut req, &p.config, &p.session, &dialect)?;

        assert!(req.system_prompt.is_some());
        assert!(req.editable_calls.is_empty());
        assert_eq!(req.messages.len(), 2); // Context message and ACK only
        assert_flow(&req.messages);

        Ok(())
    }
}
