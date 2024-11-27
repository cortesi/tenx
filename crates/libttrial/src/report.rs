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
    /// Total tokens used in input
    pub tokens_in: u64,
    /// Total tokens used in output
    pub tokens_out: u64,
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
        let (tokens_in, tokens_out) = session
            .steps()
            .iter()
            .filter_map(|step| step.model_response.as_ref()?.usage.as_ref())
            .map(|usage| usage.totals())
            .fold((0, 0), |(acc_in, acc_out), (in_, out)| {
                (acc_in + in_, acc_out + out)
            });
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
            tokens_in,
            tokens_out,
            error_patch,
            error_check,
            error_response_parse,
            error_other,
            time_taken,
        }
    }
}
