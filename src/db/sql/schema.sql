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
    last_uid BIGINT NOT NULL,
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
    recipients TEXT,
    subject TEXT,
    sent_at TEXT,
    sent_at_unreadable TEXT,
    received_at TEXT NOT NULL,
    body_text TEXT NOT NULL,
    snippet TEXT NOT NULL,
    UNIQUE (mailbox_id, external_uid)
);
CREATE INDEX IF NOT EXISTS messages_received_idx ON messages (received_at DESC);
CREATE INDEX IF NOT EXISTS messages_mailbox_idx ON messages (mailbox_id, received_at DESC);
-- A message keeps its To and Subject as it states them, NULL when it states
-- none, and a Date header that does not read as an instant in
-- sent_at_unreadable. A table older builds wrote lacks that column, and its
-- rows hold the '(no subject)' and empty recipient list those builds wrote
-- for headers a message left out; they are carried to NULL once, as the
-- column is added.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT column_name FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'messages'
          AND column_name = 'sent_at_unreadable'
    ) THEN
        ALTER TABLE messages ALTER COLUMN recipients DROP NOT NULL;
        ALTER TABLE messages ALTER COLUMN subject DROP NOT NULL;
        UPDATE messages SET subject = NULL WHERE subject = '(no subject)';
        UPDATE messages SET recipients = NULL WHERE recipients = '';
        ALTER TABLE messages ADD COLUMN sent_at_unreadable TEXT;
    END IF;
END
$$;
-- What an import recorded beyond a message's text: one row per attachment, by
-- its index among the message's attachments in MIME order, and the list
-- headers it states (List-Id, List-Unsubscribe, List-Unsubscribe-Post).
-- Attachment bytes are not kept; they are read from the provider when asked
-- for. A message imported before these were recorded keeps details_recorded
-- FALSE, so "not read yet" is told from "has none".
ALTER TABLE messages ADD COLUMN IF NOT EXISTS details_recorded BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS list_id TEXT;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS list_unsubscribe TEXT;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS list_unsubscribe_post TEXT;
CREATE TABLE IF NOT EXISTS message_attachments (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    attachment_index BIGINT NOT NULL,
    filename TEXT,
    content_type TEXT NOT NULL,
    size_bytes BIGINT,
    unreadable TEXT,
    PRIMARY KEY (message_id, attachment_index)
);
-- One request to leave the mailing list a stored message came from, claimed
-- by its idempotency key before the list is asked: a one-click HTTPS POST
-- (RFC 8058) or mail from the receiving mailbox (RFC 2369 mailto).
CREATE TABLE IF NOT EXISTS unsubscribe_attempts (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    idempotency_key TEXT NOT NULL UNIQUE,
    method TEXT NOT NULL CHECK (method IN ('one_click', 'mailto')),
    target TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'sending', 'sent', 'failed', 'uncertain')),
    provider_message_id TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT
);
CREATE INDEX IF NOT EXISTS unsubscribe_message_idx ON unsubscribe_attempts (message_id, created_at DESC);
-- The operator's own labels on stored messages: local to Skrzynka, never
-- sent to the provider. One row per message and tag.
CREATE TABLE IF NOT EXISTS message_tags (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    tagged_at TEXT NOT NULL,
    PRIMARY KEY (message_id, tag)
);
CREATE INDEX IF NOT EXISTS message_tags_tag_idx ON message_tags (tag);
-- The receiving provider's Authentication-Results verdicts on the sender, as
-- JSON, recorded with the message's other details.
ALTER TABLE messages ADD COLUMN IF NOT EXISTS authentication_results TEXT;
-- How many messages the operator lets a mailbox send in one UTC day. A
-- mailbox without a row has no limit. Claims of a send lock this row, so the
-- count and the claim happen as one step.
CREATE TABLE IF NOT EXISTS mailbox_send_limits (
    mailbox_id TEXT PRIMARY KEY REFERENCES mailboxes(id) ON DELETE CASCADE,
    per_day BIGINT NOT NULL,
    updated_at TEXT NOT NULL
);
-- A warm-up: from the UTC day `ramp_since` the limit grows by `ramp_by` each
-- day, from `per_day`, until it reaches `ramp_to`. All three or none are set.
ALTER TABLE mailbox_send_limits ADD COLUMN IF NOT EXISTS ramp_by BIGINT;
ALTER TABLE mailbox_send_limits ADD COLUMN IF NOT EXISTS ramp_to BIGINT;
ALTER TABLE mailbox_send_limits ADD COLUMN IF NOT EXISTS ramp_since TEXT;
-- A stored message that is a delivery status notification (RFC 3464): the
-- Message-ID of the send it reports on, why the report could not be read
-- when it could not, and one row per recipient outcome as the reporting
-- server wrote it. Recorded with the message's other details.
CREATE TABLE IF NOT EXISTS message_bounces (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    original_message_id TEXT,
    unreadable TEXT
);
CREATE INDEX IF NOT EXISTS message_bounces_original_idx ON message_bounces (original_message_id);
CREATE TABLE IF NOT EXISTS message_bounce_recipients (
    message_id TEXT NOT NULL REFERENCES message_bounces(message_id) ON DELETE CASCADE,
    recipient TEXT,
    action TEXT,
    status TEXT,
    diagnostic TEXT
);
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
-- The files a reply carries, as a JSON array of {filename, content_type,
-- size_bytes, sha256}; the bytes are not kept. Replies sent before files
-- could be attached carried none, which '[]' states.
ALTER TABLE reply_attempts ADD COLUMN IF NOT EXISTS attachments TEXT NOT NULL DEFAULT '[]';
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
-- The files an outbound message carries, as a JSON array of {filename,
-- content_type, size_bytes, sha256}; the bytes are not kept. Messages sent
-- before files could be attached carried none, which '[]' states.
ALTER TABLE outbound_messages ADD COLUMN IF NOT EXISTS attachments TEXT NOT NULL DEFAULT '[]';
-- Every change an operator asked Skrzynka to make to a message at its
-- provider: marking it seen, unseen, flagged or unflagged, moving it to another folder, or
-- deleting it. A row is claimed as pending before the provider is asked and
-- ends done, failed or uncertain with the provider's or transport's words.
CREATE TABLE IF NOT EXISTS message_actions (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    action TEXT NOT NULL CHECK (action IN ('seen', 'unseen', 'flagged', 'unflagged', 'move', 'delete')),
    folder TEXT,
    status TEXT NOT NULL CHECK (status IN ('pending', 'done', 'failed', 'uncertain')),
    error_code TEXT,
    error_message TEXT,
    requested_at TEXT NOT NULL,
    completed_at TEXT
);
CREATE INDEX IF NOT EXISTS message_actions_message_idx ON message_actions (message_id, requested_at DESC);
-- What the service watching mailboxes acts on is announced on the
-- skrzynka_mailboxes channel, whichever process wrote it: a mailbox created
-- or removed; one enabled, disabled or pointed at another account or IMAP
-- endpoint; and one whose recorded failure a successful sync cleared, so its
-- stopped watcher starts again. A sync that changes nothing else (last_uid,
-- last_sync_at, the SMTP side) and a failure being recorded announce nothing.
CREATE OR REPLACE FUNCTION skrzynka_announce_mailboxes() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify('skrzynka_mailboxes', '');
    RETURN NULL;
