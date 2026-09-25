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

test("real mailbox declaration, undeclaration and removal journeys pass", { timeout: 600_000 }, async () => {
  // Each name is the module its case lives in; a name that moves must move here too.
  const histories = [
    "credentials::a_declared_mailbox_persists_only_the_profile_and_refuses_profile_overrides",
    "lifecycle::a_mailbox_exists_while_its_skarbiec_item_carries_the_tag",
    "lifecycle::undeclare_removes_only_the_mailbox_tag_and_keeps_the_mailbox",
    "lifecycle::a_mailbox_cannot_be_added_beside_skarbiec",
    "lifecycle::mailbox_remove_requires_undeclare_and_confirmation_and_preserves_skarbiec",
    "lifecycle::schema_three_migration_adds_smtp_credential_without_rewriting_mail_history",
    "account_sources::declare_refuses_incomplete_skarbiec_profile_without_creating_local_account",
    "account_sources::invalid_source_security_does_not_fall_back_to_a_different_transport",
    "readiness::every_connection_path_is_reported_and_none_is_usable_without_its_declaration",
    "readiness::a_consumer_account_is_refused_for_delegation_without_asking_google",
    "readiness::an_address_that_is_not_an_address_is_refused_before_anything_is_contacted",
  ];
  const result = await runMailboxTarget();
  assert.equal(
    result.code,
    0,
    `cargo test process exited with code ${result.code}${result.signal ? ` after signal ${result.signal}` : ""}; measured from the child-process exit status\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
  );

  const summary =
    /test result: (?:ok|FAILED)\. (?<passed>\d+) passed; (?<failed>\d+) failed;(?: \d+ ignored;)?(?: (?<measured>\d+) measured;)?/.exec(
      result.stdout,
    );
  assert.ok(
    summary,
    `cargo test emitted no recognizable result summary; measured by parsing passed, failed, and optional measured counts from stdout\nstdout:\n${result.stdout}`,
  );
  assert.equal(
    Number(summary.groups.failed),
    0,
    `cargo test reported ${summary.groups.failed} failed tests; measured from the failed count in its stdout summary`,
  );
  if (summary.groups.measured !== undefined) {
    assert.equal(
      Number(summary.groups.measured),
      0,
      `cargo test reported ${summary.groups.measured} measured tests; measured from the measured count in its stdout summary`,
    );
  }

  for (const name of histories) {
    assert.match(
      result.stdout,
      new RegExp(`test ${name} \\.\\.\\. ok`),
      `named mailbox history "${name}" did not finish with ok; measured from its individual cargo test result line in stdout`,
    );
  }

  // Stable Rust has no machine-readable test result, so the parsed pass count is a lower bound that stays valid when tests are added.
  assert.ok(
    Number(summary.groups.passed) >= histories.length,
    `cargo test reported ${summary.groups.passed} passed tests, fewer than the ${histories.length} named mailbox histories; measured from the passed count in its stdout summary`,
  );
});
