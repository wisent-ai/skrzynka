#!/usr/bin/env node
// Sends one real message from a dedicated mailbox to a recipient the operator
// names, then proves the mailbox send limit: a limit equal to today's count
// refuses the next new send with MAILBOX_SEND_LIMIT_REACHED and records
// nothing for it, while repeating the first send's key still answers the first
// attempt. The mailbox's prior limit is restored at the end.
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
const outputRoot = join(root, '.build/real-tests/mailbox-limit');
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
const limit = () => skrzynka('mailbox', 'limit', values.mailbox);
const bodyFile = join(output, 'body.txt');
const sendArgs = key => cli('message', 'send', '--mailbox', values.mailbox,
  '--to', values.recipient, '--subject', `Skrzynka send limit qualification ${key}`,
  '--body-file', bodyFile, '--idempotency-key', key);
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
  writeFileSync(bodyFile, 'Sent by the Skrzynka send limit qualification.\n', { flag: 'wx' });

  prior = limit();
  const cleared = skrzynka('mailbox', 'limit', values.mailbox, '--clear');
  assert.equal(cleared.per_day, null, 'The limit still reads after --clear');
  assert.equal(cleared.remaining, null, 'A mailbox without a limit reports a remaining count');

  // One real send with no limit; it counts against today.
  const firstKey = randomUUID();
  const first = JSON.parse(successful(values.binary, sendArgs(firstKey)));
  assert.equal(first.status, 'sent', `The qualification message ended ${first.status}`);
  const counted = limit();
  assert(counted.sent_today > prior.sent_today, 'The real send was not counted against today');

  // A limit equal to today's count leaves nothing.
  const full = skrzynka('mailbox', 'limit', values.mailbox, '--per-day', String(counted.sent_today));
  assert.equal(full.per_day, counted.sent_today);
  assert(!full.remaining, `A used-up limit still reports ${full.remaining} remaining`);

  // The next new send is refused and leaves no row.
  const refusedKey = randomUUID();
  const refused = run(values.binary, sendArgs(refusedKey));
  assert(refused.status, 'A send past the limit was not refused');
  assert.equal(JSON.parse(refused.stderr).error.code, 'MAILBOX_SEND_LIMIT_REACHED');
  const rows = skrzynka('message', 'outbound', '--mailbox', values.mailbox);
  assert(!rows.some(row => row.idempotency_key === refusedKey), 'The refused send left an outbound row');

  // Repeating the first key answers the first attempt; it is not a new send.
  const repeated = JSON.parse(successful(values.binary, sendArgs(firstKey)));
  assert.equal(repeated.id, first.id, 'Repeating a key under a used-up limit did not answer the first attempt');
  assert.equal(limit().sent_today, counted.sent_today, 'Repeating a key was counted again');

  // A warm-up set today starts at its count, names its step, ceiling and day,
  // and still admits nothing more today; a ceiling not above the start is refused.
  const ceiling = String(counted.sent_today + counted.sent_today);
  const warming = skrzynka('mailbox', 'limit', values.mailbox, '--per-day', String(counted.sent_today),
    '--grow-by', String(counted.sent_today), '--up-to', ceiling);
  assert.equal(warming.per_day, counted.sent_today, 'A warm-up set today does not start at its count');
  // From n by n up to 2n takes one step: the ceiling is allowed the day after.
  const oneDay = Date.parse('1970-01-02T00:00:00Z');
  const nextDay = new Date(Date.parse(`${warming.day}T00:00:00Z`) + oneDay).toISOString().slice(Number(), 'YYYY-MM-DD'.length);
  assert.deepEqual(warming.warm_up, { from: counted.sent_today, grow_by: counted.sent_today,
    up_to: Number(ceiling), since: warming.day, reaches_up_to_on: nextDay }, 'The warm-up reads back otherwise than it was set');
  assert(!warming.remaining, 'A warm-up set today admits more than its starting count');
  const flat = run(values.binary, cli('mailbox', 'limit', values.mailbox, '--per-day', ceiling,
    '--grow-by', String(counted.sent_today), '--up-to', ceiling));
  assert.equal(JSON.parse(flat.stderr).error.code, 'MAILBOX_LIMIT_INVALID', 'A ceiling equal to the start was not refused');
  const halfWarmUp = run(values.binary, cli('mailbox', 'limit', values.mailbox, '--per-day', ceiling,
    '--grow-by', String(counted.sent_today)));
  assert(halfWarmUp.status, '--grow-by without --up-to was not refused');
  const plain = skrzynka('mailbox', 'limit', values.mailbox, '--per-day', String(counted.sent_today));
  assert.equal(plain.warm_up, null, 'Setting the limit alone kept the warm-up');

  report.observed = { prior, counted, full, warming };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (prior) {
    // A warm-up is restored from the count in force today, so it goes on as it
    // would have; one that already reached its ceiling is that ceiling alone.
    const growing = prior.warm_up && prior.per_day < prior.warm_up.up_to;
    const warmUp = growing ? ['--grow-by', String(prior.warm_up.grow_by), '--up-to', String(prior.warm_up.up_to)] : [];
    const restore = prior.per_day === null ? ['--clear'] : ['--per-day', String(prior.per_day), ...warmUp];
    const restored = run(values.binary, cli('mailbox', 'limit', values.mailbox, ...restore));
    report.restored = { exit_status: restored.status, stderr: restored.stderr };
    if (restored.status && !failure) {
      report.status = 'failed';
      failure = new Error(`The mailbox's prior limit could not be restored: ${restored.stderr}`);
      report.error = failure.message;
    }
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
