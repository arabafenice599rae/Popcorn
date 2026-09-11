//! Key files (SPEC.md §1): the node key and the foundation key are distinct, and stay that
//! way.
//!
//! The node key signs blocks and controls no funds; the foundation key holds value and signs
//! no blocks. A node that could sign with the foundation key would collapse that separation,
//! so the node process only ever loads the former.

use std::path::Path;

use ed25519_dalek::SigningKey;

use crate::encoding::{hex32, to_hex};

#[derive(Debug)]
pub enum KeyError {
    Io(String),
    Malformed,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::Io(m) => write!(f, "key file: {m}"),
            KeyError::Malformed => write!(f, "key file must contain 32 hex-encoded bytes"),
        }
    }
}

/// Generate a key from operating-system randomness.
pub fn generate() -> SigningKey {
    let mut seed = [0u8; 32];
    getrandom(&mut seed);
    SigningKey::from_bytes(&seed)
}

/// Write a key as hex, readable by its owner only.
pub fn save(key: &SigningKey, path: &Path) -> Result<(), KeyError> {
    std::fs::write(path, format!("{}\n", to_hex(&key.to_bytes())))
        .map_err(|e| KeyError::Io(e.to_string()))?;
    restrict_permissions(path)?;
    Ok(())
}

pub fn load(path: &Path) -> Result<SigningKey, KeyError> {
    let raw = std::fs::read_to_string(path).map_err(|e| KeyError::Io(e.to_string()))?;
    let bytes = hex32(raw.trim()).ok_or(KeyError::Malformed)?;
    Ok(SigningKey::from_bytes(&bytes))
}

/// Read a public key from a key file without keeping the secret around.
pub fn public_of(path: &Path) -> Result<[u8; 32], KeyError> {
    Ok(load(path)?.verifying_key().to_bytes())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), KeyError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, permissions).map_err(|e| KeyError::Io(e.to_string()))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<(), KeyError> {
    Ok(())
}

/// Operating-system randomness, without adding a dependency for it.
fn getrandom(buffer: &mut [u8; 32]) {
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut file = std::fs::File::open("/dev/urandom").expect("/dev/urandom");
        file.read_exact(buffer).expect("read randomness");
    }
    #[cfg(not(unix))]
    {
        compile_error!("key generation needs a platform randomness source");
    }
}
