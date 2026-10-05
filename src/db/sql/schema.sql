CREATE TABLE IF NOT EXISTS schema_meta (
    key TEXT PRIMARY KEY,
    value BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS mailboxes (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL,
    skarbiec_item_id TEXT NOT NULL,
    smtp_skarbiec_item_id TEXT,
    display_name TEXT NOT NULL,
    email TEXT NOT NULL,
    imap_host TEXT NOT NULL,
    imap_port BIGINT NOT NULL,
    smtp_host TEXT NOT NULL,
    smtp_port BIGINT NOT NULL,
    smtp_security TEXT NOT NULL CHECK (smtp_security IN ('starttls', 'tls')),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_uid BIGINT NOT NULL DEFAULT 0,
    last_sync_at TEXT,
    last_error_code TEXT,
    last_error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (organization_id, skarbiec_item_id)
);
-- A mailbox is read when its provider reports new mail (IMAP IDLE), not on a
-- stored interval, so the interval column of older databases is dropped.
ALTER TABLE mailboxes DROP COLUMN IF EXISTS poll_interval_seconds;
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    external_uid BIGINT NOT NULL,
    provider_message_id TEXT,
    in_reply_to TEXT,
    references_header TEXT,
    sender TEXT NOT NULL,
    reply_to TEXT,
    recipients TEXT NOT NULL,
    subject TEXT NOT NULL,
    sent_at TEXT,
    received_at TEXT NOT NULL,
    body_text TEXT NOT NULL,
    snippet TEXT NOT NULL,
    UNIQUE (mailbox_id, external_uid)
);
CREATE INDEX IF NOT EXISTS messages_received_idx ON messages (received_at DESC);
CREATE INDEX IF NOT EXISTS messages_mailbox_idx ON messages (mailbox_id, received_at DESC);
CREATE TABLE IF NOT EXISTS reply_attempts (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    idempotency_key TEXT NOT NULL UNIQUE,
    body TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'sending', 'sent', 'failed', 'uncertain')),
    provider_message_id TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    sent_at TEXT
);
CREATE INDEX IF NOT EXISTS replies_message_idx ON reply_attempts (message_id, created_at DESC);
CREATE TABLE IF NOT EXISTS outbound_messages (
    id TEXT PRIMARY KEY,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    idempotency_key TEXT NOT NULL UNIQUE,
    recipients TEXT NOT NULL,
    cc TEXT,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'sending', 'sent', 'failed', 'uncertain')),
    provider_message_id TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    sent_at TEXT
);
CREATE INDEX IF NOT EXISTS outbound_mailbox_idx ON outbound_messages (mailbox_id, created_at DESC);
