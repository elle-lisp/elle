//! Cryptographic primitives (SHA-256, HMAC-SHA256)
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Extract byte data from a string or bytes value.
/// Strings are treated as their UTF-8 encoding.
fn extract_byte_data(val: &Value, name: &str, pos: &str) -> Result<Vec<u8>, (SignalBits, Value)> {
    if let Some(bytes) = val.with_string(|s| s.as_bytes().to_vec()) {
        Ok(bytes)
    } else if let Some(b) = val.as_bytes() {
        Ok(b.to_vec())
    } else if let Some(blob_ref) = val.as_blob() {
        Ok(blob_ref.borrow().clone())
    } else {
        Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: {} must be string, bytes, or blob, got {}",
                    name,
                    pos,
                    val.type_name()
                ),
            ),
        ))
    }
}

/// SHA-256 hash. Accepts string, bytes, or blob. Returns bytes (32 bytes).
pub fn prim_sha256(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("crypto/sha256: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let data = match extract_byte_data(&args[0], "crypto/sha256", "argument") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let hash = Sha256::digest(&data);
    (SIG_OK, Value::bytes(hash.to_vec()))
}

/// HMAC-SHA256. Accepts (key, message), each string/bytes/blob. Returns bytes (32 bytes).
pub fn prim_hmac_sha256(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "crypto/hmac-sha256: expected 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let key = match extract_byte_data(&args[0], "crypto/hmac-sha256", "key") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let message = match extract_byte_data(&args[1], "crypto/hmac-sha256", "message") {
        Ok(d) => d,
        Err(e) => return e,
    };

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(&key).expect("HMAC accepts any key length");
    mac.update(&message);
    let result = mac.finalize().into_bytes();
    (SIG_OK, Value::bytes(result.to_vec()))
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "crypto/sha256",
        func: prim_sha256,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "SHA-256 hash. Accepts string, bytes, or blob. Returns 32 bytes.",
        params: &["data"],
        category: "crypto",
        example: "(bytes->hex (crypto/sha256 \"hello\"))",
        aliases: &["sha256"],
    },
    PrimitiveDef {
        name: "crypto/hmac-sha256",
        func: prim_hmac_sha256,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "HMAC-SHA256. Takes (key, message). Returns 32 bytes.",
        params: &["key", "message"],
        category: "crypto",
        example: "(bytes->hex (crypto/hmac-sha256 \"key\" \"message\"))",
        aliases: &["hmac-sha256"],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_empty_string() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let result = prim_sha256(&[Value::string("")]);
        assert_eq!(result.0, SIG_OK);
        let hex_result = prim_bytes_to_hex_for_test(result.1);
        assert_eq!(
            hex_result,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_hello() {
        // SHA-256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let result = prim_sha256(&[Value::string("hello")]);
        assert_eq!(result.0, SIG_OK);
        let hex_result = prim_bytes_to_hex_for_test(result.1);
        assert_eq!(
            hex_result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha256_bytes_input() {
        // SHA-256 of bytes [104, 101, 108, 108, 111] = SHA-256("hello")
        let input = Value::bytes(vec![104, 101, 108, 108, 111]);
        let result = prim_sha256(&[input]);
        assert_eq!(result.0, SIG_OK);
        let hex_result = prim_bytes_to_hex_for_test(result.1);
        assert_eq!(
            hex_result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_hmac_sha256_rfc4231_test1() {
        // RFC 4231 Test Case 1
        // Key = 0x0b repeated 20 times
        // Data = "Hi There"
        // HMAC-SHA256 = b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7
        let key = Value::bytes(vec![0x0b; 20]);
        let data = Value::string("Hi There");
        let result = prim_hmac_sha256(&[key, data]);
        assert_eq!(result.0, SIG_OK);
        let hex_result = prim_bytes_to_hex_for_test(result.1);
        assert_eq!(
            hex_result,
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn test_hmac_sha256_rfc4231_test2() {
        // RFC 4231 Test Case 2
        // Key = "Jefe"
        // Data = "what do ya want for nothing?"
        // HMAC-SHA256 = 5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843
        let key = Value::string("Jefe");
        let data = Value::string("what do ya want for nothing?");
        let result = prim_hmac_sha256(&[key, data]);
        assert_eq!(result.0, SIG_OK);
        let hex_result = prim_bytes_to_hex_for_test(result.1);
        assert_eq!(
            hex_result,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn test_hmac_sha256_chained() {
        // Simulate SigV4 key derivation: HMAC(HMAC(key, msg1), msg2)
        // This tests that bytes output feeds correctly as key input
        let key = Value::string("secret");
        let msg1 = Value::string("step1");
        let intermediate = prim_hmac_sha256(&[key, msg1]);
        assert_eq!(intermediate.0, SIG_OK);

        let msg2 = Value::string("step2");
        let final_result = prim_hmac_sha256(&[intermediate.1, msg2]);
        assert_eq!(final_result.0, SIG_OK);
        // Just verify it produces 32 bytes, not a specific value
        assert!(final_result.1.as_bytes().unwrap().len() == 32);
    }

    /// Helper: extract hex string from a bytes Value
    fn prim_bytes_to_hex_for_test(val: Value) -> String {
        val.as_bytes()
            .unwrap()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}
