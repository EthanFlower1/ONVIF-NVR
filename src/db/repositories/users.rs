use crate::{
    db::models::user_models::{User, UserRole},
    error::Error,
};
use anyhow::Result;
use chrono::Utc;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// Users repository for handling user operations
#[derive(Clone)]
pub struct UsersRepository {
    pool: Arc<PgPool>,
}

impl UsersRepository {
    /// Create a new users repository
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Create a new user
    pub async fn create(&self, user: &User) -> Result<User> {
        info!("Creating new user: {}", user.username);

        let result = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (id, username, email, password_hash, role, created_at, updated_at, active)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, username, email, password_hash, role, created_at, updated_at, last_login, active
            "#
        )
        .bind(user.id)
        .bind(&user.username)
        .bind(&user.email)
        .bind(&user.password_hash)
        .bind(&user.role)
        .bind(user.created_at)
        .bind(user.updated_at)
        .bind(user.active)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create user: {}", e)))?;

        Ok(result)
    }

    /// Get user by ID
    pub async fn get_by_id(&self, id: &Uuid) -> Result<Option<User>> {
        let result = sqlx::query_as::<_, User>(
            r#"
            SELECT id, username, email, password_hash, role, created_at, updated_at, last_login, active
            FROM users
            WHERE id = $1
            "#
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get user by ID: {}", e)))?;

        Ok(result)
    }

    /// Get user by username
    pub async fn get_by_username(&self, username: &str) -> Result<Option<User>> {
        let result = sqlx::query_as::<_, User>(
            r#"
            SELECT id, username, email, password_hash, role, created_at, updated_at, last_login, active
            FROM users
            WHERE username = $1
            "#
        )
        .bind(username)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get user by username: {}", e)))?;

        Ok(result)
    }

    /// Get user by email
    pub async fn get_by_email(&self, email: &str) -> Result<Option<User>> {
        let result = sqlx::query_as::<_, User>(
            r#"
            SELECT id, username, email, password_hash, role, created_at, updated_at, last_login, active
            FROM users
            WHERE email = $1
            "#
        )
        .bind(email)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get user by email: {}", e)))?;

        Ok(result)
    }

    /// Update user
    pub async fn update(&self, user: &User) -> Result<User> {
        let result = sqlx::query_as::<_, User>(
            r#"
            UPDATE users
            SET username = $1, email = $2, password_hash = $3, role = $4, updated_at = $5, active = $6
            WHERE id = $7
            RETURNING id, username, email, password_hash, role, created_at, updated_at, last_login, active
            "#
        )
        .bind(&user.username)
        .bind(&user.email)
        .bind(&user.password_hash)
        .bind(&user.role)
        .bind(Utc::now())
        .bind(user.active)
        .bind(user.id)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update user: {}", e)))?;

        Ok(result)
    }

    /// Delete user
    pub async fn delete(&self, id: &Uuid) -> Result<bool> {
        let result = sqlx::query(
            r#"
            DELETE FROM users
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to delete user: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all users
    pub async fn get_all(&self) -> Result<Vec<User>> {
        let result = sqlx::query_as::<_, User>(
            r#"
            SELECT id, username, email, password_hash, role, created_at, updated_at, last_login, active
            FROM users
            ORDER BY username
            "#
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get all users: {}", e)))?;

        Ok(result)
    }

    /// Update last login time
    pub async fn update_last_login(&self, id: &Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE users
            SET last_login = $1
            WHERE id = $2
            "#,
        )
        .bind(Utc::now())
        .bind(id)
        .execute(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update last login: {}", e)))?;

        Ok(())
    }

    /// Get users by role
    pub async fn get_by_role(&self, role: &UserRole) -> Result<Vec<User>> {
        let result = sqlx::query_as::<_, User>(
            r#"
            SELECT id, username, email, password_hash, role, created_at, updated_at, last_login, active
            FROM users
            WHERE role = $1
            ORDER BY username
            "#
        )
        .bind(role)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get users by role: {}", e)))?;

        Ok(result)
    }
}

