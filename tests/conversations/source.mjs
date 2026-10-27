#!/usr/bin/env node
// Reads a dedicated real mailbox; sends and changes nothing at the provider.
// Proves `message source` saves the provider's original of a stored message:
// the file is the bytes the command reports, carries the stored Message-ID,
// is never overwritten, and an unknown message is refused.
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
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/message-source');
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
// The Message-ID header of a saved original, as written.
function statedMessageId(bytes) {
  const line = bytes.toString('latin1').split(/\r?\n/)
    .find(candidate => candidate.toLowerCase().startsWith('message-id:'));
  assert(line, 'The saved original carries no Message-ID header');
  return line.slice('message-id:'.length).trim();
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
  assert(values.organization && values.mailbox,
    '--organization and --mailbox are required; name a dedicated real mailbox holding stored mail');

  const [newest] = skrzynka('message', 'list', '--mailbox', values.mailbox);
  assert(newest, 'The mailbox holds no stored message; sync it first');
  const path = join(output, 'original.eml');
  const saved = skrzynka('message', 'source', newest.id, '--output', path);
  const bytes = readFileSync(path);
  assert.equal(saved.message_id, newest.id);
  assert.equal(saved.size_bytes, bytes.length, 'The reported size differs from the file');
  assert.equal(saved.sha256, hash(bytes), 'The reported SHA-256 differs from the file');
  if (newest.message_id) {
    assert.equal(statedMessageId(bytes), newest.message_id.trim(), 'The saved original is another message');
  }

  // Saving again never overwrites the file.
  const again = run(values.binary, cli('message', 'source', newest.id, '--output', path));
  assert(again.status, 'Saving to an existing path was not refused');
  assert.equal(JSON.parse(again.stderr).error.code, 'MESSAGE_SOURCE_OUTPUT_EXISTS');
  assert.equal(hash(readFileSync(path)), saved.sha256, 'The refused save changed the file');

  const missing = run(values.binary, cli('message', 'source', randomUUID(),
    '--output', join(output, 'missing.eml')));
  assert(missing.status, 'An unknown message was not refused');
  assert.equal(JSON.parse(missing.stderr).error.code, 'NOT_FOUND');

  report.observed = { saved };
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
