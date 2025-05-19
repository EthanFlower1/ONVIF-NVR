use crate::config::SecurityConfig;
use crate::error::Error;
use anyhow::Result;
use bcrypt::{hash, verify};

/// Hash a password with bcrypt
pub fn hash_password(password: &str, config: &SecurityConfig) -> Result<String> {
    // Use the cost from config or default
    let cost = config.password_hash_cost;
    
    // Hash the password
    let hashed = hash(password, cost)
        .map_err(|e| Error::Authentication(format!("Failed to hash password: {}", e)))?;
    
    Ok(hashed)
}

/// Verify a password against a hash
pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
    let result = verify(password, hash)
        .map_err(|e| Error::Authentication(format!("Failed to verify password: {}", e)))?;
    
    Ok(result)
}

/// Generate a random password
pub fn generate_random_password(length: usize) -> String {
    use rand::{Rng, thread_rng};
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()";
    
    let mut rng = thread_rng();
    let password: String = (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();
    
    password
}