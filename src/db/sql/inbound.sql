-- Inbound domains: a domain whose mail a receiving provider accepts for every
-- address (the domains accounts are signed up on), read by Skrzynka through
-- wisent-integrations instead of IMAP. The domain is the organization's
-- declaration; the messages are what Skrzynka received for it.
CREATE TABLE IF NOT EXISTS inbound_domains (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL,
    domain TEXT NOT NULL,
    last_sync_at TEXT,
    last_error_code TEXT,
    last_error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (organization_id, domain)
);
CREATE TABLE IF NOT EXISTS inbound_messages (
    id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES inbound_domains(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL,
    sender TEXT NOT NULL,
    recipients TEXT NOT NULL,
    subject TEXT NOT NULL,
    received_at TEXT NOT NULL,
    body_text TEXT NOT NULL,
    UNIQUE (domain_id, provider_id)
);
CREATE INDEX IF NOT EXISTS inbound_messages_domain_idx ON inbound_messages (domain_id, received_at DESC);
