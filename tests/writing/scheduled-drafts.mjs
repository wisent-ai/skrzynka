#!/usr/bin/env node
// Needs a `skrzynka serve` already running against the same fleet database.
// Schedules one complete draft on a dedicated real mailbox to a recipient the
// operator names, the operator's --lead-seconds ahead, and reads it until the
// service has sent it or recorded why not; schedules and unschedules a second
// one, which stays.
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
  'lead-seconds': { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/scheduled-drafts');
mkdirSync(outputRoot, { recursive: true });
const output = mkdtempSync(join(outputRoot, 'run-'));
const report = { started_at: new Date().toISOString(), commands: [], status: 'running' };
const hash = data => createHash('sha256').update(data).digest('hex');
const second = new Date('1970-01-01T00:00:01Z').getTime();
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
const drafts = [];
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
  assert(values.organization && values.mailbox && values.recipient,
    '--organization, --mailbox and --recipient are required; name a dedicated real mailbox and a recipient that accepts its mail');
  const lead = Number(values['lead-seconds']) * second;
  const now = Date.now();
  assert(Number.isInteger(lead) && now + lead > now,
    '--lead-seconds must be a whole number of seconds that lies ahead: how far ahead the draft is scheduled');

  const bodyFile = join(output, 'body.txt');
  writeFileSync(bodyFile, 'Sent on schedule by the Skrzynka qualification.\n', { flag: 'wx' });
  const subject = `Skrzynka scheduled draft ${randomUUID()}`;
  const draft = skrzynka('message', 'draft', 'save', '--mailbox', values.mailbox,
    '--to', values.recipient, '--subject', subject, '--body-file', bodyFile);
  drafts.push(draft.id);

  const past = run(values.binary, cli('message', 'draft', 'schedule', draft.id,
    '--at', new Date(Date.now() - lead).toISOString(), '--idempotency-key', randomUUID()));
  assert(past.status, 'A past instant was not refused');
  assert.equal(JSON.parse(past.stderr).error.code, 'DRAFT_SCHEDULE_INVALID');

  const key = randomUUID();
  const scheduled = skrzynka('message', 'draft', 'schedule', draft.id,
    '--at', new Date(Date.now() + lead).toISOString(), '--idempotency-key', key);
  assert(scheduled.send_at, 'The scheduled draft shows no send_at');
  assert.equal(scheduled.send_key, key, 'The scheduled draft shows another key');

  // The running service sends it at its time: read it until it is gone, or
  // until the service records why it was not sent.
  let shown = run(values.binary, cli('message', 'draft', 'show', draft.id));
  while (!shown.status) {
    const current = JSON.parse(shown.stdout);
    assert(!current.schedule_error, `The scheduled send failed: ${current.schedule_error}`);
    shown = run(values.binary, cli('message', 'draft', 'show', draft.id));
  }
  assert.equal(JSON.parse(shown.stderr).error.code, 'NOT_FOUND');
  drafts.pop();
  const sent = skrzynka('message', 'outbound', '--mailbox', values.mailbox)
    .find(row => row.idempotency_key === key);
  assert(sent, 'No outbound row carries the scheduled key');
  assert.equal(sent.status, 'sent', `The scheduled send ended ${sent.status}`);
  assert.equal(sent.subject, subject);

  // A schedule taken back leaves the draft, unscheduled.
  const kept = skrzynka('message', 'draft', 'save', '--mailbox', values.mailbox, '--subject', subject);
  drafts.push(kept.id);
  skrzynka('message', 'draft', 'schedule', kept.id, '--at', new Date(Date.now() + lead).toISOString(),
    '--idempotency-key', randomUUID());
  const unscheduled = skrzynka('message', 'draft', 'unschedule', kept.id);
  assert.equal(unscheduled.send_at, null, 'The unscheduled draft still shows send_at');

  // message send --at keeps the message as a draft scheduled under its key;
  // a past instant is refused and leaves no draft behind.
  const laterKey = randomUUID();
  const later = skrzynka('message', 'send', '--mailbox', values.mailbox, '--to', values.recipient,
    '--subject', subject, '--body-file', bodyFile, '--idempotency-key', laterKey,
    '--at', new Date(Date.now() + lead).toISOString());
  drafts.push(later.id);
  assert.equal(later.send_key, laterKey, 'send --at scheduled the draft under another key');
  assert(later.send_at, 'send --at left the draft unscheduled');
  // draft list --send-by keeps the scheduled drafts going out before the
  // bound: not at its own send instant, but a minute after it; an
  // unreadable bound is refused.
  const minute = Date.parse('1970-01-01T00:01:00Z');
  const goingOut = bound => skrzynka('message', 'draft', 'list', '--mailbox', values.mailbox, '--send-by', bound)
    .map(listed => listed.id);
  assert(!goingOut(later.send_at).includes(later.id), 'draft list --send-by kept a draft sent at the bound itself');
  assert(goingOut(new Date(Date.parse(later.send_at) + minute).toISOString()).includes(later.id),
    'draft list --send-by left out a draft going out before the bound');
  const badBound = run(values.binary, cli('message', 'draft', 'list', '--send-by', 'soon'));
  assert.equal(JSON.parse(badBound.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'An unreadable --send-by was not refused');
  assert.deepEqual([later.subject, later.body.trim()], [subject, readFileSync(bodyFile, 'utf8').trim()],
    'send --at kept another message than was given');
  skrzynka('message', 'draft', 'unschedule', later.id);
  const before = skrzynka('message', 'draft', 'list', '--mailbox', values.mailbox).map(listed => listed.id);
  const pastSend = run(values.binary, cli('message', 'send', '--mailbox', values.mailbox, '--to', values.recipient,
    '--subject', subject, '--body-file', bodyFile, '--idempotency-key', randomUUID(),
    '--at', new Date(Date.now() - lead).toISOString()));
  assert.equal(JSON.parse(pastSend.stderr).error.code, 'DRAFT_SCHEDULE_INVALID', 'send --at a past instant was not refused');
  assert.deepEqual(skrzynka('message', 'draft', 'list', '--mailbox', values.mailbox).map(listed => listed.id), before,
    'A refused send --at left a draft behind');

  // A draft to an address that bounced for good is not scheduled: its send
  // would be refused with nobody there. Run only when the list holds one.
  const [bouncedAddress] = skrzynka('message', 'bounced');
  if (bouncedAddress) {
    const toBounced = skrzynka('message', 'draft', 'save', '--mailbox', values.mailbox,
      '--to', bouncedAddress.address, '--subject', subject, '--body-file', bodyFile);
    drafts.push(toBounced.id);
    const refusedSchedule = run(values.binary, cli('message', 'draft', 'schedule', toBounced.id,
      '--at', new Date(Date.now() + lead).toISOString(), '--idempotency-key', randomUUID()));
    assert.equal(JSON.parse(refusedSchedule.stderr).error.code, 'OUTBOUND_RECIPIENT_BOUNCED',
      'A draft to a bounced address was scheduled');
    assert.equal(skrzynka('message', 'draft', 'show', toBounced.id).send_at, null,
      'The refused schedule left a send_at');
  }

  report.observed = { sent: sent.id };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  report.cleanup = drafts.map(id => {
    const removed = run(values.binary, cli('message', 'draft', 'delete', id));
    return { id, exit_status: removed.status, stderr: removed.stderr };
  });
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
