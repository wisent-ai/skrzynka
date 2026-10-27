#!/usr/bin/env node
// Forwards one real stored message from a dedicated mailbox to a recipient
// the operator names. Proves the forward is originated mail carrying the
// original whole: the row is sent as `Fwd: …` with one message/rfc822 file
// equal to what `message source` saves, a repeated key answers the same row,
// and an unknown message is refused. Then forwards a saved view listing that
// message alone, each forward claimed by <key>-<message id>, and removes the
// run's tag and view again.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' }, recipient: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/message-forward');
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
let tagged;
let viewName;
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
  assert(values.organization && values.mailbox && values.recipient,
    '--organization, --mailbox and --recipient are required; name a dedicated real mailbox and a recipient that accepts its mail');

  const [newest] = skrzynka('message', 'list', '--mailbox', values.mailbox);
  assert(newest, 'The mailbox holds no stored message; sync it first');
  const original = skrzynka('message', 'source', newest.id, '--output', join(output, 'original.eml'));

  const bodyFile = join(output, 'body.txt');
  writeFileSync(bodyFile, 'Forwarded by the Skrzynka forward qualification.\n', { flag: 'wx' });
  const ownPath = join(output, 'context.txt');
  const ownBytes = Buffer.from(`Context sent with the forward ${randomUUID()}\n`);
  writeFileSync(ownPath, ownBytes, { flag: 'wx' });
  const forwardArgs = ['message', 'forward', newest.id, '--to', values.recipient,
    '--body-file', bodyFile, '--attach', `text/plain=${ownPath}`, '--idempotency-key', randomUUID()];
  const sent = skrzynka(...forwardArgs);
  assert.equal(sent.status, 'sent', `The forward ended ${sent.status}: ${sent.error_message}`);
  assert.equal(sent.mailbox_id, newest.mailbox_id, 'The forward left from another mailbox');
  assert(sent.subject.startsWith('Fwd:'), `The forward's subject is ${sent.subject}`);
  const [attached, own, ...extra] = sent.attachments;
  assert(attached && own && !extra.length, 'The forward does not carry exactly the original and the own file');
  assert.equal(attached.content_type, 'message/rfc822');
  assert.equal(attached.size_bytes, original.size_bytes, 'The attached original differs in size');
  assert.equal(attached.sha256, original.sha256, 'The attached original differs from the provider\'s');
  assert.deepEqual(own, { filename: 'context.txt', content_type: 'text/plain',
    size_bytes: ownBytes.length, sha256: hash(ownBytes) }, 'The own file was not sent after the original as read');

  // A file a send would refuse is refused before the provider is read.
  const untyped = run(values.binary, cli('message', 'forward', newest.id, '--to', values.recipient,
    '--body-file', bodyFile, '--attach', ownPath, '--idempotency-key', randomUUID()));
  assert(untyped.status, 'A file without a type was not refused');
  assert.equal(JSON.parse(untyped.stderr).error.code, 'OUTBOUND_ATTACHMENT_INVALID');
  // The same key answers the same row; nothing is sent again.
  assert.equal(skrzynka(...forwardArgs).id, sent.id, 'Repeating the key made another send');

  const missing = run(values.binary, cli('message', 'forward', randomUUID(), '--to', values.recipient,
    '--body-file', bodyFile, '--idempotency-key', randomUUID()));
  assert(missing.status, 'An unknown message was not refused');
  assert.equal(JSON.parse(missing.stderr).error.code, 'NOT_FOUND');

  // forward --view forwards every message the view lists: a run-only tag
  // keeps this message alone, so exactly one forward is sent, claimed by
  // <key>-<message id>, and the same request again sends nothing new.
  const tag = `forward-view-${randomUUID()}`;
  skrzynka('message', 'tag', newest.id, '--add', tag);
  tagged = { id: newest.id, tag };
  viewName = `forward-view-${randomUUID()}`;
  skrzynka('message', 'list', '--tag', tag, '--save-view', viewName);
  const viewKey = randomUUID();
  const viewArgs = ['message', 'forward', '--view', viewName, '--to', values.recipient, '--body-file', bodyFile,
    '--idempotency-key', viewKey];
  const viewed = skrzynka(...viewArgs);
  assert.deepEqual([viewed.forwarded.map(row => [row.idempotency_key, row.status]), viewed.refused],
    [[[`${viewKey}-${newest.id}`, 'sent']], []], 'forward --view sent otherwise than one forward of the tagged message');
  assert.deepEqual(skrzynka(...viewArgs).forwarded.map(row => row.id), viewed.forwarded.map(row => row.id),
    'Repeating forward --view with the same key sent again');
  const unknownView = run(values.binary, cli('message', 'forward', '--view', randomUUID(), '--to', values.recipient,
    '--body-file', bodyFile, '--idempotency-key', randomUUID()));
  assert.equal(JSON.parse(unknownView.stderr).error.code, 'VIEW_NOT_FOUND', 'An unknown view was not refused');
  const both = run(values.binary, cli('message', 'forward', newest.id, '--view', viewName, '--to', values.recipient,
    '--body-file', bodyFile, '--idempotency-key', randomUUID()));
  assert(both.status, 'forward with a message and --view together was not refused');

  report.observed = { sent };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (viewName) report.view_cleanup = run(values.binary, cli('message', 'view', 'delete', viewName)).status;
  if (tagged) report.tag_cleanup = run(values.binary, cli('message', 'tag', tagged.id, '--remove', tagged.tag)).status;
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
