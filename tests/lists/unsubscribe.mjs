#!/usr/bin/env node
// Drives the real CLI against a dedicated mailbox whose synced mail holds one
// message from a test mailing list the operator owns (--list-message) and one
// message from no list (--plain-message). It asks that test list to stop
// sending exactly once; it never mails or posts to any other list.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { readTable } from '../tables/csv.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' },
  'list-message': { type: 'string' }, 'plain-message': { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/unsubscribe');
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
const skrzynka = (...args) => JSON.parse(successful(values.binary,
  ['--organization', values.organization, ...args]));
function refused(args, code) {
  const result = run(values.binary, ['--organization', values.organization, ...args]);
  assert(result.status, `${args.join(' ')} was not refused`);
  const { error } = JSON.parse(result.stderr);
  assert.equal(error.code, code, `${args.join(' ')} was refused for another reason: ${error.message}`);
  return error.message;
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
  assert(values.organization && values.mailbox && values['list-message'] && values['plain-message'],
    '--organization, --mailbox, --list-message and --plain-message are required: name a dedicated mailbox, a message from a test list you own, and a message from no list');

  // Listing the message's attachments records its list headers if it was
  // imported before they were recorded.
  skrzynka('message', 'attachment', 'list', values['list-message']);

  // A missing key is refused before anything is asked.
  refused(['message', 'unsubscribe', values['list-message']], 'IDEMPOTENCY_KEY_REQUIRED');

  // A message from no list names no way out.
  const none = refused(['message', 'unsubscribe', values['plain-message'],
    '--idempotency-key', randomUUID()], 'UNSUBSCRIBE_UNAVAILABLE');
  assert(none.includes(values['plain-message']), `The refusal does not name the message: ${none}`);

  // The test list is one of the mailbox's lists, with a way Skrzynka can use.
  const before = skrzynka('message', 'subscriptions', '--mailbox', values.mailbox);
  assert.equal(typeof before.messages_not_read, 'number');
  const listMessage = skrzynka('message', 'show', values['list-message']);
  assert(listMessage.mailing_list?.unsubscribe, 'The list message states no List-Unsubscribe header');
  const list = before.lists.find(entry => entry.mailbox_id === values.mailbox
    && (entry.list_id ?? entry.sender) === (listMessage.mailing_list.list_id ?? listMessage.sender));
  assert(list, 'The test list is missing from message subscriptions');
  assert(list.ways_out.some(way => way.method !== 'page'),
    'The test list names only pages; use a list with a one-click or mailto way out');

  // One request; the same key returns the same attempt instead of asking again.
  const key = randomUUID();
  const first = skrzynka('message', 'unsubscribe', values['list-message'], '--idempotency-key', key);
  assert.equal(first.status, 'sent', `The list did not accept the request: ${first.error_code} ${first.error_message}`);
  const again = skrzynka('message', 'unsubscribe', values['list-message'], '--idempotency-key', key);
  assert.equal(again.id, first.id, 'A repeated key made a second request');
  assert.equal(again.updated_at, first.updated_at, 'A repeated key changed the first attempt');
  // --list names the same list by its List-Id, as the mailbox receives it:
  // its newest stored message is the test message, so the same key answers
  // the same attempt; a List-Id no stored mail names, and a blank one, are refused.
  assert(list.list_id, 'The test list states no List-Id; use a list that names one');
  assert.equal(list.latest_message_id, values['list-message'], '--list-message is not the newest message of its list; name the newest');
  const byList = skrzynka('message', 'unsubscribe', '--list', list.list_id.toUpperCase(), '--mailbox', values.mailbox,
    '--idempotency-key', key);
  assert.equal(byList.id, first.id, 'unsubscribe --list made another request than the one its newest message made');
  refused(['message', 'unsubscribe', '--list', `${randomUUID()}.lists.example`, '--idempotency-key', randomUUID()],
    'UNSUBSCRIBE_LIST_NOT_FOUND');
  refused(['message', 'unsubscribe', '--list', ' ', '--idempotency-key', randomUUID()], 'UNSUBSCRIBE_LIST_INVALID');

  // The list shows the request as its last.
  const after = skrzynka('message', 'subscriptions', '--mailbox', values.mailbox);
  const shown = after.lists.find(entry => entry.last_attempt?.id === first.id);
  assert(shown, 'message subscriptions does not show the request on its list');
  // --subscribed leaves out exactly the lists a request to leave ended `sent` for.
  const still = skrzynka('message', 'subscriptions', '--mailbox', values.mailbox, '--subscribed');
  const left = list => list.last_attempt && list.last_attempt.status === 'sent';
  assert.deepEqual(still.lists.map(list => list.latest_message_id),
    after.lists.filter(list => !left(list)).map(list => list.latest_message_id),
    '--subscribed kept other lists than those not left');
  // --contains finds the test list by its List-Id (or sender), case aside,
  // keeps only matching lists, and a text no list holds keeps none.
  const named = (list.list_id ?? list.sender).toUpperCase();
  const found = skrzynka('message', 'subscriptions', '--mailbox', values.mailbox, '--contains', named);
  assert(found.lists.some(entry => entry.latest_message_id === list.latest_message_id),
    '--contains does not find the test list by its own name');
  const holds = entry => [entry.list_id, entry.sender].filter(field => typeof field === 'string')
    .some(field => field.toUpperCase().includes(named));
  assert(found.lists.every(holds), '--contains kept a list that does not hold the text');
  assert.deepEqual(skrzynka('message', 'subscriptions', '--contains', randomUUID()).lists, [],
    '--contains kept a list for a text no list holds');
  const blank = run(values.binary, ['--organization', values.organization, 'message', 'subscriptions', '--contains', ' ']);
  assert.equal(JSON.parse(blank.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'A blank --contains was not refused');
  // --since and --until at the test list's newest message split the lists
  // there: since keeps it and every list as new, until leaves it out; a
  // window whose start is not before its end is refused.
  const at = list.latest_received_at;
  const recent = skrzynka('message', 'subscriptions', '--mailbox', values.mailbox, '--since', at).lists;
  assert(recent.some(entry => entry.latest_message_id === list.latest_message_id), '--since its newest message left the test list out');
  assert(recent.every(entry => Date.parse(entry.latest_received_at) >= Date.parse(at)), '--since kept an older list');
  const quiet = skrzynka('message', 'subscriptions', '--mailbox', values.mailbox, '--until', at).lists;
  assert(quiet.every(entry => Date.parse(entry.latest_received_at) < Date.parse(at)), '--until kept a list as new as the bound');
  assert.deepEqual([...recent, ...quiet].map(entry => entry.latest_message_id).sort(),
    after.lists.map(entry => entry.latest_message_id).sort(), '--since and --until do not split the lists between them');
  const backwards = run(values.binary, ['--organization', values.organization, 'message', 'subscriptions', '--since', at, '--until', at]);
  assert.equal(JSON.parse(backwards.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'A backwards window was not refused');
  // --leavable keeps exactly the lists naming a way out other than a page,
  // the test list among them.
  const leavable = skrzynka('message', 'subscriptions', '--mailbox', values.mailbox, '--leavable').lists;
  assert.deepEqual(leavable.map(entry => entry.latest_message_id),
    after.lists.filter(entry => entry.ways_out.some(way => way.method !== 'page')).map(entry => entry.latest_message_id),
    '--leavable kept other lists than those Skrzynka can leave itself');
  // --leave asks only a narrowed listing: narrowed by no text or window it is
  // UNSUBSCRIBE_LIST_INVALID, narrowed to a text no list holds
  // UNSUBSCRIBE_LIST_NOT_FOUND, and without a key it is refused by Clap; none
  // of them asks any list.
  refused(['message', 'subscriptions', '--mailbox', values.mailbox, '--leave', '--idempotency-key', randomUUID()],
    'UNSUBSCRIBE_LIST_INVALID');
  refused(['message', 'subscriptions', '--contains', randomUUID(), '--leave', '--idempotency-key', randomUUID()],
    'UNSUBSCRIBE_LIST_NOT_FOUND');
  const keyless = run(values.binary, ['--organization', values.organization, 'message', 'subscriptions', '--contains', named, '--leave']);
  assert(keyless.status && !keyless.stdout, 'subscriptions --leave without a key was not refused');
  assert.deepEqual(skrzynka('message', 'subscriptions', '--mailbox', values.mailbox).lists.map(entry => entry.last_attempt?.id),
    after.lists.map(entry => entry.last_attempt?.id), 'A refused --leave asked a list');
  // --csv writes the lists kept as a table, one row per list in the listing's
  // order, the test list's row naming its last request's status; the same
  // path again is refused and left as it was.
  const listTable = join(output, 'subscriptions.csv');
  const listed = skrzynka('message', 'subscriptions', '--mailbox', values.mailbox).lists;
  assert.equal(skrzynka('message', 'subscriptions', '--mailbox', values.mailbox, '--csv', listTable).lists,
    listed.length, 'subscriptions --csv counted other lists than the listing');
  const writtenLists = readFileSync(listTable, 'utf8');
  const { records: listRows } = readTable(writtenLists);
  assert.deepEqual(listRows.map(row => [row.latest_message_id, row.messages]),
    listed.map(entry => [entry.latest_message_id, String(entry.messages)]), 'subscriptions --csv rows differ from the listing');
  const testRow = listRows.find(row => row.latest_message_id === list.latest_message_id);
  assert.equal(testRow.last_attempt_status, first.status, 'subscriptions --csv wrote another last request for the test list');
  const runRefused = run(values.binary, ['--organization', values.organization, 'message', 'subscriptions', '--csv', listTable]);
  assert.equal(JSON.parse(runRefused.stderr).error.code, 'SUBSCRIPTION_OUTPUT_EXISTS', 'An existing --csv path was not refused');
  assert.equal(readFileSync(listTable, 'utf8'), writtenLists, 'The refused --csv path was changed');

  report.observed = { attempt: first, lists: after.lists.length };
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
