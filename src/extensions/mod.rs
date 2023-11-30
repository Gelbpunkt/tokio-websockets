pub use self::error::Error;
pub(crate) use self::parser::ExtensionIterator;
pub(self) use self::parser::{ParamsIterator, Proposal};
use crate::proto::Role;

mod error;
mod parser;
#[cfg(feature = "permessage-deflate")]
pub mod permessage_deflate;

pub(self) trait Configuration: Sized {
    type Extension: From<(Self, Role)>;

    /// Name of the extension as negotiated in the `Sec-WebSocket-Extensions`
    /// header.
    fn name() -> &'static str;

    /// Formats extension parameters to use or suggest for the
    /// `Sec-WebSocket-Extensions` header.
    #[cfg(feature = "client")]
    fn generate_client_proposal(&self, proposal: &mut Proposal);

    /// Parse extension parameters proposed by the client in the
    /// `Sec-WebSocket-Extensions` header.
    ///
    /// The counter-proposal should get written to the [`Proposal`] provided.
    ///
    /// Invalid or unknown parameters receivedhere must result in an error.
    #[cfg(feature = "server")]
    fn accept_client_proposal(
        &self,
        params: ParamsIterator<'_>,
        accepted: &mut Proposal,
    ) -> Result<Self::Extension, Error>;

    /// Parse extension parameters proposed by the server in the
    /// `Sec-WebSocket-Extensions` header.
    ///
    /// Invalid or unknown parameters received here must result in an error.
    #[cfg(feature = "client")]
    fn verify_server_proposal(&self, params: ParamsIterator<'_>) -> Result<Self::Extension, Error>;
}

#[derive(Debug, Default)] // By default, all extensions are disabled
pub struct Extensions {
    #[cfg(feature = "permessage-deflate")]
    pub(crate) permessage_deflate: Option<permessage_deflate::Extension>,
}

#[derive(Clone, Debug, Default)] // By default, all extensions are disabled
pub struct ExtensionConfiguration {
    #[cfg(feature = "permessage-deflate")]
    permessage_deflate: Option<permessage_deflate::Configuration>,
}

impl ExtensionConfiguration {
    #[cfg(feature = "permessage-deflate")]
    pub fn permessage_deflate(
        mut self,
        permessage_deflate: permessage_deflate::Configuration,
    ) -> Self {
        self.permessage_deflate = Some(permessage_deflate);
        self
    }

    /// Generate the client `Sec-WebSocket-Extensions` header value, depending
    /// on the configuration. If no extensions are enabled, `None` is
    /// returned.
    #[cfg(feature = "client")]
    pub(crate) fn generate_extension_header(&self) -> Option<String> {
        // As we add support for new extensions, we can just join the proposals with a
        // comma in between. For now, save us some overhead since we only support one.
        #[cfg(feature = "permessage-deflate")]
        if let Some(permessage_deflate) = &self.permessage_deflate {
            let mut proposal = Proposal::new(permessage_deflate::Configuration::name());
            permessage_deflate.generate_client_proposal(&mut proposal);

            Some(proposal.finish())
        } else {
            None
        }

        #[cfg(not(feature = "permessage-deflate"))]
        None
    }

    // TODO: Reject conflicting RSV1 big usage in the future

    /// Generate the header for the `Sec-WebSocket-Extensions` header depending
    /// on the server's configuration.
    #[cfg(feature = "server")]
    pub(crate) fn accept_client_proposals(
        &self,
        client_extensions: ExtensionIterator<'_>,
    ) -> Result<(Option<String>, Extensions), Error> {
        let mut agreed_proposals: Vec<String> = Vec::new();
        let mut extensions = Extensions::default();

        for extension in client_extensions {
            let extension = extension?;

            #[cfg(feature = "permessage-deflate")]
            if let Some(permessage_deflate) = &self.permessage_deflate {
                if extension.name == permessage_deflate::Configuration::name() {
                    // Suggesting multiple possible parameters for the same extension is possible.
                    // We should use the first one since it has higher priority.
                    if extensions.permessage_deflate.is_some() {
                        continue;
                    }
                    let mut accepted = Proposal::new(permessage_deflate::Configuration::name());
                    extensions.permessage_deflate = Some(
                        permessage_deflate
                            .accept_client_proposal(extension.params, &mut accepted)?,
                    );
                    agreed_proposals.push(accepted.finish());
                }
            }

            // We simply reject all unsupported extensions proposed!
        }

        let proposals = if agreed_proposals.is_empty() {
            None
        } else {
            Some(agreed_proposals.join(", "))
        };

        Ok((proposals, extensions))
    }

    #[cfg(feature = "client")]
    pub(crate) fn verify_server_proposals(
        &self,
        server_extensions: ExtensionIterator,
    ) -> Result<Extensions, Error> {
        let mut extensions = Extensions::default();

        for extension in server_extensions {
            let extension = extension?;

            #[cfg(feature = "permessage-deflate")]
            if extension.name == permessage_deflate::Configuration::name() {
                if extensions.permessage_deflate.is_some() {
                    return Err(Error::DuplicateExtension);
                }

                if let Some(permessage_deflate) = &self.permessage_deflate {
                    extensions.permessage_deflate =
                        Some(permessage_deflate.verify_server_proposal(extension.params)?);
                }
            } else {
                return Err(Error::ExtensionNeverProposed);
            }
        }

        Ok(extensions)
    }

    pub(crate) fn into_extensions(self, role: Role) -> Extensions {
        Extensions {
            #[cfg(feature = "permessage-deflate")]
            permessage_deflate: self
                .permessage_deflate
                .map(|deflate| permessage_deflate::Extension::from((deflate, role))),
        }
    }
}
