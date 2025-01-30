use crate::{
    config::Config,
    session::{Session, Step},
};

use super::core::*;

pub struct Code {
    _prompt: Option<String>,
}

impl Action for Code {
    fn next_step(&self, _config: &Config, _session: &Session) -> Option<Step> {
        None
    }
}
