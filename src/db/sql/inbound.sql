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
    subject TEXT,
    received_at TEXT NOT NULL,
    body_text TEXT NOT NULL,
    UNIQUE (domain_id, provider_id)
);
CREATE INDEX IF NOT EXISTS inbound_messages_domain_idx ON inbound_messages (domain_id, received_at DESC);
-- A message's subject is NULL when it states none. A table older builds wrote
-- keeps subject NOT NULL and holds '' where they wrote one in for a feed row
-- without a subject; those rows are carried to NULL once, as the constraint
-- is dropped.
DO $$
BEGIN
    IF EXISTS (
        SELECT column_name FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'inbound_messages'
          AND column_name = 'subject'
          AND is_nullable = 'NO'
    ) THEN
        ALTER TABLE inbound_messages ALTER COLUMN subject DROP NOT NULL;
        UPDATE inbound_messages SET subject = NULL WHERE subject = '';
    END IF;
END
$$;

-- The rest of this file is how the operator handles stored mail inside
-- Skrzynka: blocked senders, tag rules, snoozes, templates and follow-ups.
-- None of it reaches the provider. This file is created after schema.sql,
-- whose tables these refer to.

-- Senders (an address, or `@domain`) whose mail listings leave out.
CREATE TABLE IF NOT EXISTS blocked_senders (
    organization_id TEXT NOT NULL,
    address TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (organization_id, address)
);

-- Recipients (an address, or `@domain`) no originated mail may be sent to,
-- with why (docs/outreach/suppression.md).
CREATE TABLE IF NOT EXISTS suppressed_recipients (
    organization_id TEXT NOT NULL,
    entry TEXT NOT NULL,
    reason TEXT,
    added_at TEXT NOT NULL,
    PRIMARY KEY (organization_id, entry)
);

-- Addresses the operator says work again although a stored bounce report
-- says delivery to them failed for good: a report received before
-- `cleared_at` no longer counts (docs/delivery/report.md).
CREATE TABLE IF NOT EXISTS bounce_clearances (
    organization_id TEXT NOT NULL,
    address TEXT NOT NULL,
    cleared_at TEXT NOT NULL,
    PRIMARY KEY (organization_id, address)
);

-- Rules that tag mail as it is imported (docs/lists/rules.md).
CREATE TABLE IF NOT EXISTS tag_rules (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    sender TEXT,
    list TEXT,
    contains TEXT,
    created_at TEXT NOT NULL
);
-- A rule kept to one mailbox's mail; absent for every mailbox of the organization.
ALTER TABLE tag_rules ADD COLUMN IF NOT EXISTS mailbox_id TEXT REFERENCES mailboxes(id) ON DELETE CASCADE;
-- Text a rule's message must carry in its To or Cc header; absent for any.
ALTER TABLE tag_rules ADD COLUMN IF NOT EXISTS recipient TEXT;
-- Text the recorded name of one of a rule's message's files must contain; absent for any.
ALTER TABLE tag_rules ADD COLUMN IF NOT EXISTS attachment TEXT;
-- When a rule was paused: imports and `rule apply` without --rule skip it; absent while it tags.
ALTER TABLE tag_rules ADD COLUMN IF NOT EXISTS paused_at TEXT;

-- The Cc header, recorded with a message's details; replies to all copy it.
ALTER TABLE messages ADD COLUMN IF NOT EXISTS cc_header TEXT;
ALTER TABLE reply_attempts ADD COLUMN IF NOT EXISTS reply_all BOOLEAN NOT NULL DEFAULT FALSE;

-- A message out of sight until an instant (docs/lists/snooze.md).
CREATE TABLE IF NOT EXISTS message_snoozes (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    until TEXT NOT NULL
);

-- Texts kept to reuse in replies and messages (docs/writing/templates.md).
CREATE TABLE IF NOT EXISTS message_templates (
    organization_id TEXT NOT NULL,
    name TEXT NOT NULL,
    body TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (organization_id, name)
);
-- The subject a new message from the template carries; NULL for none.
ALTER TABLE message_templates ADD COLUMN IF NOT EXISTS subject TEXT;

