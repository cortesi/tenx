use libtenx::{session::Session, Result, TenxError};

/// A report about a trial execution.
#[derive(Debug)]
pub struct TrialReport {
    /// The name of the trial being executed
    pub trial_name: String,
    /// The name of the model used for execution
    pub model_name: String,
    /// The iteration number (when a trial is run multiple times)
    pub api_model: String,
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

/// Aggregate performance metrics for a model across multiple trial runs.
#[derive(Debug)]
pub struct ModelScore {
    /// Name of the model
    pub model_name: String,
    /// API model identifier
    pub api_model: String,
    /// Total number of trials run
    pub total_trials: usize,
    /// Number of failed trials
    pub total_fails: usize,
    /// Number of successful trials
    pub total_succeeds: usize,
    /// Total number of errors across all trials
    pub total_errors: usize,
    /// Number of patch application errors
    pub error_patch: usize,
    /// Number of check failures
    pub error_check: usize,
    /// Number of response parsing errors
    pub error_response_parse: usize,
    /// Number of other errors
    pub error_other: usize,
    /// Total number of words received from the model
    pub total_words: usize,
    /// Total time spent waiting for model responses
    pub total_time: f64,
}

/// Create aggregate model scores from a sequence of trial reports.
pub fn model_scores<'a>(reports: impl IntoIterator<Item = &'a TrialReport>) -> Vec<ModelScore> {
    let mut scores = std::collections::HashMap::new();

    for report in reports {
        scores
            .entry((report.model_name.clone(), report.api_model.clone()))
            .or_insert_with(|| ModelScore::new(report.model_name.clone(), report.api_model.clone()))
            .add_report(report);
    }

    scores.into_values().collect()
}

impl ModelScore {
    /// Add a trial report to the model score
    pub fn add_report(&mut self, report: &TrialReport) {
        self.total_trials += 1;
        if report.failed {
            self.total_fails += 1;
        } else {
            self.total_succeeds += 1;
        }
        self.total_errors += report.error_patch
            + report.error_check
            + report.error_response_parse
            + report.error_other;
        self.error_patch += report.error_patch;
        self.error_check += report.error_check;
        self.error_response_parse += report.error_response_parse;
        self.error_other += report.error_other;
        self.total_words += report.words_received;
        self.total_time += report.total_response_time;
    }

    /// Create a new ModelScore for a specific model
    pub fn new(model_name: String, api_model: String) -> Self {
        ModelScore {
            model_name,
            api_model,
            total_trials: 0,
            total_fails: 0,
            total_succeeds: 0,
            total_errors: 0,
            error_patch: 0,
            error_check: 0,
            error_response_parse: 0,
            error_other: 0,
            total_words: 0,
            total_time: 0.0,
        }
    }
}

