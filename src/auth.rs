use crate::error::{Error, Result};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
type Signer = Hmac<Sha256>;
pub struct Auth {
    token_hash: [u8; 32],
    key: [u8; 32],
}
impl Auth {
    pub fn new(token: &str) -> Self {
        Self {
            token_hash: Sha256::digest(token).into(),
            key: rand::random(),
        }
    }
    pub fn accepts(&self, token: &str) -> bool {
        bool::from(self.token_hash.ct_eq(&Sha256::digest(token)))
    }
    pub fn issue(&self, now: u64) -> Result<String> {
        let nonce: [u8; 16] = rand::random();
        let payload = format!("{}.{}", hex::encode(nonce), now.saturating_add(3600));
        let mut mac = Signer::new_from_slice(&self.key).map_err(|_| Error::Auth)?;
        mac.update(payload.as_bytes());
        Ok(format!(
            "{}.{}",
            payload,
            hex::encode(mac.finalize().into_bytes())
        ))
    }
    pub fn verify(&self, cookie: &str, now: u64) -> bool {
        let Some((payload, signature)) = cookie.rsplit_once('.') else {
            return false;
        };
        let Some((nonce, expires)) = payload.split_once('.') else {
            return false;
        };
        if nonce.len() != 32 || hex::decode(nonce).is_err() {
            return false;
        }
        let Ok(expires) = expires.parse::<u64>() else {
            return false;
        };
        if expires <= now || expires > now.saturating_add(3600) {
            return false;
        }
        let Ok(signature) = hex::decode(signature) else {
            return false;
        };
        let Ok(mut mac) = Signer::new_from_slice(&self.key) else {
            return false;
        };
        mac.update(payload.as_bytes());
        mac.verify_slice(&signature).is_ok()
    }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expires_and_restart_revokes() {
        let a = Auth::new("fixture");
        let c = a.issue(100).unwrap();
        assert!(a.verify(&c, 101));
        assert!(!a.verify(&c, 3700));
        assert!(!Auth::new("fixture").verify(&c, 101));
    }
    #[test]
    fn rejects_tampering_and_malformed() {
        let a = Auth::new("fixture");
        let c = a.issue(100).unwrap();
        assert!(!a.verify(&format!("{c}0"), 101));
        for c in ["", "a.b.c", "...", "00.9999.00"] {
            assert!(!a.verify(c, 101));
        }
        assert!(a.accepts("fixture"));
        assert!(!a.accepts("different"));
    }
}
