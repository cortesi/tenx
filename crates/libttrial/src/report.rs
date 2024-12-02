use libtenx::{Session, StepType, TenxError};

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
    /// Number of patch errors
    pub error_patch: usize,
    /// Number of check errors
    pub error_check: usize,
    /// Number of response parse errors
    pub error_response_parse: usize,
    /// Number of other errors
    pub error_other: usize,
    /// Total model response time in seconds
    pub total_response_time: f64,
    /// Total number of words received from the model
    pub words_received: usize,
}

impl TrialReport {
    /// Computes a trial report from a session
    pub fn from_session(session: &Session, trial_name: &str) -> libtenx::Result<Self> {
        let steps_ref = session.steps();
        let model_name = steps_ref
            .first()
            .map(|s| s.model.clone())
            .ok_or_else(|| libtenx::TenxError::Internal("no steps in session".into()))?;
        let steps_ref = session.steps();
        let num_steps = steps_ref.len();
        let failed = session.last_step_error().is_some();

        let mut error_patch = 0;
        let mut error_check = 0;
        let mut error_response_parse = 0;
        let mut error_other = 0;
        let mut words_received = 0;

        let is_first_fix = !steps_ref.is_empty() && matches!(steps_ref[0].step_type, StepType::Fix);

        for (i, step) in steps_ref.iter().enumerate() {
            if let Some(err) = &step.err {
                // Skip counting check error for first step if it's a Fix
                if is_first_fix && i == 0 && matches!(err, TenxError::Check { .. }) {
                    continue;
                }
                match err {
                    TenxError::Patch { .. } => error_patch += 1,
                    TenxError::Check { .. } => error_check += 1,
                    TenxError::ResponseParse { .. } => error_response_parse += 1,
                    _ => error_other += 1,
                }
            }
            if let Some(response) = &step.model_response {
                if let Some(text) = &response.response_text {
                    words_received += text.split_whitespace().count();
                }
            }
        }

        Ok(TrialReport {
            trial_name: trial_name.to_string(),
            model_name,
            failed,
            steps: num_steps,
            error_patch,
            error_check,
            error_response_parse,
            error_other,
            total_response_time: steps_ref.iter().filter_map(|s| s.response_time).sum(),
            words_received,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libtenx::{
        model::{OpenAiUsage, Usage},
        StepType,
    };

    #[test]
    fn test_from_session_code() {
        let mut session = Session::default();

        // Add a successful step with token usage
        session
            .add_prompt("test_model".into(), "test 1".to_string(), StepType::Code)
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.response_time = Some(1.5);
            step.model_response = Some(libtenx::ModelResponse {
                comment: None,
                patch: None,
                operations: vec![],
                usage: Some(Usage::OpenAi(OpenAiUsage {
                    prompt_tokens: Some(10),
                    completion_tokens: Some(20),
                    total_tokens: Some(30),
                })),
                response_text: Some("test response".to_string()),
            });
        }

        // Add a step with a patch error
        session
            .add_prompt("test_model".into(), "test 2".to_string(), StepType::Code)
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.err = Some(TenxError::Patch {
                user: "user".to_string(),
                model: "model".to_string(),
            });
        }

        // Add a step with a check error
        session
            .add_prompt("test_model".into(), "test 3".to_string(), StepType::Code)
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.err = Some(TenxError::Check {
                name: "check".to_string(),
                user: "user".to_string(),
                model: "model".to_string(),
            });
        }

        let report = TrialReport::from_session(&session, "trial1").unwrap();

        assert_eq!(report.trial_name, "trial1");
        assert_eq!(report.model_name, "test_model");
        assert_eq!(report.steps, 3);
        // These values are defaults, as we only check for the existence of stats.
        assert_eq!(report.error_patch, 1);
        assert_eq!(report.error_check, 1);
        assert_eq!(report.error_response_parse, 0);
        assert_eq!(report.error_other, 0);
        assert_eq!(report.total_response_time, 1.5);
        assert!(report.failed);
    }

    #[test]
    fn test_from_session_fix() {
        let mut session = Session::default();

        // Add a Fix step with a check error (should be ignored)
        session
            .add_prompt("test_model".into(), "test 1".to_string(), StepType::Fix)
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.err = Some(TenxError::Check {
                name: "check".to_string(),
                user: "user".to_string(),
                model: "model".to_string(),
            });
        }

        // Add a step with a check error (should be counted)
        session
            .add_prompt("test_model".into(), "test 2".to_string(), StepType::Code)
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.err = Some(TenxError::Check {
                name: "check".to_string(),
                user: "user".to_string(),
                model: "model".to_string(),
            });
        }

        let report = TrialReport::from_session(&session, "trial1").unwrap();

        assert_eq!(
            report.error_check, 1,
            "Only the second check error should be counted"
        );
        assert_eq!(report.steps, 2);
        assert!(report.failed);
    }
}
