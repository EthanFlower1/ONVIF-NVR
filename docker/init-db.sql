-- This is just a placeholder for database initialization.
-- Your Rust application should handle schema migrations through its own mechanisms.
-- If you have specific tables or data to pre-populate, add them here.

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Create a dummy table to verify database connection
CREATE TABLE IF NOT EXISTS docker_ready (
    id SERIAL PRIMARY KEY,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

INSERT INTO docker_ready (created_at) VALUES (NOW());

-- You can add more initialization SQL here as needed