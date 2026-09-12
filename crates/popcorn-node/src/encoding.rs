//! Hex and base64 helpers.
//!
//! Written out rather than pulled in: §2 fixes the dependency list, and the API layer
//! needing to print bytes is not a reason to widen it.

/// Lowercase hex.
pub fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    out
}

/// Parse lowercase or uppercase hex. Rejects odd lengths and non-hex bytes.
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.len() % 2 != 0 {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

/// Parse hex into exactly 32 bytes.
pub fn hex32(text: &str) -> Option<[u8; 32]> {
    from_hex(text)?.try_into().ok()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn to_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[(triple >> 18) as usize & 0x3f] as char);
        out.push(B64[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            B64[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[triple as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

/// Standard base64, padding optional. Returns `None` on any invalid input rather than
/// guessing: these bytes arrive from the wire.
pub fn from_base64(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    let mut accumulator: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for byte in text.bytes() {
        if byte == b'=' {
            break;
        }
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'\n' | b'\r' => continue,
            _ => return None,
        } as u32;
        accumulator = (accumulator << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((accumulator >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_every_length_up_to_a_block() {
        for length in 0..=64usize {
            let data: Vec<u8> = (0..length).map(|i| (i * 7 + 3) as u8).collect();
            let encoded = to_base64(&data);
            assert_eq!(from_base64(&encoded).unwrap(), data, "length {length}");
        }
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(to_base64(b""), "");
        assert_eq!(to_base64(b"f"), "Zg==");
        assert_eq!(to_base64(b"fo"), "Zm8=");
        assert_eq!(to_base64(b"foo"), "Zm9v");
        assert_eq!(to_base64(b"foob"), "Zm9vYg==");
        assert_eq!(to_base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(to_base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn invalid_base64_is_refused() {
        assert!(from_base64("!!!!").is_none());
        assert!(from_base64("Zm9v*g==").is_none());
    }

    #[test]
    fn hex_round_trips() {
        let data: Vec<u8> = (0..=255u8).collect();
        assert_eq!(from_hex(&to_hex(&data)).unwrap(), data);
        assert!(from_hex("abc").is_none());
        assert!(from_hex("zz").is_none());
    }
}
