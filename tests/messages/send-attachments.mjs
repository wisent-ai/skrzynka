#!/usr/bin/env node
// Sends one real message with two files from a dedicated mailbox to a
// dedicated recipient and reads the stored row back. Proves
// `message send --attach`, the files recorded by name, type, size and
// SHA-256, and the refusals before anything is claimed.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' }, to: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/send-attachments');
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
const message = (...args) => run(values.binary, ['--organization', values.organization, 'message', ...args]);
function refusedWith(result, code, words) {
  assert(result.status, `${code} was expected and the command succeeded: ${result.stdout}`);
  const { error } = JSON.parse(result.stderr);
  assert.equal(error.code, code);
  assert(error.message.includes(words), `expected "${words}" in: ${error.message}`);
}
let failure;
try {
  report.harness_revision = successful('git', ['rev-parse', 'HEAD']).trim();
  report.test_sha256 = hash(readFileSync(fileURLToPath(import.meta.url)));
  assert(values['source-revision']?.trim(), '--source-revision is required: name the exact candidate commit');
  report.source_revision = successful('git', ['rev-parse', '--verify',
    `${values['source-revision']}^{commit}`]).trim();
  report.binary_identity = JSON.parse(successful(values.binary, ['version']));
  assert.equal(report.binary_identity.source, report.source_revision,
    'Candidate source identity differs from --source-revision; qualify a release built from that commit');
  assert(values.organization && values.mailbox && values.to,
    '--organization, --mailbox and --to are required: a dedicated mailbox and a recipient that may receive this message');

  const run_id = randomUUID();
  const body = join(output, 'body.txt');
  const text = join(output, 'notes.txt');
  const data = join(output, 'data.json');
  writeFileSync(body, `Attachment journey ${run_id}\n`);
  writeFileSync(text, `notes of ${run_id}\n`);
  writeFileSync(data, `${JSON.stringify({ run: run_id })}\n`);
  const send = (...extra) => message('send', '--mailbox', values.mailbox, '--to', values.to,
    '--subject', `Attachment journey ${run_id}`, '--body-file', body, ...extra);

  // Refused before anything is claimed.
  refusedWith(send('--attach', text, '--idempotency-key', randomUUID()), 'OUTBOUND_ATTACHMENT_INVALID', 'names no type');
  refusedWith(send('--attach', `text/plain=${join(output, 'missing.txt')}`, '--idempotency-key', randomUUID()),
    'OUTBOUND_ATTACHMENT_INVALID', 'could not be read');
  refusedWith(send('--attach', `not a type=${text}`, '--idempotency-key', randomUUID()),
    'OUTBOUND_ATTACHMENT_INVALID', 'which is not a MIME type');

  // Sent with both files, recorded by name, type, size and digest.
  const key = randomUUID();
  const sent = JSON.parse(successful(values.binary, ['--organization', values.organization, 'message', 'send',
    '--mailbox', values.mailbox, '--to', values.to, '--subject', `Attachment journey ${run_id}`,
    '--body-file', body, '--attach', `text/plain=${text}`, '--attach', `application/json=${data}`,
    '--idempotency-key', key]));
  assert.equal(sent.status, 'sent', `the message ended ${sent.status}: ${sent.error_message}`);
  assert.deepEqual(sent.attachments, [
    { filename: 'notes.txt', content_type: 'text/plain', size_bytes: readFileSync(text).length, sha256: hash(readFileSync(text)) },
    { filename: 'data.json', content_type: 'application/json', size_bytes: readFileSync(data).length, sha256: hash(readFileSync(data)) },
  ]);
  const stored = JSON.parse(successful(values.binary, ['--organization', values.organization, 'message', 'outbound', '--id', sent.id]));
  assert.deepEqual(stored.attachments, sent.attachments, 'the stored row does not hold the files that were sent');

  // The same key with different files is a different request.
  refusedWith(send('--attach', `text/plain=${text}`, '--idempotency-key', key), 'IDEMPOTENCY_KEY_REUSED', 'different outbound message');

  report.observed = { outbound: sent.id, attachments: sent.attachments };
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
