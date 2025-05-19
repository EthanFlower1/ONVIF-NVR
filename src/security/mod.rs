use crate::db::models::user_models::{AuthToken, UserRole};
use crate::error::Error;
use crate::{config::SecurityConfig, db::models::user_models::User};
use anyhow::Result;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, TokenData, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod auth;
pub mod password;

/// JWT claims structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// User name
    pub name: String,
    /// User role
    pub role: String,
    /// Expiration time (Unix timestamp)
    pub exp: usize,
    /// Issued at (Unix timestamp)
    pub iat: usize,
}

impl Claims {
    /// Get the user ID from the claims
    pub fn user_id(&self) -> Result<uuid::Uuid, uuid::Error> {
        uuid::Uuid::parse_str(&self.sub)
    }
}

/// Security service for handling authentication and authorization
pub struct SecurityService {
    config: SecurityConfig,
}

impl SecurityService {
    /// Create a new security service
    pub fn new(config: SecurityConfig) -> Self {
        Self { config }
    }

    /// Generate a JWT token for a user
    pub fn generate_token(&self, user: &User) -> Result<AuthToken> {
        // Current time
        let now = Utc::now();

        // Expiration time
        let expiration = now + Duration::minutes(self.config.jwt_expiration_minutes as i64);

        // Create claims
        let claims = Claims {
            sub: user.id.to_string(),
            name: user.username.clone(),
            role: format!("{:?}", user.role).to_lowercase(),
            exp: expiration.timestamp() as usize,
            iat: now.timestamp() as usize,
        };

        // Encode token
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.config.jwt_secret.as_bytes()),
        )
        .map_err(|e| Error::Authentication(format!("Failed to generate JWT token: {}", e)))?;

        // Return auth token
        Ok(AuthToken {
            access_token: token,
            token_type: "Bearer".to_string(),
            expires_in: self.config.jwt_expiration_minutes * 60, // Convert to seconds
        })
    }

    /// Validate and decode a JWT token
    pub fn validate_token(&self, token: &str) -> Result<TokenData<Claims>> {
        // Decode and validate token
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.config.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|e| Error::Authentication(format!("Invalid token: {}", e)))?;

        Ok(token_data)
    }

    /// Extract user ID from validated token data
    pub fn get_user_id_from_token(&self, token_data: &TokenData<Claims>) -> Result<Uuid> {
        // Parse user ID from subject claim
        let user_id = Uuid::parse_str(&token_data.claims.sub)
            .map_err(|e| Error::Authentication(format!("Invalid user ID in token: {}", e)))?;

        Ok(user_id)
    }

    /// Check if user has specified role
    pub fn has_role(&self, token_data: &TokenData<Claims>, required_role: UserRole) -> bool {
        // Parse role from token
        let user_role = match token_data.claims.role.as_str() {
            "admin" => UserRole::Admin,
            "operator" => UserRole::Operator,
            "viewer" => UserRole::Viewer,
            _ => return false,
        };

        // Check role hierarchy
        match required_role {
            UserRole::Admin => user_role == UserRole::Admin,
            UserRole::Operator => user_role == UserRole::Admin || user_role == UserRole::Operator,
            UserRole::Viewer => true, // All roles can do viewer actions
        }
    }
}

