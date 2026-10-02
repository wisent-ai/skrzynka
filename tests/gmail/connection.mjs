#!/usr/bin/env node
// Read-only qualification against the installed product, real vault and Gmail.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, email: { type: 'string' },
  bind: { type: 'string' }, binary: { type: 'string', default: 'skrzynka' },
  'usable-path': { type: 'string' },
} });
for (const name of ['organization', 'email', 'bind', 'usable-path']) {
  if (!values[name]) throw new Error(`--${name} is required; use a real connected account`);
}
const paths = ['app_password', 'oauth', 'delegation'];
assert(['app_password', 'delegation'].includes(values['usable-path']),
  '--usable-path must be app_password or delegation; an OAuth redirect probe cannot prove authentication');
const outputRoot = join(root, '.build/real-tests/gmail-connection');
mkdirSync(outputRoot, { recursive: true });
const output = mkdtempSync(join(outputRoot, 'run-'));
const report = { started_at: new Date().toISOString(), commands: [], status: 'running' };
const hash = data => createHash('sha256').update(data).digest('hex');
function run(binary, args) {
  const result = spawnSync(binary, args, { cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
  const index = report.commands.length;
  writeFileSync(join(output, `${index}.stdout`), result.stdout ?? '');
  writeFileSync(join(output, `${index}.stderr`), result.stderr ?? '');
  report.commands.push({ argv: [binary, ...args], exit_status: result.status,
    signal: result.signal, error: result.error?.message,
    stdout: `${index}.stdout`, stderr: `${index}.stderr` });
  if (result.error) throw result.error;
  return result;
}
function successful(binary, args) {
  const result = run(binary, args);
  assert.equal(result.status, 0, `${binary} ${args.join(' ')} failed: ${result.stderr}`);
  return result.stdout;
}
function command(args) {
  return JSON.parse(successful(values.binary, ['--organization', values.organization, ...args]));
}
function inventory(status) {
  for (const field of ['mailbox_count', 'enabled_mailbox_count', 'message_count', 'schema_version']) {
    assert(Number.isSafeInteger(status[field]) && status[field] >= 0, `Invalid persisted ${field}`);
  }
  return { mailbox_count: status.mailbox_count, enabled_mailbox_count: status.enabled_mailbox_count,
    message_count: status.message_count, schema_version: status.schema_version };
}
try {
  report.source_revision = successful('git', ['rev-parse', 'HEAD']).trim();
  report.source_diff = successful('git', ['diff', '--', 'src', 'Cargo.toml', 'Cargo.lock']);
  report.test_sha256 = hash(readFileSync(fileURLToPath(import.meta.url)));
  const binaryPath = values.binary.includes('/') ? resolve(values.binary)
    : successful('/usr/bin/which', [values.binary]).trim();
  report.binary_path = binaryPath;
  report.binary_sha256 = hash(readFileSync(binaryPath));
  report.version = successful(values.binary, ['--version']).trim();
  // Preflight the actual installed command before contacting a provider.
  successful(values.binary, ['account', 'connection', '--help']);
  const before = command(['status']);
  const measured = command(['account', 'connection', '--provider', 'gmail',
    '--bind', values.bind, '--email', values.email]);
  assert.equal(measured.account, values.email.trim());
  assert.deepEqual(measured.paths.map(path => path.path), paths);
  assert.equal(measured.usable_paths, measured.paths.filter(path => path.verdict === 'usable').length);
  const working = measured.paths.find(path => path.path === values['usable-path']);
  assert.equal(working.verdict, 'usable', `Real authentication was not proved: ${JSON.stringify(working)}`);
  for (const path of measured.paths) {
    if (path.action.startsWith('skrzynka ')) {
      assert(path.action.includes(`--organization ${values.organization} `),
        `${path.path} guidance lost the requesting organization`);
    }
    if (path.path === 'oauth' && path.verdict === 'unproven' && path.observed.redirect_uri) {
      assert(path.action.includes('--bind '), 'OAuth guidance omitted the required callback address');
      assert(path.action.includes(path.observed.redirect_uri), 'OAuth guidance lost the measured callback');
    }
  }
  const refused = run(values.binary, ['--organization', values.organization,
    'account', 'connection', '--provider', 'gmail', '--bind', '0.0.0.0:8790', '--email', values.email]);
  assert.equal(refused.status, 1, 'Non-loopback callback was not refused');
  assert.match(refused.stderr, /NON_LOOPBACK_BIND_REFUSED/);
  const after = command(['status']);
  assert.deepEqual(inventory(after), inventory(before), 'Readiness changed the persisted inbox inventory');
  report.observed = { before: inventory(before), connection: measured, after: inventory(after) };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  process.exitCode = 1;
} finally {
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
