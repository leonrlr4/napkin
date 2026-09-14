//! Randomness and time, behind a trait so tests can pin them.

use std::time::{SystemTime, UNIX_EPOCH};

pub trait Env {
    fn fill_random(&mut self, bytes: &mut [u8]);
    /// Milliseconds since the Unix epoch (JS `Date.now()`).
    fn now_ms(&mut self) -> f64;
}

pub struct SystemEnv;

impl Env for SystemEnv {
    fn fill_random(&mut self, bytes: &mut [u8]) {
        getrandom::fill(bytes).expect("OS random source");
    }

    fn now_ms(&mut self) -> f64 {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970");
        since_epoch.as_millis() as f64
    }
}

/// nanoid@3.3.3's `urlAlphabet`.
const URL_ALPHABET: &[u8; 64] = b"useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict";

/// `nanoid()`: 21 characters, each a random byte masked to 6 bits.
pub fn random_id(env: &mut impl Env) -> String {
    let mut bytes = [0u8; 21];
    env.fill_random(&mut bytes);
    bytes
        .iter()
        .map(|b| URL_ALPHABET[usize::from(b & 63)] as char)
        .collect()
}

/// Excalidraw's `randomInteger()`: uniform in `[0, 2^31)`.
pub fn random_integer(env: &mut impl Env) -> f64 {
    let mut bytes = [0u8; 4];
    env.fill_random(&mut bytes);
    f64::from(u32::from_le_bytes(bytes) >> 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_have_nanoid_format() {
        let mut env = SystemEnv;
        let id = random_id(&mut env);
        assert_eq!(id.len(), 21);
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
            "{id}"
        );
        assert_ne!(id, random_id(&mut env));
    }

    #[test]
    fn integers_stay_below_two_to_the_31() {
        let mut env = SystemEnv;
        assert!(
            (0..1000)
                .map(|_| random_integer(&mut env))
                .all(|n| (0.0..2_147_483_648.0).contains(&n))
        );
    }
}
