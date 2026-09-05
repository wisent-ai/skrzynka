import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const repository = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

function runMailboxTarget() {
  return new Promise((resolveRun, rejectRun) => {
    const child = spawn(
      "cargo",
      ["test", "--test", "mailboxes", "--", "--nocapture"],
      {
        cwd: repository,
        env: process.env,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    const stdout = [];
    const stderr = [];
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.once("error", rejectRun);
    child.once("close", (code, signal) => {
      resolveRun({
        code,
        signal,
        stdout: Buffer.concat(stdout).toString("utf8"),
        stderr: Buffer.concat(stderr).toString("utf8"),
      });
    });
  });
}

test("real mailbox add, edit, and removal journeys pass", { timeout: 600_000 }, async () => {
  const result = await runMailboxTarget();
  assert.equal(
    result.code,
    0,
    `mailbox integration target failed${result.signal ? ` with ${result.signal}` : ""}\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
  );
  assert.match(result.stdout, /test result: ok\. 6 passed; 0 failed;/);
  for (const name of [
    "mailbox_add_persists_only_the_profile_and_refuses_duplicate_or_invalid_accounts",
    "mailbox_disable_changes_only_enabled_state_and_refuses_unknown_accounts",
    "mailbox_enable_changes_only_enabled_state_and_refuses_unknown_accounts",
    "mailbox_remove_requires_confirmation_deletes_local_state_and_preserves_skarbiec",
    "schema_three_migration_adds_smtp_credential_without_rewriting_mail_history",
    "gmail_app_password_mailbox_selector_refusals_leave_credentials_and_mailboxes_unchanged",
  ]) {
    assert.match(result.stdout, new RegExp(`test ${name} \\.\\.\\. ok`));
  }
});
