#[cfg(feature = "server")]
mod client_request;
#[cfg(feature = "client")]
mod server_response;

#[cfg(feature = "server")]
pub use client_request::ClientRequestCodec;
#[cfg(feature = "client")]
pub use server_response::ServerResponseCodec;
