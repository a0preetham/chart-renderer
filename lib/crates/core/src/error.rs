use std::fmt;

/// Every failure mode is a typed error. Nothing in the render path is allowed to
/// panic — a panic in WASM aborts the whole Worker isolate, so a malformed spec
/// from a request must come back as an `Err`.
#[derive(Debug)]
pub enum Error {
    /// The spec was not valid JSON, or did not match the expected shape.
    Parse(serde_json::Error),
    /// A mark type we do not implement (`area`, `tick`, `rule`, ...).
    UnsupportedMark(String),
    /// A field type we do not implement (`temporal`, `geojson`).
    UnsupportedFieldType(String),
    /// A data source other than inline `values`.
    UnsupportedData(&'static str),
    /// A required encoding channel was absent.
    MissingEncoding(&'static str),
    /// An encoding channel was present but had no `field`.
    MissingField(&'static str),
    /// An encoded field is not present in the data rows.
    UnknownField(String),
    /// The encoding channels are individually valid but not a combination this
    /// mark can draw (e.g. a bar with two quantitative channels).
    UnsupportedEncoding(String),
    /// `width`/`height` were zero, negative, or absurdly large.
    InvalidSize(String),
    /// PNG encoding failed.
    Encode(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse(e) => write!(f, "invalid spec: {e}"),
            Error::UnsupportedMark(m) => write!(f, "unsupported mark type: {m:?}"),
            Error::UnsupportedFieldType(t) => write!(f, "unsupported field type: {t:?}"),
            Error::UnsupportedData(what) => write!(f, "unsupported data source: {what}"),
            Error::MissingEncoding(c) => write!(f, "missing required encoding channel: {c}"),
            Error::MissingField(c) => write!(f, "encoding channel {c} has no field"),
            Error::UnknownField(name) => write!(f, "field {name:?} is not present in the data"),
            Error::UnsupportedEncoding(why) => write!(f, "unsupported encoding: {why}"),
            Error::InvalidSize(why) => write!(f, "invalid chart size: {why}"),
            Error::Encode(e) => write!(f, "png encoding failed: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Parse(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Parse(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
