use std::num::NonZeroU8;

use bytes::BytesMut;
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

use crate::proto::Role;

const PER_MESSAGE_DEFLATE: &str = "permessage-deflate";
const CLIENT_NO_CONTEXT_TAKEOVER: &str = "client_no_context_takeover";
const SERVER_NO_CONTEXT_TAKEOVER: &str = "server_no_context_takeover";
const CLIENT_MAX_WINDOW_BITS: &str = "client_max_window_bits";
const SERVER_MAX_WINDOW_BITS: &str = "server_max_window_bits";

/// Default LZ77 sliding window size.
const DEFAULT_MAX_WINDOW_BITS: NonZeroU8 = unsafe { NonZeroU8::new_unchecked(15) };

const ZLIB_TRAILER: [u8; 4] = [0x00, 0x00, 0xff, 0xff];

/// Reimplementation of [`flate2::Decompress::decompress_vec`] for [`BytesMut`].
fn decompress_bytesmut(
    decompress: &mut Decompress,
    input: &[u8],
    output: &mut BytesMut,
    flush: FlushDecompress,
) -> Result<Status, flate2::DecompressError> {
    let cap = output.capacity();
    let len = output.len();

    unsafe {
        let before = decompress.total_out();
        let ret = {
            let ptr = output.as_mut_ptr().offset(len as isize);
            let out = std::slice::from_raw_parts_mut(ptr, cap - len);
            decompress.decompress(input, out, flush)
        };
        output.set_len((decompress.total_out() - before) as usize + len);
        ret
    }
}

/// Validates that the parameter for max window bits
/// does not have leading zeroes and is in the range between 9 and 15.
///
/// This parameter is the base-2 logarithm of the LZ77 sliding window
/// size. If absent, the window may be up to 32,768 bytes large.
fn is_allowed_window_bits(input: &str) -> bool {
    matches!(input, "9" | "10" | "11" | "12" | "13" | "14" | "15")
}

#[derive(Clone, Debug)]
pub struct Configuration {
    compression_level: Compression,
    server_no_context_takeover: bool,
    client_no_context_takeover: bool,
    server_max_window_bits: NonZeroU8,
    client_max_window_bits: NonZeroU8,
}

impl Configuration {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn compression_level(mut self, compression_level: Compression) -> Self {
        self.compression_level = compression_level;
        self
    }

    pub fn server_no_context_takeover(mut self, server_no_context_takeover: bool) -> Self {
        self.server_no_context_takeover = server_no_context_takeover;
        self
    }

    pub fn client_no_context_takeover(mut self, client_no_context_takeover: bool) -> Self {
        self.client_no_context_takeover = client_no_context_takeover;
        self
    }

    pub fn server_max_window_bits(mut self, server_max_window_bits: NonZeroU8) -> Self {
        assert!(
            9 <= server_max_window_bits.get() && server_max_window_bits.get() <= 15,
            "max window bits must be between 9 and 15"
        );
        self.server_max_window_bits = server_max_window_bits;
        self
    }

    pub fn client_max_window_bits(mut self, client_max_window_bits: NonZeroU8) -> Self {
        assert!(
            9 <= client_max_window_bits.get() && client_max_window_bits.get() <= 15,
            "max window bits must be between 9 and 15"
        );
        self.client_max_window_bits = client_max_window_bits;
        self
    }
}

// It's better to be explicit here...
impl Default for Configuration {
    fn default() -> Self {
        Self {
            compression_level: Compression::default(),
            server_no_context_takeover: false,
            client_no_context_takeover: false,
            server_max_window_bits: DEFAULT_MAX_WINDOW_BITS,
            client_max_window_bits: DEFAULT_MAX_WINDOW_BITS,
        }
    }
}

impl super::Configuration for Configuration {
    type Extension = Extension;

