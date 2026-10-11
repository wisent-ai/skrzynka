#!/usr/bin/env node
// Reads a dedicated real mailbox that already holds synced mail; never sends,
// tags, deletes or moves a cursor. Proves `message list --recipient --sender`
// against the messages the mailbox really received.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/message-filter');
mkdirSync(outputRoot, { recursive: true });
const output = mkdtempSync(join(outputRoot, 'run-'));
const report = { started_at: new Date().toISOString(), commands: [], status: 'running' };
const hash = data => createHash('sha256').update(data).digest('hex');
function run(binary, args) {
  const index = report.commands.length;
  const stdoutPath = join(output, `${index}.stdout`);
  const stderrPath = join(output, `${index}.stderr`);
  const stdout = openSync(stdoutPath, 'wx');
  const stderr = openSync(stderrPath, 'wx');
  let result;
  try {
    result = spawnSync(binary, args, { cwd: root, stdio: ['ignore', stdout, stderr] });
  } finally {
    closeSync(stdout);
    closeSync(stderr);
  }
  report.commands.push({ argv: [binary, ...args], exit_status: result.status,
    signal: result.signal, error: result.error?.message,
    stdout: `${index}.stdout`, stderr: `${index}.stderr` });
  if (result.error) throw result.error;
  return { ...result, stdout: readFileSync(stdoutPath, 'utf8'), stderr: readFileSync(stderrPath, 'utf8') };
}
function successful(binary, args) {
  const result = run(binary, args);
  assert(!result.status && !result.signal, `${binary} ${args.join(' ')} failed: ${result.stderr}`);
  return result.stdout;
}
const list = (...filters) => JSON.parse(successful(values.binary, ['--organization',
  values.organization, 'message', 'list', '--mailbox', values.mailbox, ...filters]));
// One To entry's address: the bracketed part of `Name <address>`, or the entry.
function bareAddress(entry) {
  const bracketed = /<([^>]+)>/.exec(entry);
  if (!bracketed) return entry.trim().toLowerCase();
  const [, inner] = bracketed;
  return inner.trim().toLowerCase();
}
const bareAddresses = header => header.split(',').map(bareAddress).filter(Boolean);
const ids = messages => messages.map(message => message.id);
let failure;
try {
  report.harness_revision = successful('git', ['rev-parse', 'HEAD']).trim();
  report.test_sha256 = hash(readFileSync(fileURLToPath(import.meta.url)));
  assert(values['source-revision']?.trim(), '--source-revision is required: name the exact candidate commit');
  report.source_revision = successful('git', ['rev-parse', '--verify',
    `${values['source-revision']}^{commit}`]).trim();
  report.binary_identity = JSON.parse(successful(values.binary, ['version']));
  assert.equal(report.binary_identity.product, 'skrzynka', 'Candidate identifies a different product');
  assert.equal(report.binary_identity.source, report.source_revision,
    'Candidate source identity differs from --source-revision; qualify a release built from that commit');
  assert(values.organization && values.mailbox,
    '--organization and --mailbox are required; name a dedicated real mailbox that has synced mail');

  const every = list();
  const [newest] = every;
  assert(newest, 'The mailbox holds no synced message; sync real mail into it first');
  const [address] = bareAddresses(newest.recipients);
  assert(address, `The newest message names no recipient: ${newest.recipients}`);

  // A recipient keeps exactly the messages addressed to it, in order, without case.
  const addressed = list('--recipient', address);
  const expected = every.filter(message => bareAddresses(message.recipients).includes(address));
  assert.deepEqual(ids(addressed), ids(expected), 'The recipient filter skipped, added or reordered a message');
  assert(ids(addressed).includes(newest.id), `${address} did not find the newest message`);
  assert.deepEqual(ids(list('--recipient', address.toUpperCase())), ids(addressed),
    'The recipient filter depends on case');

  // An address that merely ends in the recipient's is a different recipient.
  const longer = `x${address}`;
  assert(!ids(list('--recipient', longer)).includes(newest.id), `${longer} matched mail addressed to ${address}`);

  // A sender part keeps the messages whose From header contains it.
  const senderPart = newest.sender.toLowerCase();
  const fromSender = list('--recipient', address, '--sender', senderPart);
  assert(ids(fromSender).includes(newest.id), 'The sender filter dropped the newest message');
  for (const message of fromSender) assert(message.sender.toLowerCase().includes(senderPart));

  // The page is cut after the filters: the second match is the one-message
  // page that skips the first match.
  const [first, second] = addressed;
  if (second) {
    const position = String(addressed.indexOf(second));
    assert.deepEqual(ids(list('--recipient', address, '--limit', position, '--offset', position)),
      [second.id], `The page was cut before the filter (first match ${first.id})`);
  }

  // A blank filter is refused with its cause, never read as no filter.
  for (const flag of ['--recipient', '--sender']) {
    const refused = run(values.binary, ['--organization', values.organization,
      'message', 'list', '--mailbox', values.mailbox, flag, ' ']);
    assert(refused.status, `A blank ${flag} was not refused`);
    assert.equal(JSON.parse(refused.stderr).error.code, 'MESSAGE_FILTER_INVALID');
  }
  report.observed = { address, matching: addressed.length, total: every.length };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
