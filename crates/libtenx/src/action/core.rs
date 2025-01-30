use crate::{
    config::Config,
    session::{Session, Step},
};

pub trait Action {
    fn next_step(&self, config: &Config, session: &Session) -> Option<Step>;
}