    fn name() -> &'static str {
        PER_MESSAGE_DEFLATE
    }

    #[cfg(feature = "client")]
    fn generate_client_proposal(&self, proposal: &mut super::Proposal) {
        if self.server_no_context_takeover {
            proposal.add_param(SERVER_NO_CONTEXT_TAKEOVER, None);
        }

        if self.client_no_context_takeover {
            proposal.add_param(CLIENT_NO_CONTEXT_TAKEOVER, None);
        }

        if self.server_max_window_bits != DEFAULT_MAX_WINDOW_BITS {
            proposal.add_param(
                SERVER_MAX_WINDOW_BITS,
                Some(&self.server_max_window_bits.to_string()),
            );
        }

        if self.client_max_window_bits != DEFAULT_MAX_WINDOW_BITS {
            proposal.add_param(
                CLIENT_MAX_WINDOW_BITS,
                Some(&self.client_max_window_bits.to_string()),
            );
        }
    }

    #[cfg(feature = "server")]
    fn accept_client_proposal(
        &self,
        params: super::ParamsIterator<'_>,
        accepted: &mut super::Proposal,
    ) -> Result<Self::Extension, super::Error> {
        let mut parameters = Self {
            compression_level: self.compression_level,
            ..Self::default()
        };

        for param in params {
            match param? {
                (SERVER_NO_CONTEXT_TAKEOVER, None) => {
                    parameters.server_no_context_takeover = true;
                    accepted.add_param(SERVER_NO_CONTEXT_TAKEOVER, None);
                }
                (SERVER_NO_CONTEXT_TAKEOVER, Some(_)) => {
                    return Err(super::Error::ParamDisallowsValue);
                }
                (CLIENT_NO_CONTEXT_TAKEOVER, None) => {
                    parameters.client_no_context_takeover = true;
                    accepted.add_param(CLIENT_NO_CONTEXT_TAKEOVER, None);
                }
                (CLIENT_NO_CONTEXT_TAKEOVER, Some(_)) => {
                    return Err(super::Error::ParamDisallowsValue)
                }
                (SERVER_MAX_WINDOW_BITS, Some(bits)) => {
                    if !is_allowed_window_bits(bits) {
                        return Err(super::Error::InvalidServerMaxWindowBits);
                    }

                    // SAFETY: We have verified that this is a non-null integer in the range of
                    // a u8.
                    parameters.server_max_window_bits = unsafe { bits.parse().unwrap_unchecked() };
                    accepted.add_param(SERVER_MAX_WINDOW_BITS, Some(bits));
                }
                (SERVER_MAX_WINDOW_BITS, None) => return Err(super::Error::ParamRequiresValue),
                (CLIENT_MAX_WINDOW_BITS, Some(bits)) => {
                    if !is_allowed_window_bits(bits) {
                        return Err(super::Error::InvalidClientMaxWindowBits);
                    }

                    // SAFETY: We have verified that this is a non-null integer in the range of
                    // a u8.
                    parameters.server_max_window_bits = unsafe { bits.parse().unwrap_unchecked() };
                    accepted.add_param(CLIENT_MAX_WINDOW_BITS, Some(bits));
                }
                (CLIENT_MAX_WINDOW_BITS, None) => return Err(super::Error::ParamRequiresValue),
                _ => return Err(super::Error::UnknownParameter),
            }
        }

        Ok(Extension::new(parameters, Role::Server))
    }

    #[cfg(feature = "client")]
    fn verify_server_proposal(
        &self,
        params: super::parser::ParamsIterator<'_>,
    ) -> Result<Self::Extension, super::Error> {
        let mut parameters = Self {
            compression_level: self.compression_level,
            ..Self::default()
        };

        for param in params {
            match param? {
                (SERVER_NO_CONTEXT_TAKEOVER, None) => {
                    // Servers can force this even if it was not in the client proposal.
                    parameters.server_no_context_takeover = true;
                }
                (SERVER_NO_CONTEXT_TAKEOVER, Some(_)) => {
                    return Err(super::Error::ParamDisallowsValue);
                }
                (CLIENT_NO_CONTEXT_TAKEOVER, None) => {
                    // Servers can force this even if it was not in the client proposal.
                    parameters.client_no_context_takeover = true;
                }
                (CLIENT_NO_CONTEXT_TAKEOVER, Some(_)) => {
                    return Err(super::Error::ParamDisallowsValue)
                }
                (SERVER_MAX_WINDOW_BITS, Some(bits)) => {
                    // Servers can force this even if it was not in the client proposal.

                    if !is_allowed_window_bits(bits) {
                        return Err(super::Error::InvalidServerMaxWindowBits);
                    }

                    // SAFETY: We have verified that this is a non-null integer in the range of
                    // a u8.
                    parameters.server_max_window_bits = unsafe { bits.parse().unwrap_unchecked() };
                }
                (SERVER_MAX_WINDOW_BITS, None) => return Err(super::Error::ParamRequiresValue),
                (CLIENT_MAX_WINDOW_BITS, Some(bits)) => {
                    if !is_allowed_window_bits(bits) {
                        return Err(super::Error::InvalidClientMaxWindowBits);
                    }

                    // SAFETY: We have verified that this is a non-null integer in the range of
                    // a u8.
                    parameters.server_max_window_bits = unsafe { bits.parse().unwrap_unchecked() };

                    if self.client_max_window_bits != parameters.client_max_window_bits {
                        // This must have been proposed by the client previously.
                        return Err(super::Error::DisallowedClientMaxWindowBits);
                    }
                }
                (CLIENT_MAX_WINDOW_BITS, None) => return Err(super::Error::ParamRequiresValue),
                _ => return Err(super::Error::UnknownParameter),
            }
        }

        Ok(Extension::new(parameters, Role::Client))
    }
}

