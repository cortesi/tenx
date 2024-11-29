use libtenx::{Session, TenxError};

/// A report about a trial execution.
#[derive(Debug)]
pub struct TrialReport {
    /// Name of the trial
    pub trial_name: String,
    /// Name of the model used
    pub model_name: String,
    /// Whether the trial failed
    pub failed: bool,
    /// Number of steps taken
    pub steps: usize,
    /// Total words sent
    pub words_sent: usize,
    /// Total words received
    pub words_received: usize,
    /// Number of patch errors
    pub error_patch: usize,
    /// Number of check errors
    pub error_check: usize,
    /// Number of response parse errors
    pub error_response_parse: usize,
    /// Number of other errors
    pub error_other: usize,
    /// Total execution time in seconds
    pub time_taken: f64,
}

impl TrialReport {
    /// Computes a trial report from a session
    pub fn from_session(
        session: &Session,
        trial_name: String,
        model_name: String,
        time_taken: f64,
    ) -> Self {
        let steps = session.steps().len();
        let stats = session
            .stats(&libtenx::config::Config::default())
            .unwrap_or_default();
        let words_sent = stats.words_sent;
        let words_received = stats.words_received;
        let failed = session.last_step_error().is_some();

        let mut error_patch = 0;
        let mut error_check = 0;
        let mut error_response_parse = 0;
        let mut error_other = 0;

        for step in session.steps() {
            if let Some(err) = &step.err {
                match err {
                    TenxError::Patch { .. } => error_patch += 1,
                    TenxError::Check { .. } => error_check += 1,
                    TenxError::ResponseParse { .. } => error_response_parse += 1,
                    _ => error_other += 1,
                }
            }
        }

        TrialReport {
            trial_name,
            model_name,
            failed,
            steps,
            words_sent,
            words_received,
            error_patch,
            error_check,
            error_response_parse,
            error_other,
            time_taken,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libtenx::{
        model::{OpenAiUsage, Usage},
        prompt::Prompt,
    };

    #[test]
    fn test_from_session() {
        let mut session = Session::default();

        // Add a successful step with token usage
        session
            .add_prompt("test_model".into(), Prompt::User("test 1".to_string()))
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.model_response = Some(libtenx::ModelResponse {
                comment: None,
                patch: None,
                operations: vec![],
                usage: Some(Usage::OpenAi(OpenAiUsage {
                    prompt_tokens: Some(10),
                    completion_tokens: Some(20),
                    total_tokens: Some(30),
                })),
                text: Some("test response".to_string()),
            });
        }

        // Add a step with a patch error
        session
            .add_prompt("test_model".into(), Prompt::User("test 2".to_string()))
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.err = Some(TenxError::Patch {
                user: "user".to_string(),
                model: "model".to_string(),
            });
        }

        // Add a step with a check error
        session
            .add_prompt("test_model".into(), Prompt::User("test 3".to_string()))
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.err = Some(TenxError::Check {
                name: "check".to_string(),
                user: "user".to_string(),
                model: "model".to_string(),
            });
        }

        let report =
            TrialReport::from_session(&session, "trial1".to_string(), "gpt4".to_string(), 1.5);

        assert_eq!(report.trial_name, "trial1");
        assert_eq!(report.model_name, "gpt4");
        assert_eq!(report.steps, 3);
        // These values are defaults, as we only check for the existence of stats.
        assert_eq!(report.words_sent, 0);
        assert_eq!(report.words_received, 0);
        assert_eq!(report.error_patch, 1);
        assert_eq!(report.error_check, 1);
        assert_eq!(report.error_response_parse, 0);
        assert_eq!(report.error_other, 0);
        assert_eq!(report.time_taken, 1.5);
        assert!(report.failed);
    }
}
