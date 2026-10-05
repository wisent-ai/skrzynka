#!/usr/bin/env node
// Uses a dedicated real mailbox; never tags, sends, deletes, or resets a cursor.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' },
  fixture: { type: 'string' }, binary: { type: 'string', default: 'skrzynka' },
} });
const outputRoot = join(root, '.build/real-tests/mailbox-backlog');
mkdirSync(outputRoot, { recursive: true });
const output = mkdtempSync(join(outputRoot, 'run-'));
const report = { started_at: new Date().toISOString(), commands: [], status: 'running' };
const hash = data => createHash('sha256').update(data).digest('hex');
function run(binary, args) {
  const result = spawnSync(binary, args, { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
  const index = report.commands.length;
  writeFileSync(join(output, `${index}.stdout`), result.stdout ?? '');
  writeFileSync(join(output, `${index}.stderr`), result.stderr ?? '');
  report.commands.push({ argv: [binary, ...args], exit_status: result.status,
    signal: result.signal, error: result.error?.message,
    stdout: `${index}.stdout`, stderr: `${index}.stderr` });
  if (result.error) throw result.error;
  return result;
}
function successful(binary, args) {
  const result = run(binary, args);
  assert.equal(result.status, 0, `${binary} ${args.join(' ')} failed: ${result.stderr}`);
  return result.stdout;
}
function command(args, organization = values.organization) {
  return JSON.parse(successful(values.binary, ['--organization', organization, ...args]));
}
function allMessages() {
  const messages = command(['message', 'list', '--mailbox', values.mailbox]);
  assert(Array.isArray(messages), 'message list did not return an array');
  return messages;
}
try {
  report.source_revision = successful('git', ['rev-parse', 'HEAD']).trim();
  report.source_diff = successful('git', ['diff', '--', 'src', 'Cargo.toml', 'Cargo.lock']);
  report.test_sha256 = hash(readFileSync(fileURLToPath(import.meta.url)));
  const binaryPath = values.binary.includes('/') ? resolve(values.binary)
    : successful('/usr/bin/which', [values.binary]).trim();
  report.binary_path = binaryPath;
  report.binary_sha256 = hash(readFileSync(binaryPath));
  report.version = successful(values.binary, ['--version']).trim();
  assert(values.organization, '--organization is required');
  // Fail on an incompatible installed CLI before inspecting fixture data.
  successful(values.binary, ['--organization', values.organization, 'sync', '--help']);
  assert(values.mailbox && values.fixture,
    '--mailbox and --fixture are required; use a dedicated real mailbox with an unimported backlog fixture');
  const fixtureBytes = readFileSync(resolve(values.fixture));
  report.fixture_sha256 = hash(fixtureBytes);
  const expected = JSON.parse(fixtureBytes);
  assert(Array.isArray(expected) && expected.length > 1,
    'Fixture must describe real messages, not generated or simulated provider responses');
  for (const message of expected) {
    assert(Number.isSafeInteger(message.external_uid) && message.external_uid > 0);
    for (const field of ['message_id', 'subject', 'body_text']) assert.equal(typeof message[field], 'string');
  }
  assert.equal(new Set(expected.map(message => message.external_uid)).size, expected.length);
  const before = command(['mailbox', 'show', values.mailbox]);
  const pending = expected.filter(message => message.external_uid > before.last_uid);
  assert(pending.length > 1, 'Fixture has no backlog past the cursor; qualification cannot pass without importing one');
  // One pass imports everything past the cursor: no batch bound leaves a remainder.
  const summary = command(['sync', '--mailbox', values.mailbox]);
  const summaries = [summary];
  assert.equal(summary.mailbox_id, values.mailbox);
  assert.equal('has_more' in summary, false, 'Sync still reports a page continuation');
  const highest = Math.max(...pending.map(message => message.external_uid));
  assert(summary.last_uid >= highest, `One pass stopped at UID ${summary.last_uid} before ${highest}`);
  const stored = command(['mailbox', 'show', values.mailbox]);
  assert.equal(stored.last_uid, summary.last_uid, 'Reported cursor was not persisted');
  const messages = allMessages();
  for (const fixture of expected) {
    const matches = messages.filter(message => message.external_uid === fixture.external_uid);
    assert.equal(matches.length, 1, `UID ${fixture.external_uid} was skipped or duplicated`);
    const persisted = command(['message', 'show', matches[0].id]);
    assert.equal(persisted.mailbox_id, values.mailbox);
    for (const field of ['external_uid', 'message_id', 'subject', 'body_text']) {
      assert.equal(persisted[field], fixture[field], `Persisted ${field} differs for UID ${fixture.external_uid}`);
    }
  }
  const repeated = command(['sync', '--mailbox', values.mailbox]);
  const after = allMessages();
  for (const fixture of expected) {
    assert.equal(after.filter(message => message.external_uid === fixture.external_uid).length, 1,
      `Repeated sync duplicated UID ${fixture.external_uid}`);
  }
  const refused = run(values.binary, ['--organization', `${values.organization}-outside`,
    'message', 'show', messages.find(message => message.external_uid === expected[0].external_uid).id]);
  assert.equal(refused.status, 1, 'Cross-organization message read was not refused');
  assert.equal(JSON.parse(refused.stderr).error.code, 'NOT_FOUND');
  report.observed = { initial_cursor: before.last_uid, summaries, repeated,
    verified_message_count: expected.length };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  process.exitCode = 1;
} finally {
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
