// Read-only real mailbox and vault provenance; use an isolated, quiescent mailbox.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  binary: { type: 'string' }, skarbiec: { type: 'string' },
  organization: { type: 'string' }, mailbox: { type: 'string' },
  'source-revision': { type: 'string' }, 'expected-route': { type: 'string' },
} });
for (const name of Object.keys(values)) assert.ok(values[name]?.trim(), `--${name} cannot be empty`);
for (const name of ['binary', 'skarbiec', 'organization', 'mailbox', 'source-revision', 'expected-route']) {
  assert.ok(values[name], `--${name} is required`);
}
assert.equal(process.env.SKRZYNKA_CREDENTIALS_FILE, undefined,
  'This journey requires the selected real Skarbiec, not a credentials-file override');
const binary = resolve(values.binary);
const vaultBinary = resolve(values.skarbiec);
const runs = join(root, '.build/real-tests/mailbox-provenance');
mkdirSync(runs, { recursive: true });
const output = mkdtempSync(join(runs, 'run-'));
const hash = (data) => createHash('sha256').update(data).digest('hex');
const report = { commands: [], status: 'failed',
  binarySha256: hash(readFileSync(binary)), vaultBinarySha256: hash(readFileSync(vaultBinary)),
  testSha256: hash(readFileSync(fileURLToPath(import.meta.url))) };
function run(program, args) {
  const result = spawnSync(program, args, { cwd: root, encoding: 'utf8' });
  report.commands.push({ program, args, status: result.status, signal: result.signal,
    stdout: result.stdout, stderr: result.stderr,
    error: result.error && { code: result.error.code, errno: result.error.errno, message: result.error.message } });
  return result;
}
function successful(program, args) {
  const result = run(program, args);
  assert.ifError(result.error);
  assert.equal(result.signal, null);
  assert.ok(result.status !== null && !result.status, result.stderr);
  return result.stdout;
}
function mailbox(skarbiec) {
  return JSON.parse(successful(binary, ['--organization', values.organization,
    '--skarbiec-bin', skarbiec, 'mailbox', 'show', values.mailbox]));
}
function persisted(value) {
  const { credential_source, recovery_command, ...stored } = value;
  return stored;
}
try {
  report.sourceRevision = successful('git', ['rev-parse', '--verify',
    `${values['source-revision']}^{commit}`]).trim();
  report.binaryIdentity = JSON.parse(successful(binary, ['version']));
  assert.equal(report.binaryIdentity.product, 'skrzynka');
  assert.equal(report.binaryIdentity.source, report.sourceRevision,
    'Candidate must identify the requested source revision');
  const status = JSON.parse(successful(vaultBinary, ['status']));
  assert.equal(status.source.route, values['expected-route']);
  assert.equal(typeof status.vault, 'string');
  assert.ok(status.vault.trim(), 'Real vault has not supplied an identity');
  const before = mailbox(vaultBinary);
  assert.equal(before.organization_id, values.organization);
  assert.equal(before.id, values.mailbox, 'Use the actual mailbox UUID');
  const inventory = JSON.parse(successful(vaultBinary, ['list']));
  assert.ok(Array.isArray(inventory), 'Real vault inventory must be an array');
  const coordinate = before.skarbiec_item_id;
  const held = inventory.some((item) => item.id === coordinate ||
    (coordinate.startsWith('role:') && item.tags?.includes(`stado:${coordinate}`)));
  assert.ok(held, 'The declared mailbox fixture must name an item held by the actual vault');
  report.inventory = inventory;
  assert.equal(before.credential_source.error, null);
  assert.equal(before.credential_source.vault, status.vault);
  assert.equal(before.credential_source.route, status.source.route);
  assert.equal(before.credential_source.endpoint, status.source.endpoint ?? null);
  assert.equal(before.credential_source.item_held, true, 'Bound credential must exist in the real selected vault');
  assert.equal(before.credential_source.item_error, null);

  const missing = join(output, 'absent-skarbiec');
  const observed = run(missing, ['status']);
  assert.ok(observed.error, 'Expected actual OS refusal for the absent executable');
  assert.equal(typeof observed.error.errno, 'number');
  const refused = mailbox(missing);
  assert.equal(refused.credential_source.vault, null);
  assert.equal(refused.credential_source.item_held, null,
    'An unreadable inventory must not assert that the bound item is absent');
  for (const cause of [refused.credential_source.error, refused.credential_source.item_error]) {
    assert.equal(typeof cause, 'string');
    assert.ok(cause.includes(missing), `Lost selected executable: ${cause}`);
    assert.ok(cause.includes(String(Math.abs(observed.error.errno))), `Lost actual OS cause: ${cause}`);
  }
  assert.deepEqual(persisted(refused), persisted(before), 'Diagnostic refusal changed persisted mailbox state');
  const recovered = mailbox(vaultBinary);
  assert.deepEqual(persisted(recovered), persisted(before), 'Read-only recovery changed persisted mailbox state');
  assert.equal(recovered.credential_source.error, null);
  assert.equal(recovered.credential_source.item_error, null);
  assert.equal(recovered.credential_source.item_held, true);
  assert.equal(recovered.credential_source.vault, status.vault);
  report.observed = { status, before, refused, recovered };
  report.status = 'passed';
} finally {
  writeFileSync(join(output, 'report.json'), JSON.stringify(report, null, '  '));
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
