use crate::config;

pub mod ast;
mod error;
mod lint;
mod parser;

pub use ast::{CommitMessage, Footer, FooterSeparator, Header};
pub use error::{LintError, LintErrorKind, MessageError, ParseError, ParseErrorKind};
pub use parser::parse;

pub fn validate(message: &str, config: &config::Config) -> Result<(), String> {
    validate_structured(message, config)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn validate_structured<'a>(
    message: &'a str,
    config: &config::Config,
) -> Result<CommitMessage<'a>, MessageError> {
    let parsed = parse(message).map_err(MessageError::Parse)?;
    lint::lint(&parsed, message, config).map_err(MessageError::Lint)?;
    Ok(parsed)
}
