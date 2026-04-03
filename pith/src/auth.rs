/// Password hashing and verification for identity authentication.
///
/// Passwords are hashed with argon2id and stored as zero-byte files
/// whose NAME encodes the PHC-format hash. This preserves the 0-bytes
/// philosophy: no file ever contains data.
///
/// Filesystem convention:
///   `hard/identities/{id}/-secret/{encoded_hash}`
///
/// The PHC format `$argon2id$v=19$m=19456,t=2,p=1$salt$digest` is made
/// filename-safe by replacing `$` with `.` (reversible, since `.` never
/// appears inside PHC field values).

use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rand::rngs::OsRng;

use crate::error::{PithError, Result};

// --- Hash / Verify ---

/// Hash a password using argon2id, returning the PHC-format string.
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default(); // argon2id v0x13
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| PithError::Auth {
            reason: format!("password hash failed: {}", e),
        })?;
    Ok(hash.to_string())
}

/// Verify a password against a PHC-format hash string.
pub fn verify_password(password: &str, phc_hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(phc_hash).map_err(|e| PithError::Auth {
        reason: format!("invalid hash format: {}", e),
    })?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

// --- Filename encoding ---

/// Encode a PHC-format hash into a filesystem-safe filename.
///
/// Replaces `$` with `.` — reversible because `.` never appears
/// inside PHC field values (salt and digest use bcrypt-Base64: `./A-Za-z0-9`
/// but the `.` only appears at the start after our replacement of leading `$`).
pub fn hash_to_filename(phc_hash: &str) -> String {
    phc_hash.replace('$', ".")
}

/// Decode a filesystem filename back to a PHC-format hash.
pub fn filename_to_hash(filename: &str) -> String {
    filename.replace('.', "$")
}

// --- Interactive prompt ---

/// Prompt the user for a password with confirmation and validation.
///
/// Uses rpassword to hide typed characters. Requires a TTY.
pub fn prompt_password_interactive(context: &str) -> Result<String> {
    println!("\n=== Pith — {} ===", context);
    println!("Please set a password for the admin identity (001).\n");

    eprint!("  Enter password: ");
    let password =
        rpassword::read_password().map_err(|e| PithError::Auth {
            reason: format!("failed to read password: {}", e),
        })?;

    if password.is_empty() {
        return Err(PithError::Auth {
            reason: "password cannot be empty".to_string(),
        });
    }

    if password.len() < 8 {
        return Err(PithError::Auth {
            reason: "password must be at least 8 characters".to_string(),
        });
    }

    eprint!("  Confirm password: ");
    let confirm =
        rpassword::read_password().map_err(|e| PithError::Auth {
            reason: format!("failed to read confirmation: {}", e),
        })?;

    if password != confirm {
        return Err(PithError::Auth {
            reason: "passwords do not match".to_string(),
        });
    }

    Ok(password)
}

/// Check if stdin is a TTY (interactive terminal).
pub fn is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify() {
        let password = "test_password_123";
        let hash = hash_password(password).unwrap();

        // PHC format starts with $argon2id$
        assert!(hash.starts_with("$argon2id$"));

        // Correct password verifies.
        assert!(verify_password(password, &hash).unwrap());

        // Wrong password fails.
        assert!(!verify_password("wrong_password", &hash).unwrap());
    }

    #[test]
    fn test_hash_to_filename_roundtrip() {
        let password = "roundtrip_test";
        let hash = hash_password(password).unwrap();

        let filename = hash_to_filename(&hash);
        // Filename should not contain $.
        assert!(!filename.contains('$'));
        // Filename should start with .argon2id.
        assert!(filename.starts_with(".argon2id."));

        // Round-trip back to hash.
        let recovered = filename_to_hash(&filename);
        assert_eq!(recovered, hash);

        // Verify still works with recovered hash.
        assert!(verify_password(password, &recovered).unwrap());
    }

    #[test]
    fn test_filename_encoding_deterministic() {
        let phc = "$argon2id$v=19$m=19456,t=2,p=1$abcSALT$xyzDIGEST";
        let filename = hash_to_filename(phc);
        assert_eq!(filename, ".argon2id.v=19.m=19456,t=2,p=1.abcSALT.xyzDIGEST");

        let back = filename_to_hash(&filename);
        assert_eq!(back, phc);
    }

    #[test]
    fn test_different_passwords_different_hashes() {
        let h1 = hash_password("password_one").unwrap();
        let h2 = hash_password("password_two").unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_verify_invalid_hash_format() {
        let result = verify_password("anything", "not-a-valid-hash");
        assert!(result.is_err());
    }
}
