use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use p256::ecdh::diffie_hellman;
use p256::elliptic_curve::rand_core::OsRng;
use p256::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
#[error("invalid encrypted chat payload or key")]
pub struct CryptoError;

pub struct ChatIdentity {
    key: SecretKey,
}

#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    #[serde(rename = "publicKey")]
    public_key: serde_json::Value,
    #[serde(rename = "privateKey")]
    private_key: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct EncryptedPacket {
    iv: Vec<u8>,
    data: Vec<u8>,
}

impl ChatIdentity {
    pub fn generate() -> Self {
        Self {
            key: SecretKey::random(&mut OsRng),
        }
    }

    pub fn import(serialized: &str) -> Result<Self, CryptoError> {
        let identity: StoredIdentity = serde_json::from_str(serialized).map_err(|_| CryptoError)?;
        let key = SecretKey::from_jwk_str(&canonical_jwk(&identity.private_key, true)?)
            .map_err(|_| CryptoError)?;
        let public = PublicKey::from_jwk_str(&canonical_jwk(&identity.public_key, false)?)
            .map_err(|_| CryptoError)?;
        if key.public_key() != public {
            return Err(CryptoError);
        }
        Ok(Self { key })
    }

    pub fn export(&self) -> Result<Zeroizing<String>, CryptoError> {
        let mut private_key: serde_json::Value =
            serde_json::from_str(self.key.to_jwk_string().as_str()).map_err(|_| CryptoError)?;
        private_key["key_ops"] = serde_json::json!(["deriveKey", "deriveBits"]);
        private_key["ext"] = serde_json::Value::Bool(true);
        let serialized = serde_json::to_string(&StoredIdentity {
            public_key: self.public_key()?,
            private_key,
        })
        .map_err(|_| CryptoError)?;
        Ok(Zeroizing::new(serialized))
    }

    pub fn public_key(&self) -> Result<serde_json::Value, CryptoError> {
        let mut key: serde_json::Value =
            serde_json::from_str(&self.key.public_key().to_jwk_string())
                .map_err(|_| CryptoError)?;
        key["key_ops"] = serde_json::json!([]);
        key["ext"] = serde_json::Value::Bool(true);
        Ok(key)
    }

    pub fn derive(&self, remote: &serde_json::Value) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
        let public =
            PublicKey::from_jwk_str(&canonical_jwk(remote, false)?).map_err(|_| CryptoError)?;
        let shared = diffie_hellman(self.key.to_nonzero_scalar(), public.as_affine());
        Ok(Zeroizing::new((*shared.raw_secret_bytes()).into()))
    }
}

fn canonical_jwk(
    value: &serde_json::Value,
    private: bool,
) -> Result<Zeroizing<String>, CryptoError> {
    #[derive(Serialize)]
    struct Key<'a> {
        kty: &'a str,
        crv: &'a str,
        x: &'a str,
        y: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        d: Option<&'a str>,
    }
    let key = Key {
        kty: value["kty"].as_str().ok_or(CryptoError)?,
        crv: value["crv"].as_str().ok_or(CryptoError)?,
        x: value["x"].as_str().ok_or(CryptoError)?,
        y: value["y"].as_str().ok_or(CryptoError)?,
        d: if private {
            Some(value["d"].as_str().ok_or(CryptoError)?)
        } else {
            None
        },
    };
    if key.kty != "EC" || key.crv != "P-256" {
        return Err(CryptoError);
    }
    serde_json::to_string(&key)
        .map(Zeroizing::new)
        .map_err(|_| CryptoError)
}

pub fn encrypt(key: &[u8; 32], plaintext: &str) -> Result<String, CryptoError> {
    let mut iv = [0_u8; 12];
    getrandom::fill(&mut iv).map_err(|_| CryptoError)?;
    encrypt_with_iv(key, plaintext, iv)
}

fn encrypt_with_iv(key: &[u8; 32], plaintext: &str, iv: [u8; 12]) -> Result<String, CryptoError> {
    if plaintext.len() > 64 * 1024 {
        return Err(CryptoError);
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError)?;
    let data = cipher
        .encrypt(Nonce::from_slice(&iv), plaintext.as_bytes())
        .map_err(|_| CryptoError)?;
    serde_json::to_string(&EncryptedPacket {
        iv: iv.to_vec(),
        data,
    })
    .map_err(|_| CryptoError)
}

pub fn decrypt(key: &[u8; 32], serialized: &str) -> Result<Zeroizing<String>, CryptoError> {
    if serialized.len() > 320 * 1024 {
        return Err(CryptoError);
    }
    let packet: EncryptedPacket = serde_json::from_str(serialized).map_err(|_| CryptoError)?;
    if packet.iv.len() != 12 || packet.data.len() < 16 || packet.data.len() > 64 * 1024 + 16 {
        return Err(CryptoError);
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError)?;
    let data = cipher
        .decrypt(Nonce::from_slice(&packet.iv), packet.data.as_slice())
        .map_err(|_| CryptoError)?;
    String::from_utf8(data)
        .map(Zeroizing::new)
        .map_err(|_| CryptoError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_roundtrip_without_changing_peer_shared_key() {
        let alice = ChatIdentity::generate();
        let bob = ChatIdentity::generate();
        let restored = ChatIdentity::import(&alice.export().unwrap()).unwrap();
        let alice_key = restored.derive(&bob.public_key().unwrap()).unwrap();
        let bob_key = bob.derive(&alice.public_key().unwrap()).unwrap();
        assert_eq!(*alice_key, *bob_key);
        let packet = encrypt(&alice_key, "привет 👋").unwrap();
        assert_eq!(decrypt(&bob_key, &packet).unwrap().as_str(), "привет 👋");
        let mut changed: serde_json::Value = serde_json::from_str(&packet).unwrap();
        changed["data"][0] = serde_json::json!((changed["data"][0].as_u64().unwrap() + 1) % 256);
        assert!(decrypt(&bob_key, &changed.to_string()).is_err());
    }

    #[test]
    fn unauthenticated_and_invalid_iv_packets_are_rejected() {
        assert!(decrypt(&[0; 32], "{\"iv\":[],\"data\":[]}").is_err());
        assert!(decrypt(&[0; 32], "{\"iv\":[256],\"data\":[]}").is_err());
        assert!(decrypt(&[0; 32], "not JSON").is_err());
    }
}
