//! Zero-copy parser for `Sec-WebSocket-Extensions` header as specified in [Section 9.1 of RFC6455](https://datatracker.ietf.org/doc/html/rfc6455#section-9.1).

use std::{borrow::Cow, fmt, str::Split};

/// Error encountered when parsing the `Sec-WebSocket-Extensions` header.
#[derive(Debug)]
pub enum Error {
    /// No extension name was provided.
    ExpectedExtensionName,
    /// No parameter name was provided.
    ExpectedParamName,
    /// No parameter value was provided.
    ExpectedParamValue,
}

impl Error {
    /// Stringify this variant.
    pub(super) const fn as_str(&self) -> &'static str {
        match self {
            Self::ExpectedExtensionName => "no extension name provided",
            Self::ExpectedParamName => "no parameter name provided",
            Self::ExpectedParamValue => "no parameter value provided",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for Error {}

/// A proposed extension with optional parameters.
#[derive(Debug)]
pub struct Extension<'a> {
    pub name: &'a str,
    pub params: ParamsIterator<'a>,
}

impl<'a> Extension<'a> {
    /// Create a new extension and an iterator over its parameters.
    fn new(name: &'a str, params: &'a str) -> Self {
        Self {
            name,
            params: ParamsIterator::new(params),
        }
    }
}

/// Iterator over parameters of an [`Extension`].
#[derive(Debug)]
pub struct ParamsIterator<'a> {
    iter: std::str::Split<'a, char>,
}

impl<'a> ParamsIterator<'a> {
    /// Create a new iterator over parameters specified.
    fn new(params: &'a str) -> Self {
        Self {
            iter: params.trim().split(';'),
        }
    }
}

impl<'a> Iterator for ParamsIterator<'a> {
    type Item = Result<(&'a str, Option<&'a str>), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().and_then(|param| {
            let param = param.trim();

            if param.is_empty() {
                None
            } else {
                if let Some((key, value)) = param.split_once('=') {
                    if key.is_empty() {
                        Some(Err(Error::ExpectedParamName))
                    } else if value.is_empty() {
                        Some(Err(Error::ExpectedParamValue))
                    } else {
                        Some(Ok((key, Some(value))))
                    }
                } else {
                    Some(Ok((param, None)))
                }
            }
        })
    }
}

/// Iterator over [`Extension`]s in a `Sec-WebSocket-Extensions` header.
pub struct ExtensionIterator<'a> {
    iter: Split<'a, char>,
}

impl<'a> ExtensionIterator<'a> {
    /// Create a new iterator over extensions in a `Sec-WebSocket-Extensions`
    /// header.
    pub fn new(header: &'a str) -> Self {
        ExtensionIterator {
            iter: header.split(','),
        }
    }
}

impl<'a> Iterator for ExtensionIterator<'a> {
    type Item = Result<Extension<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|extension| {
            let trimmed = extension.trim();
            let (name, params) = match trimmed.split_once(';') {
                Some((name, params)) => (name, params),
                None => (trimmed, ""),
            };
            if name.is_empty() {
                Err(Error::ExpectedExtensionName)
            } else {
                Ok(Extension::new(name, params))
            }
        })
    }
}

pub struct Proposal(Cow<'static, str>);

impl Proposal {
    pub fn new(extension_name: &'static str) -> Self {
        Self(Cow::Borrowed(extension_name))
    }

    pub fn add_param(&mut self, key: &str, value: Option<&str>) {
        let was_borrowed = matches!(self.0, Cow::Borrowed(_));
        let buf = self.0.to_mut();

        buf.reserve(
            (was_borrowed as usize * 2)
                + key.len()
                + value.is_some() as usize
                + value.unwrap_or_default().len(),
        );

        if was_borrowed {
            buf.push_str("; ");
        };

        buf.push_str(key);

        if let Some(value) = value {
            buf.push('=');
            buf.push_str(value);
        }
    }

    pub fn finish(self) -> String {
        self.0.into_owned()
    }
}

#[test]
fn test_parse_header() {
    let inputs = &[
        "permessage-deflate; client_max_window_bits=15, x-webkit-deflate-frame",
        "deflate-stream",
        "mux; max-channels=4; flow-control, deflate-stream",
        "private-extension",
        "superspeed, colormode; depth=16",
        "superspeed; a",
        "superspeed;,priv",
        "superspeed;",
    ];

    for input in inputs {
        for extension in ExtensionIterator::new(input) {
            let ext = extension.unwrap();
            for param in ext.params {
                assert!(param.is_ok());
            }
        }
    }
}
