#!/usr/bin/env node
// Sends one real message from a dedicated mailbox to two other declared
// mailboxes, one of them as a blind copy, then reads both received copies.
// Proves `message send --bcc`: the row records the blind copy, both mailboxes
// receive the message, and neither copy names the blind address in a header.
// It syncs the receiving mailboxes until the message arrives; with no
// delivery it does not end.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' },
  'to-mailbox': { type: 'string' }, 'bcc-mailbox': { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/send-bcc');
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
const skrzynka = (...args) => JSON.parse(successful(values.binary, ['--organization', values.organization, ...args]));
// Sync a receiving mailbox until a message with this subject is stored there.
function received(mailbox, subject) {
  for (;;) {
    skrzynka('sync', '--mailbox', mailbox);
    const [found] = skrzynka('message', 'list', '--mailbox', mailbox, '--contains', subject);
    if (found) return found;
  }
}
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
  assert(values.organization && values.mailbox && values['to-mailbox'] && values['bcc-mailbox'],
    '--organization, --mailbox, --to-mailbox and --bcc-mailbox are required: a sending mailbox and two other declared mailboxes that receive its mail');

  const to = skrzynka('mailbox', 'show', values['to-mailbox']).email;
  const blind = skrzynka('mailbox', 'show', values['bcc-mailbox']).email;
  const subject = `Skrzynka bcc qualification ${randomUUID()}`;
  const body = join(output, 'body.txt');
  writeFileSync(body, 'Sent with a blind copy by the Skrzynka qualification.\n', { flag: 'wx' });
  const sent = skrzynka('message', 'send', '--mailbox', values.mailbox, '--to', to, '--bcc', blind,
    '--subject', subject, '--body-file', body, '--idempotency-key', randomUUID());
  assert.equal(sent.status, 'sent', `The send ended ${sent.status}: ${sent.error_message}`);
  assert.equal(sent.bcc, blind, 'The outbound row does not record the blind copy');
  assert.equal(sent.cc, null, 'The blind copy was recorded as a visible copy');
  const listed = skrzynka('message', 'outbound', '--mailbox', values.mailbox, '--to', blind);
  assert(listed.some(row => row.id === sent.id), 'outbound --to does not find the message by its blind copy');

  // Both copies arrive; neither names the blind address in a header.
  const lower = text => (text ? text.toLowerCase() : '');
  for (const mailbox of [values['to-mailbox'], values['bcc-mailbox']]) {
    const copy = received(mailbox, subject);
    assert(!lower(copy.recipients).includes(lower(blind)) && !lower(copy.cc).includes(lower(blind)),
      `The copy in ${mailbox} names the blind address in To or Cc`);
    const source = join(output, `${mailbox}.eml`);
    skrzynka('message', 'source', copy.id, '--output', source);
    assert(!/^bcc:/im.test(readFileSync(source, 'utf8').split(/\r?\n\r?\n/)[0]),
      `The copy in ${mailbox} carries a Bcc header`);
  }

  report.observed = { outbound: sent.id, to, blind };
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
