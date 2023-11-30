use std::fmt;

use super::parser::Error as ParseError;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Parsing(ParseError),
    UnknownParameter,
    ParamDisallowsValue,
    ParamRequiresValue,
    ExtensionNeverProposed,
    DuplicateExtension,
    #[cfg(feature = "permessage-deflate")]
    Compress(flate2::CompressError),
    #[cfg(feature = "permessage-deflate")]
    Decompress(flate2::DecompressError),
    #[cfg(feature = "permessage-deflate")]
    InvalidServerMaxWindowBits,
    #[cfg(feature = "permessage-deflate")]
    InvalidClientMaxWindowBits,
    #[cfg(feature = "permessage-deflate")]
    DisallowedClientMaxWindowBits,
}

impl From<ParseError> for Error {
    fn from(err: ParseError) -> Self {
        Self::Parsing(err)
    }
}

#[cfg(feature = "permessage-deflate")]
impl From<flate2::CompressError> for Error {
    fn from(err: flate2::CompressError) -> Self {
        Self::Compress(err)
    }
}

#[cfg(feature = "permessage-deflate")]
impl From<flate2::DecompressError> for Error {
    fn from(err: flate2::DecompressError) -> Self {
        Self::Decompress(err)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parsing(e) => e.fmt(f),
            Self::UnknownParameter => f.write_str("unknown extension parameter"),
            Self::ParamDisallowsValue => {
                f.write_str("got value for parameter that does not take values")
            }
            Self::ParamRequiresValue => f.write_str("missing value for parameter"),
            Self::ExtensionNeverProposed => {
                f.write_str("server used extension that was never proposed")
            }
            Self::DuplicateExtension => f.write_str("extension was proposed twice"),
            #[cfg(feature = "permessage-deflate")]
            Self::Compress(e) => e.fmt(f),
            #[cfg(feature = "permessage-deflate")]
            Self::Decompress(e) => e.fmt(f),
            #[cfg(feature = "permessage-deflate")]
            Self::InvalidServerMaxWindowBits => f.write_str("invalid server max window bits"),
            #[cfg(feature = "permessage-deflate")]
            Self::InvalidClientMaxWindowBits => f.write_str("invalid client max window bits"),
            #[cfg(feature = "permessage-deflate")]
            Self::DisallowedClientMaxWindowBits => {
                f.write_str("server used client max window bits not proposed by client")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parsing(e) => Some(e),
            #[cfg(feature = "permessage-deflate")]
            Self::Compress(e) => Some(e),
            #[cfg(feature = "permessage-deflate")]
            Self::Decompress(e) => Some(e),
            _ => None,
        }
    }
}
