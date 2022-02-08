#[cfg(feature = "openssl")]
use openssl::sha::Sha1;
#[cfg(all(feature = "ring", not(feature = "openssl")))]
use ring::digest;
#[cfg(all(feature = "sha1_smol", not(feature = "ring"), not(feature = "openssl")))]
use sha1_smol::Sha1;

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[cfg(all(feature = "sha1_smol", not(feature = "ring"), not(feature = "openssl")))]
/// Calculate the SHA-1 digest of a websocket key and the GUID using the [`sha1`] crate.
pub fn digest(key: &[u8]) -> [u8; 20] {
    let mut s = Sha1::new();
    s.update(key);
    s.update(GUID.as_bytes());
    s.digest().bytes()
}

#[cfg(all(feature = "ring", not(feature = "openssl")))]
/// Calculate the SHA-1 digest of a websocket key and the GUID using the [`ring`] crate.
pub fn digest(key: &[u8]) -> [u8; 20] {
    let mut ctx = digest::Context::new(&digest::SHA1_FOR_LEGACY_USE_ONLY);
    ctx.update(key);
    ctx.update(GUID.as_bytes());
    ctx.finish().as_ref().try_into().unwrap()
}

#[cfg(feature = "openssl")]
/// Calculate the SHA-1 digest of a websocket key and the GUID using the [`openssl`] crate.
pub fn digest(key: &[u8]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(key);
    hasher.update(GUID.as_bytes());
    hasher.finish()
}