impl TrialReport {
    /// Computes a trial report from a session
    pub fn from_session(
        session: &Session,
        trial_name: &str,
        n: usize,
        config: &libtenx::config::Config,
    ) -> Result<Self> {
        let model = session.steps().first().ok_or_else(|| {
            TenxError::Internal("Cannot create trial report from empty session".to_string())
        })?;

        let model_name = model.model.clone();

        let api_model = config
            .get_model_conf(&model_name)
            .ok_or_else(|| {
                TenxError::Internal(format!("Model config not found for {}", model_name))
            })?
            .api_model()
            .to_string();

        let mut error_patch = 0;
        let mut error_check = 0;
        let mut error_response_parse = 0;
        let mut error_other = 0;

        let failed = session.last_step().and_then(|s| s.err.as_ref()).is_some();
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
            api_model,
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_model_score_new() {
        let score = ModelScore::new("gpt-4".to_string(), "gpt-4-0613".to_string());
        assert_eq!(score.model_name, "gpt-4");
        assert_eq!(score.api_model, "gpt-4-0613");
        assert_eq!(score.total_trials, 0);
        assert_eq!(score.total_fails, 0);
        assert_eq!(score.total_succeeds, 0);
        assert_eq!(score.total_errors, 0);
        assert_eq!(score.error_patch, 0);
        assert_eq!(score.error_check, 0);
        assert_eq!(score.error_response_parse, 0);
        assert_eq!(score.error_other, 0);
        assert_eq!(score.total_words, 0);
        assert_eq!(score.total_time, 0.0);
    }

    #[test]
    fn test_model_score_add_report() {
        let mut score = ModelScore::new("gpt-4".to_string(), "gpt-4-0613".to_string());

        let report1 = TrialReport {
            trial_name: "test1".to_string(),
            model_name: "gpt-4".to_string(),
            api_model: "gpt-4-0613".to_string(),
            n: 1,
            failed: false,
            steps: 3,
            error_patch: 0,
            error_check: 0,
            error_response_parse: 0,
            error_other: 0,
            total_response_time: 1.5,
            words_received: 100,
        };

        let report2 = TrialReport {
            trial_name: "test2".to_string(),
            model_name: "gpt-4".to_string(),
            api_model: "gpt-4-0613".to_string(),
            n: 2,
            failed: true,
            steps: 2,
            error_patch: 1,
            error_check: 1,
            error_response_parse: 0,
            error_other: 1,
            total_response_time: 2.0,
            words_received: 150,
        };

        score.add_report(&report1);
        assert_eq!(score.total_trials, 1);
        assert_eq!(score.total_succeeds, 1);
        assert_eq!(score.total_fails, 0);
        assert_eq!(score.total_words, 100);
        assert_eq!(score.total_time, 1.5);

        score.add_report(&report2);
        assert_eq!(score.total_trials, 2);
        assert_eq!(score.total_succeeds, 1);
        assert_eq!(score.total_fails, 1);
        assert_eq!(score.error_patch, 1);
        assert_eq!(score.error_check, 1);
        assert_eq!(score.error_other, 1);
        assert_eq!(score.total_errors, 3);
        assert_eq!(score.total_words, 250);
        assert_eq!(score.total_time, 3.5);
    }

    #[test]
    fn test_model_scores() {
        let reports = vec![
            TrialReport {
                trial_name: "test1".to_string(),
                model_name: "gpt-4".to_string(),
                api_model: "gpt-4-0613".to_string(),
                n: 1,
                failed: false,
                steps: 2,
                error_patch: 0,
                error_check: 0,
                error_response_parse: 0,
                error_other: 0,
                total_response_time: 1.0,
                words_received: 100,
            },
            TrialReport {
                trial_name: "test2".to_string(),
                model_name: "claude".to_string(),
                api_model: "claude-2".to_string(),
                n: 1,
                failed: true,
                steps: 3,
                error_patch: 1,
                error_check: 0,
                error_response_parse: 0,
                error_other: 0,
                total_response_time: 2.0,
                words_received: 150,
            },
            TrialReport {
                trial_name: "test3".to_string(),
                model_name: "gpt-4".to_string(),
                api_model: "gpt-4-0613".to_string(),
                n: 2,
                failed: true,
                steps: 1,
                error_patch: 0,
                error_check: 1,
                error_response_parse: 0,
                error_other: 0,
                total_response_time: 1.5,
                words_received: 75,
            },
        ];

        let scores = model_scores(reports.iter());
        assert_eq!(scores.len(), 2);

        let gpt4_score = scores.iter().find(|s| s.model_name == "gpt-4").unwrap();
        assert_eq!(gpt4_score.total_trials, 2);
        assert_eq!(gpt4_score.total_succeeds, 1);
        assert_eq!(gpt4_score.total_fails, 1);
        assert_eq!(gpt4_score.error_check, 1);
        assert_eq!(gpt4_score.total_words, 175);
        assert_eq!(gpt4_score.total_time, 2.5);

        let claude_score = scores.iter().find(|s| s.model_name == "claude").unwrap();
        assert_eq!(claude_score.total_trials, 1);
        assert_eq!(claude_score.total_succeeds, 0);
        assert_eq!(claude_score.total_fails, 1);
        assert_eq!(claude_score.error_patch, 1);
        assert_eq!(claude_score.total_words, 150);
        assert_eq!(claude_score.total_time, 2.0);
    }
}