#[derive(Debug)]
pub(crate) struct Extension {
    parameters: Configuration,
    role: Role,
    compress: Compress,
    decompress: Decompress,
}

impl Extension {
    pub fn new(parameters: Configuration, role: Role) -> Self {
        let compress_window_bits = match role {
            Role::Client => parameters.client_max_window_bits,
            Role::Server => parameters.server_max_window_bits,
        };
        let decompress_window_bits = match role {
            Role::Client => parameters.server_max_window_bits,
            Role::Server => parameters.client_max_window_bits,
        };

        let compress = Compress::new_with_window_bits(
            parameters.compression_level,
            false,
            compress_window_bits.into(),
        );
        let decompress = Decompress::new_with_window_bits(false, decompress_window_bits.into());

        Self {
            parameters,
            role,
            compress,
            decompress,
        }
    }

    fn no_local_context_takeover(&self) -> bool {
        match self.role {
            Role::Client => self.parameters.client_no_context_takeover,
            Role::Server => self.parameters.server_no_context_takeover,
        }
    }

    fn no_remote_context_takeover(&self) -> bool {
        match self.role {
            Role::Client => self.parameters.server_no_context_takeover,
            Role::Server => self.parameters.client_no_context_takeover,
        }
    }

    /// Compress the payload of a websocket message.
    pub(crate) fn compress(&mut self, payload: &[u8]) -> Result<Vec<u8>, super::Error> {
        // 1. Compress all the octets of the payload of the message using DEFLATE.
        let mut output = Vec::with_capacity(payload.len());
        let before_in = self.compress.total_in() as usize;
        let mut total_in;
        let mut offset = 0;

        loop {
            if offset >= payload.len() {
                break;
            }

            match self.compress.compress_vec(
                unsafe { payload.get_unchecked(offset..) },
                &mut output,
                FlushCompress::None,
            )? {
                Status::Ok => {}
                Status::BufError => output.reserve(4096),
                Status::StreamEnd => break,
            }

            total_in = self.compress.total_in() as usize;
            offset = total_in - before_in;
        }

        // 2. If the resulting data does not end with an empty DEFLATE block with no
        //    compression (the "BTYPE" bits are set to 00), append an empty DEFLATE
        //    block with no compression to the tail end.
        while !output.ends_with(&ZLIB_TRAILER) {
            output.reserve(5);

            match self
                .compress
                .compress_vec(&[], &mut output, FlushCompress::Sync)?
            {
                Status::Ok | Status::BufError => continue,
                Status::StreamEnd => break,
            }
        }

        // 3. Remove 4 octets (that are 0x00 0x00 0xff 0xff) from the tail end. After
        //    this step, the last octet of the compressed data contains (possibly part
        //    of) the DEFLATE header bits with the "BTYPE" bits set to 00.
        output.truncate(output.len() - 4);

        if self.no_local_context_takeover() {
            self.compress.reset();
        }

        Ok(output)
    }

    pub(crate) fn decompress(
        &mut self,
        mut payload: BytesMut,
        is_final: bool,
    ) -> Result<BytesMut, super::Error> {
        if is_final {
            payload.extend_from_slice(&ZLIB_TRAILER);
        }

        let before_in = self.decompress.total_in() as usize;
        // TODO: Reconsider this estimate
        let mut output = BytesMut::with_capacity(2 * payload.len());
        let mut offset = 0;

        loop {
            match decompress_bytesmut(
                &mut self.decompress,
                unsafe { payload.get_unchecked(offset..) },
                &mut output,
                FlushDecompress::None,
            )? {
                // TODO: Reconsider this estimate
                Status::Ok => output.reserve(2 * output.len()),
                Status::BufError | Status::StreamEnd => break,
            }

            offset = self.decompress.total_in() as usize - before_in;
        }

        if is_final && self.no_remote_context_takeover() {
            self.decompress.reset(false);
        }

        Ok(output)
    }
}

impl From<(Configuration, Role)> for Extension {
    fn from((parameters, role): (Configuration, Role)) -> Self {
        Self::new(parameters, role)
    }
}
