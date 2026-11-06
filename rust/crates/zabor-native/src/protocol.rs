use serde_json::{json, Value};

pub const MAX_RECORD_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("hub record exceeds the size limit")]
    Oversized,
    #[error("invalid hub JSON record")]
    Invalid,
}

#[derive(Default)]
pub struct Decoder {
    pending: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Value>, ProtocolError> {
        let mut records = Vec::new();
        let mut offset = 0;
        for (index, byte) in bytes.iter().enumerate() {
            if *byte != 0x1e {
                continue;
            }
            let part = &bytes[offset..index];
            if self.pending.len().saturating_add(part.len()) > MAX_RECORD_BYTES {
                self.pending.clear();
                return Err(ProtocolError::Oversized);
            }
            self.pending.extend_from_slice(part);
            let value =
                serde_json::from_slice::<Value>(&self.pending).map_err(|_| ProtocolError::Invalid);
            self.pending.clear();
            let value = value?;
            if !value.is_object() {
                return Err(ProtocolError::Invalid);
            }
            records.push(value);
            offset = index + 1;
        }
        if self.pending.len().saturating_add(bytes.len() - offset) > MAX_RECORD_BYTES {
            self.pending.clear();
            return Err(ProtocolError::Oversized);
        }
        self.pending.extend_from_slice(&bytes[offset..]);
        Ok(records)
    }
}

pub fn encode(value: &Value) -> Result<String, ProtocolError> {
    let mut encoded = serde_json::to_string(value).map_err(|_| ProtocolError::Invalid)?;
    if encoded.len() > MAX_RECORD_BYTES {
        return Err(ProtocolError::Oversized);
    }
    encoded.push('\u{1e}');
    Ok(encoded)
}

pub fn invocation(id: u64, target: &str, arguments: Vec<Value>) -> Result<String, ProtocolError> {
    encode(&json!({"type":1,"invocationId":id.to_string(),"target":target,"arguments":arguments}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_unicode_handshake_and_coalesced_records_remain_ordered() {
        let mut decoder = Decoder::default();
        let wire = "{}\u{1e}{\"type\":1,\"target\":\"UserUpdated\",\"arguments\":[\"привет\"]}\u{1e}{\"type\":6}\u{1e}";
        let mut records = Vec::new();
        for byte in wire.as_bytes() {
            records.extend(decoder.push(&[*byte]).unwrap());
        }
        assert_eq!(records.len(), 3);
        assert_eq!(records[0], json!({}));
        assert_eq!(records[1]["arguments"][0], "привет");
        assert_eq!(records[2]["type"], 6);
    }

    #[test]
    fn rejects_oversized_unterminated_and_nonobject_records() {
        assert_eq!(
            Decoder::default().push(&vec![b'x'; MAX_RECORD_BYTES + 1]),
            Err(ProtocolError::Oversized)
        );
        assert_eq!(
            Decoder::default().push(b"[]\x1e"),
            Err(ProtocolError::Invalid)
        );
        assert_eq!(
            Decoder::default().push(b"not-json\x1e"),
            Err(ProtocolError::Invalid)
        );
    }

    #[test]
    fn arguments_are_encoded_as_data_not_hub_records() {
        let wire = invocation(9, "Login", vec![json!("name"), json!("x\u{1e}\"\\")]).unwrap();
        let records = Decoder::default().push(wire.as_bytes()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["invocationId"], "9");
        assert_eq!(records[0]["arguments"][1], "x\u{1e}\"\\");
    }
}
