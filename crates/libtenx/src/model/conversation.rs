use crate::{
    config::Config,
    dialect::{Dialect, DialectProvider},
    Result, Session,
};

const CONTEXT_LEADIN: &str = "Here is some immutable context that you may not edit.\n";
const ACK: &str = "Got it.";

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
            conversation.add_agent_message(req, "omitted due to error")?;
        }
    }
    conversation.add_editables(req, config, session, dialect, session.steps().len())?;
    Ok(())
}
