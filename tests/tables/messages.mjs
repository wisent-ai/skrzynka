#!/usr/bin/env node
// Reads a dedicated real mailbox that already holds synced mail; never sends,
// tags, deletes or moves anything. Proves `message list --csv`: the table
// holds exactly the messages the listing prints, in its order and with its
// fields, honours the listing's filters, and refuses an existing path, an
// unwritable one and a blank filter.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { readTable } from './csv.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
assert(values.organization && values.mailbox, 'Pass --organization and --mailbox of a dedicated synced mailbox');
const outputRoot = join(root, '.build/real-tests/message-table');
mkdirSync(outputRoot, { recursive: true });
const output = mkdtempSync(join(outputRoot, 'run-'));
const report = {
  started_at: new Date().toISOString(), source_revision: values['source-revision'],
  commands: [], tables: [], status: 'running',
};
const hash = data => createHash('sha256').update(data).digest('hex');
function run(args) {
  const index = report.commands.length;
  const stdoutPath = join(output, `${index}.stdout`);
  const stderrPath = join(output, `${index}.stderr`);
  const stdout = openSync(stdoutPath, 'wx');
  const stderr = openSync(stderrPath, 'wx');
  let result;
  try {
    result = spawnSync(values.binary, ['--organization', values.organization, 'message', ...args],
      { cwd: root, stdio: ['ignore', stdout, stderr] });
  } finally {
    closeSync(stdout);
    closeSync(stderr);
  }
  report.commands.push({ argv: [values.binary, 'message', ...args], exit_status: result.status,
    signal: result.signal, error: result.error?.message, stdout: `${index}.stdout`, stderr: `${index}.stderr` });
  if (result.error) throw result.error;
  return { ...result, stdout: readFileSync(stdoutPath, 'utf8'), stderr: readFileSync(stderrPath, 'utf8') };
}
function read(...args) {
  const result = run(args);
  assert(!result.status && !result.signal, `message ${args.join(' ')} failed: ${result.stderr}`);
  return JSON.parse(result.stdout);
}
function refused(code, ...args) {
  const result = run(args);
  assert(result.status, `${code} was expected and message ${args.join(' ')} succeeded`);
  assert.equal(JSON.parse(result.stderr).error.code, code, `message ${args.join(' ')} was refused otherwise`);
}
// The listing written with `--csv` to a new file of this run, read back.
function table(...filters) {
  const path = join(output, `messages-${randomUUID()}.csv`);
  const done = read('list', '--mailbox', values.mailbox, ...filters, '--csv', path);
  const bytes = readFileSync(path);
  assert.equal(done.size_bytes, bytes.length, '--csv printed another size than it wrote');
  report.tables.push({ filters, path, sha256: hash(bytes) });
  return { done, path, ...readTable(bytes.toString('utf8')) };
}
// A field the message holds is written as it is; one it does not is empty.
function sameField(row, name, stored, id) {
  if (stored === null || stored === undefined) assert.equal(row[name], '', `--csv wrote a ${name} ${id} does not hold`);
  else assert.equal(row[name], String(stored), `--csv wrote ${name} of ${id} otherwise`);
}
const HEADER = ['id', 'mailbox_id', 'received_at', 'sent_at', 'sender', 'reply_to', 'recipients', 'cc', 'subject',
  'message_id', 'in_reply_to', 'list_id', 'snippet', 'attachments', 'tags', 'note', 'snoozed_until'];
let failure;
try {
  const listed = read('list', '--mailbox', values.mailbox);
  assert(listed.length, 'The mailbox holds no stored mail; sync it first');
  const whole = table();
  assert.equal(whole.done.messages, listed.length, '--csv counted other messages than the listing holds');
  assert.deepEqual(whole.header, HEADER, `--csv wrote another header: ${whole.header.join(',')}`);
  // Every row is the listed message in the listing's order, with its fields.
  assert.deepEqual(whole.records.map(row => row.id), listed.map(message => message.id),
    '--csv rows name other messages, or in another order');
  for (const [row, message] of whole.records.map((row, at) => [row, listed[at]])) {
    for (const name of ['mailbox_id', 'received_at', 'sent_at', 'sender', 'reply_to', 'recipients', 'cc',
      'subject', 'message_id', 'in_reply_to', 'snippet', 'note', 'snoozed_until']) {
      sameField(row, name, message[name], message.id);
    }
    sameField(row, 'list_id', message.mailing_list?.list_id, message.id);
    assert.equal(row.tags, message.tags.join('; '), `--csv wrote the tags of ${message.id} otherwise`);
    if (message.attachments === null || message.attachments === undefined) {
      assert.equal(row.attachments, '', `--csv wrote files ${message.id} does not record`);
    } else {
      const files = message.attachments.map(file => [file.filename, file.content_type].find(name => typeof name === 'string'));
      assert.equal(row.attachments, files.join('; '), `--csv wrote the files of ${message.id} otherwise`);
    }
  }
  // The listing's filters narrow the table as they narrow the listing.
  const [newest] = listed;
  const narrowed = table('--since', newest.received_at, '--oldest-first');
  assert.deepEqual(narrowed.records.map(row => row.id),
    read('list', '--mailbox', values.mailbox, '--since', newest.received_at, '--oldest-first').map(message => message.id),
    '--csv with --since --oldest-first lists otherwise than the listing');
  assert.deepEqual(table('--contains', randomUUID()).records, [], 'A text no message holds still wrote rows');
  // A path that exists is refused and left as it was; a path under a missing
  // directory is refused and not created; a blank filter is refused.
  const before = readFileSync(whole.path, 'utf8');
  refused('MESSAGE_OUTPUT_EXISTS', 'list', '--mailbox', values.mailbox, '--csv', whole.path);
  assert.equal(readFileSync(whole.path, 'utf8'), before, 'The refused path was changed');
  const missing = join(output, randomUUID(), 'messages.csv');
  refused('MESSAGE_OUTPUT_INVALID', 'list', '--mailbox', values.mailbox, '--csv', missing);
  assert(!existsSync(missing), 'A refused --csv path was written');
  refused('MESSAGE_FILTER_INVALID', 'list', '--mailbox', values.mailbox, '--contains', ' ', '--csv', join(output, 'blank.csv'));
  report.observed = { listed: listed.length, narrowed: narrowed.records.length };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, '  ')}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
