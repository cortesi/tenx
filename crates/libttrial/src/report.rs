use libtenx::{Result, Session, TenxError};

/// A report about a trial execution.
#[derive(Debug)]
pub struct TrialReport {
    /// The name of the trial being executed
    pub trial_name: String,
    /// The name of the model used for execution
    pub model_name: String,
    /// The iteration number (when a trial is run multiple times)
    pub n: usize,
    /// True if any errors occurred during execution
    pub failed: bool,
    /// Total number of steps taken
    pub steps: usize,
    /// Number of patch application errors
    pub error_patch: usize,
    /// Number of check failures
    pub error_check: usize,
    /// Number of response parsing errors
    pub error_response_parse: usize,
    /// Number of other errors
    pub error_other: usize,
    /// Total time spent waiting for model responses
    pub total_response_time: f64,
    /// Total number of words received from the model
    pub words_received: usize,
}

impl TrialReport {
    /// Computes a trial report from a session
    pub fn from_session(session: &Session, trial_name: &str, n: usize) -> Result<Self> {
        let model_name = session
            .steps()
            .first()
            .ok_or_else(|| {
                TenxError::Internal("Cannot create trial report from empty session".to_string())
            })?
            .model
            .clone();

        let mut error_patch = 0;
        let mut error_check = 0;
        let mut error_response_parse = 0;
        let mut error_other = 0;

        let mut failed = false;
        for step in session.steps() {
            if let Some(err) = &step.err {
                failed = true;
                match err {
                    TenxError::Patch { .. } => error_patch += 1,
                    TenxError::Check { .. } => error_check += 1,
                    TenxError::ResponseParse { .. } => error_response_parse += 1,
                    _ => error_other += 1,
                }
            }
        }

        let total_response_time = session.steps().iter().filter_map(|s| s.response_time).sum();

        let words_received = session
            .steps()
            .iter()
            .filter_map(|s| s.model_response.as_ref())
            .filter_map(|r| r.response_text.as_ref())
            .map(|s| s.split_whitespace().count())
            .sum();

        Ok(TrialReport {
            trial_name: trial_name.to_string(),
            model_name,
            n,
            failed,
            steps: session.steps().len(),
            error_patch,
            error_check,
            error_response_parse,
            error_other,
            total_response_time,
            words_received,
        })
    }
}
