#!/usr/bin/env node
// Exports a dedicated real mailbox with its mbox, then checks the export with
// `message verify`: the untouched export verifies whole, a copy with
// one original changed, one removed and the mbox changed is reported damaged
// file by file, and a directory without a manifest is refused. Nothing is
// written to the provider.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { appendFileSync, closeSync, cpSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/verify-export');
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
  assert(values.organization && values.mailbox, '--organization and --mailbox are required; name a dedicated real mailbox');

  const directory = join(output, 'export');
  const manifest = JSON.parse(successful(values.binary, cli('message', 'export', '--mailbox', values.mailbox,
    '--output', directory, '--mbox')));
  const [victim, removed] = manifest.exported;
  assert(victim && removed, 'The export wrote fewer than two originals to damage; sync the mailbox first');
  const listed = [...manifest.exported.map(entry => entry.file), manifest.mbox.file].sort();

  // The untouched export holds every file it lists, the mbox included.
  const whole = JSON.parse(successful(values.binary, cli('message', 'verify', '--input', directory)));
  assert.deepEqual([whole.verified.sort(), whole.damaged], [listed, []], 'An untouched export was not verified whole');

  // A copy with one original changed, one removed and the mbox changed is
  // reported damaged for exactly those three, each with its code.
  const damagedCopy = join(output, 'damaged');
  cpSync(directory, damagedCopy, { recursive: true });
  appendFileSync(join(damagedCopy, victim.file), '\nchanged\n');
  rmSync(join(damagedCopy, removed.file));
  appendFileSync(join(damagedCopy, manifest.mbox.file), '\nchanged\n');
  const damaged = run(values.binary, cli('message', 'verify', '--input', damagedCopy));
  assert(damaged.status, 'A damaged export was not refused');
  assert.equal(JSON.parse(damaged.stderr).error.code, 'EXPORT_DAMAGED');
  const found = JSON.parse(damaged.stdout);
  const codes = Object.fromEntries(found.damaged.map(entry => [entry.file, entry.code]));
  assert.deepEqual(codes, {
    [victim.file]: 'EXPORT_FILE_CHANGED',
    [removed.file]: 'EXPORT_FILE_MISSING',
    [manifest.mbox.file]: 'EXPORT_FILE_CHANGED',
  }, 'message verify reported other files damaged than the three changed');
  assert.deepEqual([...found.verified, ...Object.keys(codes)].sort(), listed,
    'message verify did not account for every listed file');

  // A directory without a manifest is no export.
  const empty = join(output, 'empty');
  mkdirSync(empty);
  const refused = run(values.binary, cli('message', 'verify', '--input', empty));
  assert(refused.status, 'A directory without a manifest was not refused');
  assert.equal(JSON.parse(refused.stderr).error.code, 'EXPORT_INPUT_INVALID');

  report.observed = { listed: listed.length, damaged: found.damaged.length };
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
