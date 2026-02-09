pub mod admin;
pub mod auth;
pub mod db;
pub mod relay;
pub mod security;
pub mod signal;
pub mod web;

pub use admin::{AdminOverview, BanUserRequest, RevokeSessionRequest};
pub use auth::{AuthResponse, ErrorResponse, LoginRequest, RegisterRequest};
pub use db::{Session, User};
pub use security::hash_token;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::LogoutResponse;

    #[test]
    fn test_hash_token_consistency() {
        let token = "test_token_12345";
        let hash1 = hash_token(token);
        let hash2 = hash_token(token);
        assert_eq!(hash1, hash2, "Hash should be consistent for same token");
    }

    #[test]
    fn test_hash_token_different_inputs() {
        let token1 = "token_one";
        let token2 = "token_two";
        let hash1 = hash_token(token1);
        let hash2 = hash_token(token2);
        assert_ne!(hash1, hash2, "Different tokens should produce different hashes");
    }

    #[test]
    fn test_hash_token_not_equal_to_input() {
        let token = "plaintext_token";
        let hash = hash_token(token);
        assert_ne!(hash, token, "Hash should not equal original token");
        assert_ne!(hash.len(), token.len(), "Hash length should differ from token");
    }

    #[test]
    fn test_hash_token_empty_string() {
        let token = "";
        let hash = hash_token(token);
        assert!(!hash.is_empty(), "Hash of empty string should not be empty");
    }

    #[test]
    fn test_hash_token_long_string() {
        let token = "a".repeat(1000);
        let hash = hash_token(&token);
        assert!(!hash.is_empty(), "Hash of long string should not be empty");
        // SHA-256 produces 64 hex characters (256 bits)
        assert_eq!(hash.len(), 64, "SHA-256 hash should be 64 hex chars");
    }

    #[test]
    fn test_hash_token_special_characters() {
        let token = "!@#$%^&*()_+-=[]{}|;':\",./<>?";
        let hash = hash_token(token);
        assert!(!hash.is_empty(), "Hash of special chars should not be empty");
    }

    #[test]
    fn test_hash_token_unicode() {
        let token = "token_with_émoji_🎉_日本語";
        let hash = hash_token(token);
        assert!(!hash.is_empty(), "Hash of unicode should not be empty");
    }

    #[test]
    fn test_register_request_validation() {
        let req = RegisterRequest {
            email: "user@example.com".to_string(),
            password: "secure_password_123".to_string(),
            display_name: "John Doe".to_string(),
            username: "johndoe".to_string(),
            public_key: "abcd1234".to_string(),
        };

        assert!(!req.email.is_empty());
        assert!(!req.password.is_empty());
        assert!(!req.username.is_empty());
        assert!(req.email.contains("@"));
    }

    #[test]
    fn test_login_request_with_totp() {
        let req = LoginRequest {
            email: "user@example.com".to_string(),
            password: "password123".to_string(),
            totp_code: Some("123456".to_string()),
        };

        assert!(req.totp_code.is_some());
        assert_eq!(req.totp_code.unwrap(), "123456");
    }

    #[test]
    fn test_login_request_without_totp() {
        let req = LoginRequest {
            email: "user@example.com".to_string(),
            password: "password123".to_string(),
            totp_code: None,
        };

        assert!(req.totp_code.is_none());
    }

    #[test]
    fn test_error_response_creation() {
        let error = ErrorResponse {
            error: "Invalid credentials".to_string(),
        };

        assert_eq!(error.error, "Invalid credentials");
    }

    #[test]
    fn test_logout_response() {
        let resp = LogoutResponse { revoked: true };
        assert!(resp.revoked);

        let resp2 = LogoutResponse { revoked: false };
        assert!(!resp2.revoked);
    }

    #[test]
    fn test_ban_user_request() {
        let ban_req = BanUserRequest {
            user_id: "user-123".to_string(),
            reason: "Violating ToS".to_string(),
            duration_hours: Some(24),
        };

        assert_eq!(ban_req.user_id, "user-123");
        assert_eq!(ban_req.reason, "Violating ToS");
        assert_eq!(ban_req.duration_hours, Some(24));
    }

    #[test]
    fn test_ban_user_permanent() {
        let ban_req = BanUserRequest {
            user_id: "user-456".to_string(),
            reason: "Permanent ban".to_string(),
            duration_hours: None,
        };

        assert!(ban_req.duration_hours.is_none());
    }

}
