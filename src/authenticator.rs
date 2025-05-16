use std::sync::Arc;

use hmac::{Hmac, Mac};
use sha1::{
    Sha1,
    digest::generic_array::{GenericArray, typenum::U20},
};

type HmacSha1 = Hmac<Sha1>;
pub type Sha1Bytes = GenericArray<u8, U20>;

#[derive(Debug)]
struct AuthenticatorInner {
    secret: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Authenticator {
    inner: Arc<AuthenticatorInner>,
}

impl Authenticator {
    pub fn new(secret: &[u8]) -> Self {
        Self {
            inner: Arc::new(AuthenticatorInner {
                secret: secret.to_owned(),
            }),
        }
    }

    pub fn sign(&self, value: i64) -> Sha1Bytes {
        let mut mac = HmacSha1::new_from_slice(&self.inner.secret).unwrap();
        mac.update(&value.to_le_bytes());
        mac.finalize().into_bytes()
    }

    pub fn verify(&self, value: i64, signature: &Sha1Bytes) -> bool {
        self.sign(value) == *signature
    }
}
