#!/usr/bin/env node
// Exports a dedicated real mailbox, restores the export into a provider
// folder the operator dedicates to this test, restores it again, and restores
// a copy with one file changed. Proves `message restore`: every file is
// accounted for, nothing is appended twice, and a changed file is refused.
// The appended messages stay in the dedicated folder.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, cpSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' }, folder: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/restore');
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
// A restore's printed report, and its refusal code when it ended refused.
function restore(directory) {
  const result = run(values.binary, cli('message', 'restore', '--mailbox', values.mailbox,
    '--input', directory, '--folder', values.folder));
  const printed = JSON.parse(result.stdout);
  if (result.status) {
    assert.equal(JSON.parse(result.stderr).error.code, 'RESTORE_INCOMPLETE');
    assert(printed.refused.length, 'RESTORE_INCOMPLETE with nothing refused');
  } else {
    assert(!printed.refused.length, 'A whole restore lists refusals');
  }
  return printed;
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
  assert(values.organization && values.mailbox && values.folder?.trim(),
    '--organization, --mailbox and --folder are required; name a dedicated real mailbox and a provider folder dedicated to this test');

  const directory = join(output, 'export');
  const exported = run(values.binary, cli('message', 'export', '--mailbox', values.mailbox, '--output', directory));
  const manifest = JSON.parse(exported.stdout);
  assert(manifest.exported.length, 'The export wrote no file to restore; sync the mailbox first');

  // Every exported file is restored or refused with a code.
  const first = restore(directory);
  const accounted = [...first.restored, ...first.refused].map(entry => entry.file).sort();
  assert.deepEqual(accounted, manifest.exported.map(entry => entry.file).sort(),
    'The restore does not account for every exported file');
  first.refused.forEach(entry => assert(entry.code && entry.message, `Refused ${entry.file} carries no code`));

  // A second restore appends nothing: every message is already in the folder.
  const second = restore(directory);
  const again = second.restored.filter(entry => entry.message_id && entry.appended);
  assert.deepEqual(again, [], `A second restore appended ${again.map(entry => entry.file).join(', ')} again`);

  // A file changed since the export is refused and never appended.
  const changed = join(output, 'changed');
  cpSync(directory, changed, { recursive: true });
  const [victim] = manifest.exported;
  writeFileSync(join(changed, victim.file), `${readFileSync(join(changed, victim.file), 'utf8')}\nchanged\n`);
  const third = restore(changed);
  const refusal = third.refused.find(entry => entry.file === victim.file);
  assert(refusal, `${victim.file} was not refused after it changed`);
  assert.equal(refusal.code, 'RESTORE_FILE_CHANGED');

  report.observed = {
    exported: manifest.exported.length,
    appended: first.restored.filter(entry => entry.appended).length,
    refused: first.refused.length,
  };
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
