#!/usr/bin/env node
// Sends one real message from a dedicated mailbox to a recipient the
// operator names. Proves the mailbox signature is recorded on the row at
// claim time, the body stays the text alone, a repeated key answers the first
// row after the signature changed, removal reads as absent, and an empty
// signature is refused. The mailbox's prior signature is restored.
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
const outputRoot = join(root, '.build/real-tests/mailbox-signature');
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
// A new file in the run directory holding `text`.
function file(name, text) {
  const path = join(output, name);
  writeFileSync(path, text, { flag: 'wx' });
  return path;
}
let failure;
let prior;
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

  prior = skrzynka('mailbox', 'signature', values.mailbox);
  const first = `Signed by the Skrzynka qualification ${randomUUID()}`;
  const set = skrzynka('mailbox', 'signature', values.mailbox, '--file', file('first.txt', `${first}\n`));
  assert.equal(set.signature, first, 'The signature is not stored as written');

  const text = 'Sent by the Skrzynka signature qualification.';
  const sendArgs = ['message', 'send', '--mailbox', values.mailbox, '--to', values.recipient,
    '--subject', `Skrzynka signature qualification ${randomUUID()}`,
    '--body-file', file('body.txt', `${text}\n`), '--idempotency-key', randomUUID()];
  const sent = skrzynka(...sendArgs);
  assert.equal(sent.status, 'sent', `The message ended ${sent.status}: ${sent.error_message}`);
  assert.equal(sent.signature, first, 'The row does not record the signature it was claimed with');
  assert.equal(sent.body.trim(), text, 'The recorded body is not the text alone');

  // A changed signature does not change a send already claimed.
  skrzynka('mailbox', 'signature', values.mailbox, '--file', file('second.txt', `${randomUUID()}\n`));
  const repeated = skrzynka(...sendArgs);
  assert.equal(repeated.id, sent.id, 'Repeating the key after a signature change made another send');
  assert.equal(repeated.signature, first, 'The repeated request answers another signature');

  const cleared = skrzynka('mailbox', 'signature', values.mailbox, '--clear');
  assert.equal(cleared.signature, null, 'The removed signature still reads');

  const empty = run(values.binary, cli('mailbox', 'signature', values.mailbox,
    '--file', file('empty.txt', ' \n')));
  assert(empty.status, 'An empty signature was not refused');
  assert.equal(JSON.parse(empty.stderr).error.code, 'SIGNATURE_INVALID');

  report.observed = { sent: { id: sent.id, signature: sent.signature } };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (prior) {
    const restore = prior.signature === null
      ? ['--clear'] : ['--file', file('prior.txt', prior.signature)];
    const restored = run(values.binary, cli('mailbox', 'signature', values.mailbox, ...restore));
    report.restored = { exit_status: restored.status, stderr: restored.stderr };
    if (restored.status && !failure) {
      report.status = 'failed';
      failure = new Error(`The mailbox's prior signature could not be restored: ${restored.stderr}`);
      report.error = failure.message;
    }
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
