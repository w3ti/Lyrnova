use serde_json::Value;

use super::LanguageError;

pub(super) const MAX_FRAME: usize = 4 * 1024 * 1024;
const MAX_HEADER: usize = 8192;

#[derive(Default)]
pub(super) struct Decoder {
    bytes: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), LanguageError> {
        if self.bytes.len() + bytes.len() > MAX_FRAME + MAX_HEADER {
            return Err(LanguageError::ProtocolViolation);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    pub fn pending(&self) -> bool {
        !self.bytes.is_empty()
    }

    pub fn next(&mut self) -> Result<Option<Value>, LanguageError> {
        let Some(end) = self.bytes.windows(4).position(|s| s == b"\r\n\r\n") else {
            if self.bytes.len() > MAX_HEADER {
                return Err(LanguageError::ProtocolViolation);
            }
            return Ok(None);
        };
        if end > MAX_HEADER {
            return Err(LanguageError::ProtocolViolation);
        }
        let header = std::str::from_utf8(&self.bytes[..end])
            .map_err(|_| LanguageError::ProtocolViolation)?;
        let mut length = None;
        for line in header.split("\r\n") {
            let (key, value) = line
                .split_once(':')
                .ok_or(LanguageError::ProtocolViolation)?;
            if key.eq_ignore_ascii_case("content-length") {
                if length.is_some() || !value.trim().bytes().all(|b| b.is_ascii_digit()) {
                    return Err(LanguageError::ProtocolViolation);
                }
                length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|_| LanguageError::ProtocolViolation)?,
                );
            }
        }
        let length = length
            .filter(|n| *n > 0 && *n <= MAX_FRAME)
            .ok_or(LanguageError::ProtocolViolation)?;
        let total = end + 4 + length;
        if self.bytes.len() < total {
            return Ok(None);
        }
        let value: Value = serde_json::from_slice(&self.bytes[end + 4..total])
            .map_err(|_| LanguageError::ProtocolViolation)?;
        if !value.is_object() || value["jsonrpc"] != "2.0" {
            return Err(LanguageError::ProtocolViolation);
        }
        self.bytes.drain(..total);
        Ok(Some(value))
    }
}

pub(super) fn frame(value: &Value) -> Result<Vec<u8>, LanguageError> {
    let body = serde_json::to_vec(value).map_err(|_| LanguageError::ProtocolViolation)?;
    if body.len() > MAX_FRAME {
        return Err(LanguageError::TooLarge);
    }
    let mut bytes = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    bytes.extend(body);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn fragmented_utf8_and_multiple_frames() {
        let value = json!({"jsonrpc":"2.0", "method":"notice", "params":"ação 🦀"});
        let mut decoder = Decoder::default();
        let bytes = frame(&value).unwrap();
        for byte in &bytes[..bytes.len() - 1] {
            decoder.push(&[*byte]).unwrap();
            assert!(decoder.next().unwrap().is_none());
        }
        decoder.push(&bytes[bytes.len() - 1..]).unwrap();
        decoder.push(&bytes).unwrap();
        assert_eq!(decoder.next().unwrap(), Some(value.clone()));
        assert_eq!(decoder.next().unwrap(), Some(value));
        assert!(!decoder.pending());
    }
    #[test]
    fn refuses_ambiguous_lengths_and_unbounded_headers() {
        for header in [
            "Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            "Content-Length: -1\r\n\r\n",
            "Content-Length: 999999999\r\n\r\n",
            "Content-Length: 2\r\n\r\n{}",
        ] {
            let mut decoder = Decoder::default();
            decoder.push(header.as_bytes()).unwrap();
            assert_eq!(decoder.next(), Err(LanguageError::ProtocolViolation));
        }
        let mut decoder = Decoder::default();
        decoder.push(&vec![b'x'; MAX_HEADER + 1]).unwrap();
        assert_eq!(decoder.next(), Err(LanguageError::ProtocolViolation));
    }
}
