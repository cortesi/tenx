use crate::{
    config::Config,
    session::{Session, Step},
};

use super::core::*;

pub struct Fix {
    _prompt: Option<String>,
}

impl Action for Fix {
    fn next_step(&self, _config: &Config, _session: &Session) -> Option<Step> {
        None
    }
}
