/// Security audit logging for authentication and authorization events
use std::net::IpAddr;
use tracing::{error, info, warn};

/// Security and operational event types for audit logging
///
/// This enum includes both security events (authentication, authorization) and
/// operational events (infrastructure failures) that have security implications.
#[derive(Debug, Clone, Copy)]
pub enum SecurityEventType {
    /// Successful login
    LoginSuccess,
    /// Failed login attempt
    LoginFailure,
    /// Account registration
    Registration,
    /// 2FA setup initiated
    TotpSetup,
    /// 2FA enabled on account
    TotpEnabled,
    /// Session logout
    Logout,
    /// Rate limit exceeded
    RateLimitExceeded,
    /// Account suspension/ban
    AccountSuspended,
    /// Invalid/malformed request
    ValidationError,
    /// Database error during auth operation
    /// Note: While primarily an infrastructure event, database errors during authentication
    /// can indicate security issues (SQL injection attempts, DoS) or availability problems
    /// that affect security posture, so they are logged in the security audit trail.
    DatabaseError,
}

impl SecurityEventType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::LoginSuccess => "LOGIN_SUCCESS",
            Self::LoginFailure => "LOGIN_FAILURE",
            Self::Registration => "REGISTRATION",
            Self::TotpSetup => "TOTP_SETUP",
            Self::TotpEnabled => "TOTP_ENABLED",
            Self::Logout => "LOGOUT",
            Self::RateLimitExceeded => "RATE_LIMIT_EXCEEDED",
            Self::AccountSuspended => "ACCOUNT_SUSPENDED",
            Self::ValidationError => "VALIDATION_ERROR",
            Self::DatabaseError => "DATABASE_ERROR",
        }
    }
}

/// Reason codes for authentication failures
#[derive(Debug, Clone, Copy)]
pub enum FailureReason {
    /// User account not found
    UserNotFound,
    /// Invalid password provided
    InvalidPassword,
    /// 2FA code required but not provided
    TotpRequired,
    /// Invalid 2FA code provided
    InvalidTotp,
    /// Account is banned/suspended
    AccountBanned,
    /// Rate limit exceeded for IP or user
    RateLimited,
    /// Invalid input format
    InvalidInput,
    /// Database or internal error
    InternalError,
}

impl FailureReason {
    fn as_str(&self) -> &'static str {
        match self {
            Self::UserNotFound => "USER_NOT_FOUND",
            Self::InvalidPassword => "INVALID_PASSWORD",
            Self::TotpRequired => "TOTP_REQUIRED",
            Self::InvalidTotp => "INVALID_TOTP",
            Self::AccountBanned => "ACCOUNT_BANNED",
            Self::RateLimited => "RATE_LIMITED",
            Self::InvalidInput => "INVALID_INPUT",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }
}

/// Log a security event for audit purposes
///
/// This creates structured log entries that can be:
/// - Aggregated for security monitoring
/// - Analyzed for anomaly detection
/// - Used for compliance audit trails
pub fn log_security_event(
    event_type: SecurityEventType,
    client_ip: Option<IpAddr>,
    user_id: Option<&str>,
    email: Option<&str>,
    reason: Option<FailureReason>,
    additional_context: Option<&str>,
) {
    let event_str = event_type.as_str();
    let reason_str = reason.map(|r| r.as_str());

    match event_type {
        SecurityEventType::LoginSuccess => {
            info!(
                event = event_str,
                client_ip = ?client_ip,
                user_id = user_id,
                email = email,
                "Authentication successful"
            );
        }
        SecurityEventType::LoginFailure => {
            warn!(
                event = event_str,
                client_ip = ?client_ip,
                user_id = user_id,
                email = email,
                reason = reason_str,
                context = additional_context,
                "Authentication failed"
            );
        }
        SecurityEventType::Registration => {
            info!(
                event = event_str,
                client_ip = ?client_ip,
                user_id = user_id,
                email = email,
                "User registration"
            );
        }
        SecurityEventType::TotpSetup | SecurityEventType::TotpEnabled => {
            info!(
                event = event_str,
                client_ip = ?client_ip,
                user_id = user_id,
                "2FA configuration changed"
            );
        }
        SecurityEventType::Logout => {
            info!(
                event = event_str,
                client_ip = ?client_ip,
                user_id = user_id,
                "Session logout"
            );
        }
        SecurityEventType::RateLimitExceeded => {
            warn!(
                event = event_str,
                client_ip = ?client_ip,
                user_id = user_id,
                email = email,
                "Rate limit exceeded"
            );
        }
        SecurityEventType::AccountSuspended => {
            warn!(
                event = event_str,
                client_ip = ?client_ip,
                user_id = user_id,
                reason = additional_context,
                "Account suspended access attempt"
            );
        }
        SecurityEventType::ValidationError => {
            warn!(
                event = event_str,
                client_ip = ?client_ip,
                context = additional_context,
                "Invalid request format"
            );
        }
        SecurityEventType::DatabaseError => {
            error!(
                event = event_str,
                context = additional_context,
                "Database error during authentication"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn test_event_type_strings() {
        assert_eq!(SecurityEventType::LoginSuccess.as_str(), "LOGIN_SUCCESS");
        assert_eq!(SecurityEventType::LoginFailure.as_str(), "LOGIN_FAILURE");
        assert_eq!(
            SecurityEventType::RateLimitExceeded.as_str(),
            "RATE_LIMIT_EXCEEDED"
        );
    }

    #[test]
    fn test_failure_reason_strings() {
        assert_eq!(FailureReason::UserNotFound.as_str(), "USER_NOT_FOUND");
        assert_eq!(FailureReason::InvalidPassword.as_str(), "INVALID_PASSWORD");
        assert_eq!(FailureReason::TotpRequired.as_str(), "TOTP_REQUIRED");
    }

    #[test]
    fn test_log_security_event_compiles() {
        // This test just ensures the function signature is correct
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        log_security_event(
            SecurityEventType::LoginFailure,
            Some(ip),
            Some("user123"),
            Some("test@example.com"),
            Some(FailureReason::InvalidPassword),
            Some("Additional context"),
        );
    }

    #[test]
    fn test_log_with_ipv6() {
        let ip = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
        log_security_event(
            SecurityEventType::LoginSuccess,
            Some(ip),
            Some("user456"),
            Some("test@example.com"),
            None,
            None,
        );
    }
}
