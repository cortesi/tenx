mod rust;
pub use rust::*;

use crate::{Result, Session};

pub trait Formatter {
    fn name(&self) -> &'static str;
    fn format(&self, state: &Session) -> Result<()>;
}

pub fn formatters(state: &Session) -> Result<Vec<Box<dyn Formatter>>> {
    let mut formatters: Vec<Box<dyn Formatter>> = vec![];
    if state
        .abs_editables()?
        .iter()
        .any(|path| path.extension().map_or(false, |ext| ext == "rs"))
    {
        formatters.push(Box::new(CargoFormatter));
    }
    Ok(formatters)
}