-- An originated message the operator expects an answer to by an instant
-- (docs/writing/follow-ups.md).
CREATE TABLE IF NOT EXISTS follow_ups (
    outbound_id TEXT PRIMARY KEY REFERENCES outbound_messages(id) ON DELETE CASCADE,
    due_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- The operator's own note on a stored message (docs/lists/notes.md).
CREATE TABLE IF NOT EXISTS message_notes (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- A listing's filters kept under a name, as JSON (docs/messages/filters.md).
CREATE TABLE IF NOT EXISTS saved_views (
    organization_id TEXT NOT NULL,
    name TEXT NOT NULL,
    filters TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (organization_id, name)
);

-- A message_actions table older builds created allows only seen, unseen,
-- move and delete; marking flagged and unflagged (docs/messages/
-- provider-actions.md) is admitted once, by replacing that check.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT conname FROM pg_constraint
        WHERE conrelid = 'message_actions'::regclass
          AND conname = 'message_actions_action_check'
          AND pg_get_constraintdef(oid) LIKE '%flagged%'
    ) THEN
        ALTER TABLE message_actions DROP CONSTRAINT IF EXISTS message_actions_action_check;
        ALTER TABLE message_actions ADD CONSTRAINT message_actions_action_check
            CHECK (action IN ('seen', 'unseen', 'flagged', 'unflagged', 'move', 'delete'));
    END IF;
END
$$;

-- Blind copies of originated mail and of drafts (docs/messages/reads.md,
-- docs/writing/drafts.md): recipients the sent headers never name,
-- comma-separated; NULL (outbound) or '' (draft) for none.
ALTER TABLE outbound_messages ADD COLUMN IF NOT EXISTS bcc TEXT;
ALTER TABLE drafts ADD COLUMN IF NOT EXISTS bcc TEXT NOT NULL DEFAULT '';

-- Muted conversations (docs/conversations/thread.md): every Message-ID a
-- conversation carried or named when it was muted, per mailbox. A message
-- carrying or naming one of them is left out of listings.
CREATE TABLE IF NOT EXISTS muted_conversations (
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    identity TEXT NOT NULL,
    muted_at TEXT NOT NULL,
    PRIMARY KEY (mailbox_id, identity)
);

-- Addresses a reply copies openly or blindly (docs/writing/reply-all.md),
-- comma-separated; NULL for none.
ALTER TABLE reply_attempts ADD COLUMN IF NOT EXISTS cc TEXT;
ALTER TABLE reply_attempts ADD COLUMN IF NOT EXISTS bcc TEXT;

-- Automatic replies (docs/replies/auto-reply.md). A mailbox with a row
-- answers mail received from starts_at until ends_at (open when absent) with
-- body, at most once per sender within resend_after_seconds, as an RFC 3834
-- auto-reply. A mailbox without a row answers nothing automatically.
CREATE TABLE IF NOT EXISTS mailbox_auto_replies (
    mailbox_id TEXT PRIMARY KEY REFERENCES mailboxes(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    starts_at TEXT NOT NULL,
    ends_at TEXT,
    resend_after_seconds BIGINT NOT NULL,
    updated_at TEXT NOT NULL
);
-- What the responder decided for each message it considered, once: the
-- reply attempt it sent, or why it answered nothing. The row is claimed
-- before anything is sent, so two processes never answer one message twice.
CREATE TABLE IF NOT EXISTS auto_reply_decisions (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    sender TEXT NOT NULL,
    reply_attempt_id TEXT,
    skipped TEXT,
    decided_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS auto_reply_decisions_sender_idx
    ON auto_reply_decisions (mailbox_id, sender, decided_at DESC);
-- Every import that writes messages, and every change to an automatic
-- reply, wakes the responder of every running service, whichever process
-- made it.
CREATE OR REPLACE FUNCTION skrzynka_announce_received() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify('skrzynka_received', '');
    RETURN NULL;
END;
$$;
CREATE OR REPLACE TRIGGER messages_received
AFTER INSERT ON messages
FOR EACH STATEMENT EXECUTE FUNCTION skrzynka_announce_received();
CREATE OR REPLACE TRIGGER auto_replies_changed
AFTER INSERT OR UPDATE ON mailbox_auto_replies
FOR EACH STATEMENT EXECUTE FUNCTION skrzynka_announce_received();
-- The order messages were stored in, for watchers that hand on each new
-- message once (`message watch`, `GET /v1/messages/watch`). A number is taken
-- under a transaction lock held until commit, so numbers become visible in
-- the order they were taken: a watcher that has read up to one number never
-- later finds a smaller one committed. Messages stored before the column
-- existed are numbered once, in an order of no meaning.
CREATE SEQUENCE IF NOT EXISTS messages_stored_order;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS stored_order BIGINT;
UPDATE messages SET stored_order = nextval('messages_stored_order') WHERE stored_order IS NULL;
ALTER TABLE messages ALTER COLUMN stored_order SET NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS messages_stored_order_idx ON messages (stored_order);
CREATE OR REPLACE FUNCTION skrzynka_order_stored() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtext('messages_stored_order'));
    NEW.stored_order := nextval('messages_stored_order');
    RETURN NEW;
END;
$$;
CREATE OR REPLACE TRIGGER messages_stored_order
BEFORE INSERT ON messages
FOR EACH ROW EXECUTE FUNCTION skrzynka_order_stored();
