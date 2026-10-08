//! Base64, for the one place it is needed.
//!
//! Ollama's `images` field takes base64 and nothing else, and a base64 encoder is
//! thirty lines. A dependency would be a licence to review and a version to
//! track for something with one caller; the project's own ZIP writer has the
//! same argument, and the same answer.

/// Encodes bytes as standard base64 with padding.
#[must_use]
pub fn encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk.first().copied().unwrap_or(0);
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let triple = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(digit(triple >> 18));
        out.push(digit(triple >> 12));
        out.push(if chunk.len() > 1 {
            digit(triple >> 6)
        } else {
            '='
        });
        out.push(if chunk.len() > 2 { digit(triple) } else { '=' });
    }
    out
}

/// The base64 digit for the low six bits of `value`.
fn digit(value: u32) -> char {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // Six bits always land in the 64-entry alphabet; the fallback is never taken.
    ALPHABET
        .get((value & 0x3F) as usize)
        .map_or('=', |&byte| char::from(byte))
}

#[cfg(test)]
mod tests {
    use super::encode;

    /// The RFC 4648 test vectors: the encoding is only correct if it agrees with
    /// every other implementation, so it is checked against the standard's own
    /// table rather than against itself.
    #[test]
    fn matches_the_rfc_4648_vectors() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_full_alphabet_is_reachable() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        let encoded = encode(&bytes);
        assert_eq!(encoded.len(), 344);
        assert!(
            encoded.contains('+'),
            "the two pad-free symbols must appear"
        );
        assert!(encoded.contains('/'));
    }

    #[test]
    fn every_length_produces_whole_quads() {
        for length in 0..12 {
            assert_eq!(encode(&vec![b'x'; length]).len() % 4, 0, "length {length}");
        }
    }
}
