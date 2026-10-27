#!/usr/bin/env node
// Replies to everyone on one real stored message the operator names and
// agrees may be answered to all its recipients (--confirm-recipients).
// Proves the attempt is sent and records reply_all, and that reply_all is
// part of the idempotent request.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' }, message: { type: 'string' },
  'confirm-recipients': { type: 'boolean' }, copy: { type: 'string' }, 'blind-copy': { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/reply-all');
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
const cli = (...args) => ['--organization', values.organization, ...args];
const skrzynka = (...args) => JSON.parse(successful(values.binary, cli(...args)));
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
  assert(values.organization && values.mailbox && values.message,
    '--organization, --mailbox and --message are required: name a stored message whose To or Cc holds other addresses');
  assert(values['confirm-recipients'],
    '--confirm-recipients is required: this test sends a real reply to every To and Cc address of the message');
  assert(values.copy && values['blind-copy'],
    '--copy and --blind-copy are required: two addresses that accept a copy of the reply');

  const message = skrzynka('message', 'show', values.message);
  assert.equal(message.mailbox_id, values.mailbox, 'The message belongs to another mailbox');
  report.recipients = { to: message.recipients, cc: message.cc };

  const bodyFile = join(output, 'body.txt');
  writeFileSync(bodyFile, 'Replied to all by the Skrzynka reply-all qualification.\n', { flag: 'wx' });
  const key = randomUUID();
  const replyArgs = ['message', 'reply', values.message, '--body-file', bodyFile, '--idempotency-key', key];
  const sent = skrzynka(...replyArgs, '--all');
  assert.equal(sent.status, 'sent', `The reply ended ${sent.status}: ${sent.error_message}`);
  assert.equal(sent.reply_all, true, 'The attempt does not record reply_all');

  // reply_all is part of the request: the same key without it is another request.
  const other = run(values.binary, cli(...replyArgs));
  assert(other.status, 'The same key without --all was not refused');
  assert.equal(JSON.parse(other.stderr).error.code, 'IDEMPOTENCY_KEY_REUSED');
  assert.equal(skrzynka(...replyArgs, '--all').id, sent.id, 'Repeating the key with --all made another reply');

  // --cc and --bcc copy more addresses; the attempt records both, and the same
  // key with other copies is another request. A copy that is no address is
  // refused before anything is claimed.
  const copied = randomUUID();
  const copyArgs = ['message', 'reply', values.message, '--body-file', bodyFile, '--idempotency-key', copied,
    '--cc', values.copy, '--bcc', values['blind-copy']];
  const withCopies = skrzynka(...copyArgs);
  assert.equal(withCopies.status, 'sent', `The copied reply ended ${withCopies.status}: ${withCopies.error_message}`);
  assert.equal(withCopies.cc, values.copy.trim(), 'The attempt does not record the copy');
  assert.equal(withCopies.bcc, values['blind-copy'].trim(), 'The attempt does not record the blind copy');
  const fewer = run(values.binary, cli('message', 'reply', values.message, '--body-file', bodyFile,
    '--idempotency-key', copied, '--cc', values.copy));
  assert.equal(JSON.parse(fewer.stderr).error.code, 'IDEMPOTENCY_KEY_REUSED', 'Other copies under the same key were accepted');
  const bad = run(values.binary, cli('message', 'reply', values.message, '--body-file', bodyFile,
    '--idempotency-key', randomUUID(), '--cc', 'not an address'));
  assert.equal(JSON.parse(bad.stderr).error.code, 'OUTBOUND_RECIPIENT_INVALID');

  report.observed = { reply: sent.id };
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
