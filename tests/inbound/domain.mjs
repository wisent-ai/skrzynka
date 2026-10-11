#!/usr/bin/env node
// Real inbound-domain journey: declares the domain of an address that has
// received mail at the receiving provider, reads that address's mail through
// Skrzynka (wisent-integrations with Skrzynka's own grant), checks it is
// stored once, and checks the refusals. A domain this run declared is removed
// at the end; one the organization already had is left as it was.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, recipient: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/inbound-domain');
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
const cli = args => ['--organization', values.organization, ...args];
function json(args) {
  const result = run(values.binary, cli(args));
  assert(!result.status && !result.signal, `skrzynka ${args.join(' ')} failed: ${result.stderr}`);
  return JSON.parse(result.stdout);
}
function refused(args, code) {
  const result = run(values.binary, cli(args));
  assert(result.status, `skrzynka ${args.join(' ')} was not refused`);
  assert.equal(JSON.parse(result.stderr).error.code, code, `skrzynka ${args.join(' ')}: ${result.stderr}`);
}
const ids = messages => messages.map(message => message.id);
let failure;
let declaredHere;
try {
  report.harness_revision = run('git', ['rev-parse', 'HEAD']).stdout.trim();
  report.test_sha256 = hash(readFileSync(fileURLToPath(import.meta.url)));
  assert(values['source-revision']?.trim(), '--source-revision is required: name the exact candidate commit');
  report.source_revision = run('git', ['rev-parse', '--verify', `${values['source-revision']}^{commit}`]).stdout.trim();
  report.binary_identity = JSON.parse(run(values.binary, ['version']).stdout);
  assert.equal(report.binary_identity.source, report.source_revision,
    'Candidate source identity differs from --source-revision; qualify a release built from that commit');
  assert(values.organization && values.recipient,
    '--organization and --recipient are required; name an address at the inbound domain that has received mail');
  const recipient = values.recipient.trim().toLowerCase();
  const parsed = /@([^@]+)$/.exec(recipient);
  assert(parsed, `--recipient ${recipient} is not a mail address`);
  const [, domain] = parsed;

  const before = json(['mailbox', 'list', '--inbound']).find(row => row.domain === domain);
  const declared = json(['mailbox', 'declare', '--inbound-domain', domain]);
  assert.equal(declared.domain, domain, 'Declaring answered another domain');
  if (!before) declaredHere = declared.id;
  assert.equal(json(['mailbox', 'declare', '--inbound-domain', domain.toUpperCase()]).id, declared.id,
    'Declaring the same domain again made a second declaration');

  // Reading the address reads the feed first, then lists its stored mail.
  const received = json(['message', 'list', '--recipient', recipient]);
  assert(received.length, `${recipient} received no mail at the receiving provider; send it one first`);
  for (const message of received) {
    assert(message.recipients.toLowerCase().includes(recipient), `${message.id} is not addressed to ${recipient}`);
    assert.equal(message.domain_id, declared.id);
  }
  const read = json(['mailbox', 'list', '--inbound']).find(row => row.id === declared.id);
  assert(read.last_sync_at, 'The read was not recorded on the domain');
  assert.equal(read.last_error_code, null, `The read recorded a failure: ${read.last_error_message}`);

  // A second read stores nothing again and answers the same messages.
  const repeated = json(['sync', '--inbound-domain', declared.id]);
  assert(!repeated.received, 'A second read stored messages that were already stored');
  assert.deepEqual(ids(json(['message', 'list', '--recipient', recipient])), ids(received),
    'A second read duplicated or lost a message');
  const [newest] = received;
  const fromSender = json(['message', 'list', '--recipient', recipient, '--sender', newest.sender]);
  assert(ids(fromSender).includes(newest.id), 'The sender filter dropped the newest message');

  refused(['mailbox', 'declare', '--inbound-domain', 'not a domain'], 'INBOUND_DOMAIN_INVALID');
  refused(['sync', '--inbound-domain', randomUUID()], 'NOT_FOUND');
  refused(['mailbox', 'remove', '--inbound', declared.id], 'CONFIRMATION_REQUIRED');
  report.observed = { domain, recipient, received: received.length, declared_here: Boolean(declaredHere) };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (declaredHere) {
    run(values.binary, cli(['mailbox', 'remove', '--inbound', declaredHere, '--confirm']));
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
