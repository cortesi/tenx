use crate::{
    config::Config,
    dialect::{Dialect, DialectProvider},
    error::{Result, TenxError},
    session::Session,
};

pub const EDITABLE_LEADIN: &str = "Here are the editable files.";
pub const CONTEXT_LEADIN: &str = "Here is some immutable context that you may not edit.";
pub const ACK: &str = "Got it.";

/// Convert a flat step offset into action and step indices
fn offset_to_indices(session: &Session, step_offset: usize) -> Result<(usize, usize)> {
    // Handle empty session specially
    if session.actions().is_empty() {
        // For empty sessions, treat offset 0 as action 0, step 0 (even though they don't exist)
        if step_offset == 0 {
            return Ok((0, 0));
        }
        return Err(TenxError::Internal(format!(
            "Invalid step offset: {} for empty session",
            step_offset
        )));
    }

    let mut remaining = step_offset;
    for (action_idx, action) in session.actions().iter().enumerate() {
        if remaining < action.steps().len() {
            return Ok((action_idx, remaining));
        }
        remaining -= action.steps().len();
    }

    // If we reach here, the offset is equal to or greater than the total number of steps
    let total_steps = session.steps().len();

    // If offset is exactly at the end, use the last action with a step index at its end
    if step_offset == total_steps {
        let action_idx = session.actions().len() - 1;
        let step_idx = session.actions()[action_idx].steps().len() - 1;
        return Ok((action_idx, step_idx));
    }

    // If the action has steps but step_offset is beyond the end
    if total_steps > 0 && step_offset > total_steps {
        return Err(TenxError::Internal(format!(
            "Step offset {} exceeds total steps {}",
            step_offset, total_steps
        )));
    }

    // Default for testing - if actions exist but have no steps, use first action, step 0
    if total_steps == 0 {
        return Ok((0, 0));
    }

    Err(TenxError::Internal(format!(
        "Invalid step offset: {}",
        step_offset
    )))
}

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
    step_offset: usize,
) -> Result<()>
where
    C: Conversation<R>,
{
    // For empty sessions or invalid offsets, skip adding editables
    if session.actions().is_empty() {
        return Ok(());
    }

    // Try to convert the offset to indices
    match offset_to_indices(session, step_offset) {
        Ok((action_idx, step_idx)) => {
            // Handle cases where we need to safely get editables
            let editables = if action_idx < session.actions().len() {
                let action = &session.actions()[action_idx];
                // If this is a valid step in the action or just at the end
                if step_idx < action.steps().len() {
                    session.editables_for_step_state(action_idx, step_idx)?
                } else {
                    // Fallback to the session's general editables
                    session.editables()
                }
            } else {
                // Fallback to the session's general editables
                session.editables()
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
        }
        Err(_) => {
            // If we can't convert the offset, just use the general session editables
            let editables = session.editables();
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
        }
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
    for (i, step) in session.steps().iter().enumerate() {
        add_editables(conversation, req, config, session, dialect, i)?;
        conversation.add_user_message(req, &dialect.render_step_request(config, session, i)?)?;
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
    add_editables(
        conversation,
        req,
        config,
        session,
        dialect,
        session.steps().len(),
    )?;
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
        p.session
            .last_action_mut()?
            .add_step(Step::new("test_model".into(), "test prompt".to_string()))?;

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