END;
$$;
CREATE OR REPLACE TRIGGER mailboxes_added_or_removed
AFTER INSERT OR DELETE ON mailboxes
FOR EACH STATEMENT EXECUTE FUNCTION skrzynka_announce_mailboxes();
CREATE OR REPLACE TRIGGER mailboxes_rewatched
AFTER UPDATE ON mailboxes FOR EACH ROW
WHEN (
    OLD.enabled IS DISTINCT FROM NEW.enabled
    OR OLD.skarbiec_item_id IS DISTINCT FROM NEW.skarbiec_item_id
    OR OLD.email IS DISTINCT FROM NEW.email
    OR OLD.imap_host IS DISTINCT FROM NEW.imap_host
    OR OLD.imap_port IS DISTINCT FROM NEW.imap_port
    OR (OLD.last_error_code IS NOT NULL AND NEW.last_error_code IS NULL)
)
EXECUTE FUNCTION skrzynka_announce_mailboxes();
-- The text a mailbox signs its replies and originated mail with, appended
-- after an RFC 3676 "-- " line. A mailbox without a row signs nothing. Each
-- reply and outbound row records the signature it was claimed with, so a
-- repeated request answers what was first sent whatever the signature is now.
CREATE TABLE IF NOT EXISTS mailbox_signatures (
    mailbox_id TEXT PRIMARY KEY REFERENCES mailboxes(id) ON DELETE CASCADE,
    signature TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
ALTER TABLE outbound_messages ADD COLUMN IF NOT EXISTS signature TEXT;
ALTER TABLE reply_attempts ADD COLUMN IF NOT EXISTS signature TEXT;
-- Messages being written: kept only in Skrzynka until `message draft send`
-- originates them, then removed once the provider accepted the send.
CREATE TABLE IF NOT EXISTS drafts (
    id TEXT PRIMARY KEY,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    recipients TEXT NOT NULL,
    cc TEXT NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS drafts_mailbox_idx ON drafts (mailbox_id, updated_at DESC);
-- A scheduled draft: a running service sends it at send_at under send_key;
-- a send that does not end `sent` clears send_at and keeps why.
ALTER TABLE drafts ADD COLUMN IF NOT EXISTS send_at TEXT;
ALTER TABLE drafts ADD COLUMN IF NOT EXISTS send_key TEXT;
ALTER TABLE drafts ADD COLUMN IF NOT EXISTS schedule_error TEXT;
CREATE INDEX IF NOT EXISTS drafts_send_at_idx ON drafts (send_at) WHERE send_at IS NOT NULL;
-- The files a draft carries until it is sent, as a JSON array of {filename,
-- content_type, content_base64}; drafts written before files could be kept
-- carried none, which '[]' states.
ALTER TABLE drafts ADD COLUMN IF NOT EXISTS attachments TEXT NOT NULL DEFAULT '[]';
-- Every change to a draft's schedule wakes the scheduler of every running
-- service, whichever process made it.
CREATE OR REPLACE FUNCTION skrzynka_announce_drafts() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify('skrzynka_drafts', '');
    RETURN NULL;
END;
$$;
CREATE OR REPLACE TRIGGER drafts_rescheduled
AFTER INSERT OR DELETE OR UPDATE OF send_at ON drafts
FOR EACH STATEMENT EXECUTE FUNCTION skrzynka_announce_drafts();
