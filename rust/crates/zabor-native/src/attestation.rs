use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

pub struct Attestation {
    key: Zeroizing<String>,
    channel: &'static str,
    version: String,
}

impl Attestation {
    pub fn from_environment(version: &str) -> Result<Option<Self>, &'static str> {
        let Ok(key) = std::env::var("ZABOR_CLIENT_SECRET") else {
            return Ok(None);
        };
        if key.is_empty() {
            return Ok(None);
        }
        let channel = match std::env::var("ZABOR_CLIENT_CHANNEL").as_deref() {
            Ok("official") => "official",
            Ok("dev") | Err(_) => "dev",
            _ => return Err("Unsupported desktop attestation channel"),
        };
        if semver::Version::parse(version).is_err() {
            return Err("Invalid attestation version");
        }
        Ok(Some(Self {
            key: Zeroizing::new(key),
            channel,
            version: version.into(),
        }))
    }

    pub fn token(&self) -> Result<Zeroizing<String>, &'static str> {
        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).map_err(|_| "Random source unavailable")?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "Invalid system clock")?
            .as_secs();
        self.sign(timestamp, nonce)
    }

    fn sign(&self, timestamp: u64, nonce: [u8; 8]) -> Result<Zeroizing<String>, &'static str> {
        let nonce = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let payload = format!("v1:{}:{}:{timestamp}:{nonce}", self.channel, self.version);
        let mut hmac = Hmac::<Sha256>::new_from_slice(self.key.as_bytes())
            .map_err(|_| "Invalid signing configuration")?;
        hmac.update(payload.as_bytes());
        let signature =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hmac.finalize().into_bytes());
        Ok(Zeroizing::new(format!("{payload}:{signature}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_uses_the_existing_six_field_hmac_contract() {
        let signing = Attestation {
            key: Zeroizing::new("local-fixture-only".into()),
            channel: "dev",
            version: "3.8.0-rust".into(),
        };
        let token = signing.sign(1700000000, [0x12; 8]).unwrap();
        let fields: Vec<_> = token.split(':').collect();
        assert_eq!(
            &fields[..5],
            &["v1", "dev", "3.8.0-rust", "1700000000", "1212121212121212"]
        );
        let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(fields[5])
            .unwrap();
        let mut verifier = Hmac::<Sha256>::new_from_slice(b"local-fixture-only").unwrap();
        verifier.update(fields[..5].join(":").as_bytes());
        verifier.verify_slice(&signature).unwrap();
        assert_ne!(*signing.token().unwrap(), *signing.token().unwrap());
    }
}
