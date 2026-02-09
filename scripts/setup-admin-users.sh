#!/bin/bash
# scripts/setup-admin-users.sh
# Sets up test admin users in the Wavry Gateway database for admin panel testing

set -euo pipefail

# Default paths
DB_PATH="${WAVRY_DB_PATH:-.wavry/gateway.db}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}"))" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "🔧 Wavry Admin User Setup"
echo "========================="
echo ""

# Check if database exists
if [ ! -f "$DB_PATH" ]; then
    echo "❌ Database not found at: $DB_PATH"
    echo "   Run wavry-gateway first to initialize the database."
    exit 1
fi

# Helper function to hash password using Argon2id
# Note: This requires cargo-run of a small Rust utility or using system argon2
hash_password() {
    local password=$1

    # Try using Rust if available
    if command -v cargo &> /dev/null; then
        # Create a temporary Rust script to hash the password
        cat > /tmp/hash_pw.rs << 'RUST_SCRIPT'
use argon2::{Argon2, PasswordHasher};
use argon2::password_hash::SaltString;
use rand::rngs::OsRng;

fn main() {
    let password = std::env::args().nth(1).expect("Password required");
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)
        .expect("Failed to hash")
        .to_string();
    println!("{}", hash);
}
RUST_SCRIPT
        rustc /tmp/hash_pw.rs -L dependency=/path/to/deps 2>/dev/null || {
            echo "⚠️  Using simple hash fallback (not production-safe)"
            echo "argon2id\$v=19\$m=19456,t=2,p=1\$$(openssl rand -base64 12 | tr -d '=\n')\$hash_of_$password"
            return
        }
        /tmp/hash_pw "$password"
    else
        # Fallback: Use simple placeholder (INSECURE - for testing only)
        echo "argon2id\$v=19\$m=19456,t=2,p=1\$test\$hash_of_$password"
    fi
}

# Function to insert user
create_test_user() {
    local email=$1
    local username=$2
    local display_name=$3
    local password=$4

    echo "Creating user: $username ($email)"

    # Generate a test public key (dummy Ed25519 key for testing)
    local public_key="$(openssl rand -hex 32)"

    # Use simple hash for testing
    local password_hash="argon2id\$v=19\$m=19456,t=2,p=1\$test\$hash_of_$password"

    # Insert into database using sqlite3
    sqlite3 "$DB_PATH" <<EOF
INSERT OR IGNORE INTO users
    (id, email, username, display_name, password_hash, public_key, created_at)
VALUES
    ('admin-user-1', '$email', '$username', '$display_name', '$password_hash', '$public_key', CURRENT_TIMESTAMP);
EOF

    echo "   ✓ User created: id=admin-user-1"
}

# Function to create session for testing
create_test_session() {
    local user_id=$1
    local token=$2

    echo "Creating test session for user: $user_id"

    # Hash the token
    local token_hash="h1:$(echo -n "$token" | sha256sum | cut -d' ' -f1)"

    sqlite3 "$DB_PATH" <<EOF
INSERT OR IGNORE INTO sessions
    (token, user_id, expires_at, created_at)
VALUES
    ('$token_hash', '$user_id', datetime('now', '+24 hours'), CURRENT_TIMESTAMP);
EOF

    echo "   ✓ Session created"
    echo "   📋 Session Token: $token"
}

# Main setup
echo "📝 Creating test admin users..."
echo ""

# Admin user
create_test_user \
    "admin@wavry.local" \
    "admin" \
    "Admin User" \
    "admin-password-123"

echo ""
echo "✅ Setup Complete"
echo ""
echo "📊 Admin Dashboard Access:"
echo "   URL: http://localhost:3000/admin"
echo ""
echo "🔐 Authentication Methods:"
echo "   1. Header: x-admin-token: <ADMIN_PANEL_TOKEN>"
echo "   2. Header: Authorization: Bearer <ADMIN_PANEL_TOKEN>"
echo ""
echo "🚀 To test the admin panel:"
echo "   1. Start the gateway with admin token:"
echo "      ADMIN_PANEL_TOKEN='$(openssl rand -hex 32)' cargo run --bin wavry-gateway"
echo ""
echo "   2. Visit http://localhost:3000/admin"
echo ""
echo "   3. Include admin token in requests:"
echo "      curl -H 'x-admin-token: <token>' http://localhost:3000/admin/api/overview"
echo ""
echo "📝 API Endpoints:"
echo "   GET  /admin/api/overview     - System overview"
echo "   POST /admin/api/sessions/revoke - Revoke session"
echo "   POST /admin/api/users/ban    - Ban user"
echo "   POST /admin/api/users/unban  - Unban user"
echo ""
