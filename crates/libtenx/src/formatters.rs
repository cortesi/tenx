pub use crate::lang::rust::*;

use crate::{config::Config, validators::Runnable, Result, Session};

pub trait Formatter {
    fn name(&self) -> &'static str;
    fn format(&self, state: &Session) -> Result<()>;
    fn is_relevant(&self, config: &Config, state: &Session) -> Result<bool>;
    fn is_configured(&self, config: &Config) -> bool;
    fn runnable(&self) -> Result<Runnable>;
}

pub fn all_formatters() -> Vec<Box<dyn Formatter>> {
    vec![Box::new(CargoFormatter)]
}

pub fn relevant_formatters(config: &Config, state: &Session) -> Result<Vec<Box<dyn Formatter>>> {
    let mut formatters: Vec<Box<dyn Formatter>> = Vec::new();
    for formatter in all_formatters() {
        if formatter.is_configured(config)
            && formatter.is_relevant(config, state)?
            && formatter.runnable()?.is_ok()
        {
            formatters.push(formatter);
        }
    }
    Ok(formatters)
}
