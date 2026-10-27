#!/usr/bin/env node
// Snoozes one stored message of a dedicated real mailbox and wakes it again;
// nothing reaches the provider. Proves `message list` leaves a snoozed
// message out, --include-snoozed lists it, a past instant is refused, and
// waking lists it again.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' }, 'lead-seconds': { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/snooze');
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
const listed = (...flags) => skrzynka('message', 'list', '--mailbox', values.mailbox, ...flags)
  .map(message => message.id);
let snoozed;
let marker;
let markedId;
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
  const lead = Number(values['lead-seconds']) * second;
  const now = Date.now();
  assert(Number.isInteger(lead) && now + lead > now,
    '--lead-seconds must be a whole number of seconds that lies ahead: how long the message is snoozed');

  const [id] = listed();
  assert(id, 'The mailbox lists no message to snooze; sync it first');

  const past = run(values.binary, cli('message', 'snooze', id, '--until', new Date(now - lead).toISOString()));
  assert(past.status, 'A past instant was not refused');
  assert.equal(JSON.parse(past.stderr).error.code, 'SNOOZE_INVALID');

  const until = new Date(Date.now() + lead).toISOString();
  snoozed = id;
  const answered = skrzynka('message', 'snooze', id, '--until', until);
  assert.equal(Date.parse(answered.snoozed_until), Date.parse(until), 'The answer does not name the instant');
  assert(!listed().includes(id), 'message list still lists the snoozed message');
  assert(listed('--include-snoozed').includes(id), '--include-snoozed does not list the snoozed message');
  // --snoozed lists the snoozed messages only, each with a wake instant still ahead.
  const onlySnoozed = skrzynka('message', 'list', '--mailbox', values.mailbox, '--snoozed');
  assert(onlySnoozed.some(message => message.id === id), '--snoozed does not list the snoozed message');
  assert(onlySnoozed.every(message => Date.parse(message.snoozed_until) > Date.now()),
    '--snoozed lists a message that is not snoozed');
  assert.equal(Date.parse(skrzynka('message', 'show', id).snoozed_until), Date.parse(until));

  // A span counts from when it is read; an unknown unit and a span of zero are refused.
  const day = new Date('1970-01-02T00:00:00Z').getTime();
  for (const span of ['+2y', '+0h']) {
    const refused = run(values.binary, cli('message', 'snooze', id, '--until', span));
    assert(refused.status, `The span ${span} was not refused`);
    assert.equal(JSON.parse(refused.stderr).error.code, 'SNOOZE_INVALID');
  }
  const before = Date.now();
  const spanned = Date.parse(skrzynka('message', 'snooze', id, '--until', '+1d').snoozed_until);
  const after = Date.now();
  assert(spanned >= before + day - second && spanned <= after + day,
    `+1d snoozed until ${new Date(spanned).toISOString()}, not a day after the request`);

  const woken = skrzynka('message', 'wake', id);
  snoozed = undefined;
  assert.equal(woken.snoozed_until, null, 'The woken message still names an instant');
  assert(listed().includes(id), 'message list does not list the woken message');

  // A saved view of just this message (a freshly drawn tag) snoozes it the same way.
  marker = `qualification-${randomUUID()}`;
  markedId = id;
  skrzynka('message', 'tag', id, '--add', marker);
  skrzynka('message', 'list', '--tag', marker, '--save-view', marker);
  snoozed = id;
  const viewed = skrzynka('message', 'snooze', '--view', marker, '--until', until);
  assert.deepEqual(viewed.messages, [id], 'snooze --view snoozed other messages than the view lists');
  assert(!listed().includes(id), 'message list still lists the message its view snoozed');
  // wake --view finds the snoozed message through the view, which itself
  // leaves snoozed mail out, and lists it again; a second wake finds none.
  const woken = skrzynka('message', 'wake', '--view', marker);
  assert.deepEqual(woken.messages, [id], 'wake --view woke other messages than the view keeps snoozed');
  assert(listed().includes(id), 'message list does not list the message its view woke');
  assert.deepEqual(skrzynka('message', 'wake', '--view', marker).messages, [], 'A second wake --view woke messages again');
  snoozed = undefined;
  skrzynka('message', 'view', 'delete', marker);
  skrzynka('message', 'tag', id, '--remove', marker);
  marker = undefined;

  // --thread snoozes every message of the conversation, as `message thread`
  // lists it, and wakes them again.
  const conversation = skrzynka('message', 'thread', id).map(message => message.id).sort();
  snoozed = id;
  const threaded = skrzynka('message', 'snooze', '--thread', id, '--until', until);
  assert.deepEqual([threaded.thread, [...threaded.messages].sort()], [id, conversation],
    'snooze --thread snoozed other messages than the conversation holds');
  assert(conversation.every(member => !listed().includes(member)), 'message list still lists a snoozed message of the conversation');
  const wokenThread = skrzynka('message', 'wake', '--thread', id);
  assert.deepEqual([wokenThread.until, [...wokenThread.messages].sort()], [null, conversation],
    'wake --thread woke other messages than the conversation holds snoozed');
  snoozed = undefined;

  report.observed = { message: id, until };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (snoozed) {
    // The message's whole conversation, which covers the message itself.
    report.cleanup = run(values.binary, cli('message', 'wake', '--thread', snoozed)).status;
  }
  if (marker) {
    report.marker_cleanup = [
      run(values.binary, cli('message', 'view', 'delete', marker)).status,
      run(values.binary, cli('message', 'tag', markedId, '--remove', marker)).status,
    ];
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
