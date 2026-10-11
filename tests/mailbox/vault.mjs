#!/usr/bin/env node
// Qualifies, against a real mailbox and its real vault, that a mailbox's bound
// item is either usable or named together with the vault that does not hold it.
// Reads only: never tags, syncs, sends, deletes, or resets a cursor.
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
  bind: { type: 'string' }, binary: { type: 'string', default: 'skrzynka' },
  'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/mailbox-vault');
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
  assert(!result.status && !result.signal,
    `${binary} ${args.join(' ')} exited ${result.status ?? result.signal}: ${result.stderr}`);
  return result.stdout;
}
function command(args) {
  return JSON.parse(successful(values.binary, ['--organization', values.organization, ...args]));
}
const text = value => typeof value === 'string' && value.trim() !== '';
try {
  for (const name of ['organization', 'mailbox', 'bind', 'source-revision']) {
    assert(text(values[name]), `--${name} is required`);
  }
  report.harness_revision = successful('git', ['rev-parse', 'HEAD']).trim();
  report.test_sha256 = hash(readFileSync(fileURLToPath(import.meta.url)));
  report.source_revision = successful('git', [
    'rev-parse', '--verify', `${values['source-revision']}^{commit}`,
  ]).trim();
  report.binary_identity = JSON.parse(successful(values.binary, ['version']));
  assert.equal(report.binary_identity.product, 'skrzynka', 'Candidate identifies a different product');
  assert.equal(report.binary_identity.source, report.source_revision,
    'Candidate source identity differs from --source-revision; qualify a release built from that commit');

  const shown = command(['mailbox', 'show', values.mailbox]);
  const source = shown.credential_source;
  assert(source, 'mailbox show did not report the vault its item is looked for in');
  assert(text(source.vault) || text(source.error),
    'credential_source names neither a vault nor why the vault could not be observed');
  assert(typeof source.item_held === 'boolean' || text(source.item_error),
    'credential_source says neither whether the vault holds the bound item nor why its item list was unreadable');

  const measured = command(['account', 'connection', '--provider', 'gmail',
    '--bind', values.bind, '--email', shown.email]);
  assert.equal(measured.receiving_skarbiec_item_id, shown.skarbiec_item_id,
    'connection report measured a different item than the mailbox binds');
  assert.equal(measured.credential_source.vault, source.vault,
    'connection report and mailbox show name different vaults');
  assert.equal(measured.credential_source.item_held, source.item_held,
    'connection report and mailbox show disagree about whether the vault holds the item');
  const usable = measured.paths.some(path => path.verdict === 'usable');
  if (source.item_held === false) {
    const password = measured.paths.find(path => path.path === 'app_password');
    assert.equal(password.verdict, 'refused', 'an item the vault does not hold was not refused');
    assert(password.detail.includes(shown.skarbiec_item_id), `refusal does not name the item: ${password.detail}`);
    assert(password.detail.includes(source.vault), `refusal does not name the vault: ${password.detail}`);
  } else if (source.item_held === true) {
    assert(usable, `the vault holds ${shown.skarbiec_item_id} but no path authenticated: ${JSON.stringify(measured.paths)}`);
  }
  report.observed = { mailbox: shown.id, item: shown.skarbiec_item_id, credential_source: source,
    usable, paths: measured.paths };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  throw error;
} finally {
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
