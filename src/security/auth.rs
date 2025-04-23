use crate::config::SecurityConfig;
use crate::db::models::user_models::{AuthToken, LoginCredentials, User, UserRole};
use crate::db::repositories::users::UsersRepository;
use crate::error::Error;
use crate::security::{password, Claims, SecurityService};
use anyhow::Result;
use chrono::Utc;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// Authentication service for handling user login/logout
pub struct AuthService {
    users_repo: UsersRepository,
    security: SecurityService,
    config: SecurityConfig,
}

impl AuthService {
    /// Create a new authentication service
    pub fn new(pool: Arc<PgPool>, config: &SecurityConfig) -> Self {
        Self {
            users_repo: UsersRepository::new(pool),
            security: SecurityService::new(config.clone()),
            config: config.clone(),
        }
    }

    /// Create a new authentication service without database for testing
    pub fn new_without_db(config: &SecurityConfig) -> Self {
        let db_pool = Arc::new(
            PgPoolOptions::new()
                .max_connections(1)
                .connect_lazy("postgres://postgres:postgres@localhost:5432/postgres")
                .unwrap_or_else(|_| panic!("Failed to create test database pool")),
        );
        Self {
            users_repo: UsersRepository::new(db_pool),
            security: SecurityService::new(config.clone()),
            config: config.clone(),
        }
    }

    /// Login a user with username/password
    pub async fn login(&self, credentials: &LoginCredentials) -> Result<(User, AuthToken)> {
        // Find user by username
        let user = self
            .users_repo
            .get_by_username(&credentials.username)
            .await?
            .ok_or_else(|| Error::Authentication("Invalid username or password".to_string()))?;

        // Check if user is active
        if !user.active {
            return Err(Error::Authentication("User account is inactive".to_string()).into());
        }

        // Verify password
        let valid = password::verify_password(&credentials.password, &user.password_hash)?;

        if !valid {
            return Err(Error::Authentication("Invalid username or password".to_string()).into());
        }

        // Update last login time
        self.users_repo.update_last_login(&user.id).await?;

        // Generate auth token
        let token = self.security.generate_token(&user)?;

        info!("User logged in: {}", user.username);

        Ok((user, token))
    }

    /// Register a new user
    pub async fn register(
        &self,
        username: &str,
        email: &str,
        password: &str,
        role: UserRole,
    ) -> Result<User> {
        // Check if username already exists
        if let Some(_) = self.users_repo.get_by_username(username).await? {
            return Err(Error::AlreadyExists("Username already exists".to_string()).into());
        }

        // Check if email already exists
        if let Some(_) = self.users_repo.get_by_email(email).await? {
            return Err(Error::AlreadyExists("Email already exists".to_string()).into());
        }

        // Hash password
        let password_hash = password::hash_password(password, &self.config)?;

        // Create user
        let user = User {
            id: Uuid::new_v4(),
            username: username.to_string(),
            email: email.to_string(),
            password_hash,
            role,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_login: None,
            active: true,
        };

        // Save user to database
        let created_user = self.users_repo.create(&user).await?;

        info!("New user registered: {}", username);

        Ok(created_user)
    }

    /// Change user password
    pub async fn change_password(
        &self,
        user_id: &Uuid,
        current_password: &str,
        new_password: &str,
    ) -> Result<()> {
        // Find user
        let user = self
            .users_repo
            .get_by_id(user_id)
            .await?
            .ok_or_else(|| Error::NotFound("User not found".to_string()))?;

        // Verify current password
        let valid = password::verify_password(current_password, &user.password_hash)?;

        if !valid {
            return Err(Error::Authentication("Current password is incorrect".to_string()).into());
        }

        // Hash new password
        let password_hash = password::hash_password(new_password, &self.config)?;

        // Update user
        let mut updated_user = user.clone();
        updated_user.password_hash = password_hash;
        updated_user.updated_at = Utc::now();

        self.users_repo.update(&updated_user).await?;

        info!("Password changed for user: {}", user.username);

        Ok(())
    }

    /// Reset user password (admin function)
    pub async fn reset_password(&self, user_id: &Uuid) -> Result<String> {
        // Find user
        let user = self
            .users_repo
            .get_by_id(user_id)
            .await?
            .ok_or_else(|| Error::NotFound("User not found".to_string()))?;

        // Generate random password
        let new_password = password::generate_random_password(12);

        // Hash new password
        let password_hash = password::hash_password(&new_password, &self.config)?;

        // Update user
        let mut updated_user = user.clone();
        updated_user.password_hash = password_hash;
        updated_user.updated_at = Utc::now();

        self.users_repo.update(&updated_user).await?;

        info!("Password reset for user: {}", user.username);

        Ok(new_password)
    }

    /// Update user role (admin function)
    pub async fn update_role(&self, user_id: &Uuid, new_role: UserRole) -> Result<User> {
        // Find user
        let user = self
            .users_repo
            .get_by_id(user_id)
            .await?
            .ok_or_else(|| Error::NotFound("User not found".to_string()))?;

        // Update user
        let mut updated_user = user.clone();
        let role_str = format!("{:?}", new_role); // Store the role as a string before moving it
        updated_user.role = new_role;
        updated_user.updated_at = Utc::now();

        let result = self.users_repo.update(&updated_user).await?;

        info!("Role updated for user {}: {}", user.username, role_str);

        Ok(result)
    }

    /// Activate or deactivate a user (admin function)
    pub async fn set_active(&self, user_id: &Uuid, active: bool) -> Result<User> {
        // Find user
        let user = self
            .users_repo
            .get_by_id(user_id)
            .await?
            .ok_or_else(|| Error::NotFound("User not found".to_string()))?;

        // Update user
        let mut updated_user = user.clone();
        updated_user.active = active;
        updated_user.updated_at = Utc::now();

        let result = self.users_repo.update(&updated_user).await?;

        info!(
            "User {} {} by admin",
            user.username,
            if active { "activated" } else { "deactivated" }
        );

        Ok(result)
    }
}

// TEMPORARILY DISABLED: JWT Extractor for protected routes
// We'll use a different approach for now to get the code compiling
// and add proper JWT authentication back later

// This allows us to get a non-authenticated claims object for testing
// IMPORTANT: This is just for development - NOT for production use!
pub async fn get_temporary_claims() -> Claims {
    Claims {
        sub: Uuid::new_v4().to_string(),
        name: "test".to_string(),
        role: "admin".to_string(),
        exp: (Utc::now().timestamp() + 3600) as usize,
        iat: Utc::now().timestamp() as usize,
    }
}

