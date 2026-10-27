#!/usr/bin/env node
// Reads a dedicated real mailbox that already holds synced mail it replied to
// and mail it originated; never sends, tags, deletes or moves a cursor.
// Proves `message replies <ID>` and `message outbound --id <ID>` against the
// attempts and outbound rows the mailbox really has, and their refusals.
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
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/message-reads');
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
const message = (...args) => run(values.binary, ['--organization', values.organization, 'message', ...args]);
const read = (...args) => JSON.parse(successful(values.binary,
  ['--organization', values.organization, 'message', ...args]));
// A refusal the service made: the code and sentence of the one JSON error
// object on stderr.
function refusedWith(result, code, sentence) {
  assert(result.status, `${code} was expected and the command succeeded: ${result.stdout}`);
  const { error } = JSON.parse(result.stderr);
  assert.equal(error.code, code);
  assert.equal(error.message, sentence);
}
// A refusal the argument parser made before dispatch: no JSON, its own words.
function refusedByParser(result, words) {
  assert(result.status, `The parser accepted what it must refuse: ${result.stdout}`);
  assert(result.stderr.includes(words), `expected "${words}" in: ${result.stderr}`);
}
const newestFirst = rows => {
  const times = rows.map(row => row.created_at);
  return JSON.stringify(times) === JSON.stringify([...times].sort().reverse());
};
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
    '--organization and --mailbox are required; name a dedicated real mailbox that has replied to and originated mail');

  // Every message's replies are its own, newest first; the mailbox has at
  // least one message it replied to.
  const replied = [];
  for (const stored of read('list', '--mailbox', values.mailbox, '--include-blocked', '--include-snoozed', '--include-muted')) {
    const replies = read('replies', stored.id);
    for (const reply of replies) assert.equal(reply.message_id, stored.id, `Reply ${reply.id} belongs to another message`);
    assert(newestFirst(replies), `The replies of ${stored.id} are not newest first`);
    if (replies.length) replied.push({ message: stored.id, replies: replies.length });
  }
  assert(replied.length, 'No message of the mailbox has a reply attempt; reply to one with message reply first');

  // Without an ID, `replies --mailbox` lists exactly the mailbox's reply attempts,
  // newest first, and `--status` keeps those in that state; an unknown status is refused.
  const everyReply = read('replies', '--mailbox', values.mailbox);
  assert.equal(everyReply.length, replied.reduce((sum, entry) => sum + entry.replies, Number()),
    'replies --mailbox counted other attempts than the messages hold');
  assert(newestFirst(everyReply), 'replies --mailbox is not newest first');
  assert.deepEqual(read('replies', '--mailbox', values.mailbox, '--oldest-first'), [...everyReply].reverse(),
    'replies --oldest-first is not the listing turned round');
  // --csv writes the listed attempts as a table, one row per attempt in order
  // with its message and status; the same path again is refused.
  const replyTable = join(output, 'replies.csv');
  assert.equal(read('replies', '--mailbox', values.mailbox, '--csv', replyTable).replies, everyReply.length,
    'replies --csv counted other attempts than the listing');
  assert.deepEqual(readTable(readFileSync(replyTable, 'utf8')).records.map(row => [row.id, row.message_id, row.status]),
    everyReply.map(reply => [reply.id, reply.message_id, reply.status]), 'replies --csv rows differ from the listing');
  const replyAgain = message('replies', '--mailbox', values.mailbox, '--csv', replyTable);
  assert.equal(JSON.parse(replyAgain.stderr).error.code, 'REPLY_OUTPUT_EXISTS', 'An existing --csv path was not refused');
  const [latestReply] = everyReply;
  assert.deepEqual(read('replies', '--mailbox', values.mailbox, '--status', latestReply.status),
    everyReply.filter(reply => reply.status === latestReply.status), 'replies --status kept other attempts');
  const unknownStatus = message('replies', '--status', 'delivered');
  assert.equal(JSON.parse(unknownStatus.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'An unknown reply status was not refused');
  // --since and --until at the newest attempt's claim instant split the attempts there.
  const replyCut = Date.parse(latestReply.created_at);
  assert.deepEqual(read('replies', '--mailbox', values.mailbox, '--since', latestReply.created_at),
    everyReply.filter(reply => Date.parse(reply.created_at) >= replyCut), 'replies --since kept other attempts');
  assert.deepEqual(read('replies', '--mailbox', values.mailbox, '--until', latestReply.created_at),
    everyReply.filter(reply => Date.parse(reply.created_at) < replyCut), 'replies --until kept other attempts');
  const backwardsReplies = message('replies', '--since', latestReply.created_at, '--until', latestReply.created_at);
  assert.equal(JSON.parse(backwardsReplies.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'A backwards reply window was not refused');
  // --contains keeps exactly the attempts whose text holds a word of the
  // newest one's, case aside; a blank text is refused.
  const [word] = latestReply.body.split(/\s+/).filter(Boolean);
  if (word) {
    assert.deepEqual(read('replies', '--mailbox', values.mailbox, '--contains', word.toUpperCase()),
      everyReply.filter(reply => reply.body.toLowerCase().includes(word.toLowerCase())), 'replies --contains kept other attempts');
  }
  const blankText = message('replies', '--contains', ' ');
  assert.equal(JSON.parse(blankText.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'A blank reply text was not refused');
  // --resend-failed answers only failures of a narrowed listing: one narrowed
  // by nothing and a blank key are refused, a text no reply holds keeps no
  // failure (NOT_FOUND) and nothing is sent; the flag needs a key.
  refusedWith(message('replies', '--resend-failed', '--idempotency-key', randomUUID()), 'OUTBOUND_RESEND_INVALID',
    'narrow the replies by mailbox, text, file name or window first; nothing answers every failed reply again at once');
  assert.equal(JSON.parse(message('replies', '--mailbox', values.mailbox, '--resend-failed', '--idempotency-key', ' ').stderr).error.code,
    'IDEMPOTENCY_KEY_INVALID', 'A blank --resend-failed key was not refused');
  refusedWith(message('replies', '--mailbox', values.mailbox, '--contains', randomUUID(), '--resend-failed',
    '--idempotency-key', randomUUID()), 'NOT_FOUND', 'failed reply was not found');
  refusedByParser(message('replies', '--mailbox', values.mailbox, '--resend-failed'), '--idempotency-key');
  assert.equal(read('replies', '--mailbox', values.mailbox).length, everyReply.length, 'A refused --resend-failed sent a reply');
  // --attachment keeps exactly the attempts that carried a file named so: a
  // file name a reply carried, in upper case, or one nothing carries.
  const carriedNames = everyReply.flatMap(reply => reply.attachments.map(file => file.filename));
  const [fileName] = [...carriedNames, randomUUID()];
  assert.deepEqual(read('replies', '--mailbox', values.mailbox, '--attachment', fileName.toUpperCase()),
    everyReply.filter(reply => reply.attachments.some(file => file.filename.toLowerCase().includes(fileName.toLowerCase()))),
    'replies --attachment kept other attempts than those carrying the file');

  // One outbound message read by its ID is the row the listing holds.
  const outbound = read('outbound', '--mailbox', values.mailbox);
  const [newest] = outbound;
  assert(newest, 'The mailbox originated no message; send one with message send first');
  assert.deepEqual(read('outbound', '--id', newest.id), newest, 'outbound --id differs from the listed row');
  // --oldest-first turns the listing round, so the newest row comes last.
  const oldestFirst = read('outbound', '--mailbox', values.mailbox, '--oldest-first');
  assert.deepEqual(oldestFirst, [...outbound].reverse(), '--oldest-first is not the listing turned round');
  // --csv writes the listing as a table: the header, then one row per listed
  // message with its ID first, in order; the same path again is refused.
  const sentTable = join(output, 'outbound.csv');
  assert.equal(read('outbound', '--mailbox', values.mailbox, '--csv', sentTable).messages, outbound.length,
    'outbound --csv counted other messages than the listing holds');
  const sentLog = readTable(readFileSync(sentTable, 'utf8'));
  assert.equal(sentLog.header.join(','),
    'id,mailbox_id,recipients,cc,bcc,subject,status,created_at,sent_at,provider_message_id,error_code,error_message,attachments',
    'outbound --csv wrote another header');
  assert.deepEqual(sentLog.records.map(row => [row.id, row.status, row.subject]),
    outbound.map(row => [row.id, row.status, row.subject]),
    'outbound --csv rows name other messages, statuses or subjects, or in another order');
  const occupied = message('outbound', '--mailbox', values.mailbox, '--csv', sentTable);
  assert.equal(JSON.parse(occupied.stderr).error.code, 'OUTBOUND_OUTPUT_EXISTS', 'An existing --csv path was not refused');

  // Each search filter keeps exactly the listed rows it describes.
  const addressee = newest.recipients.split(',')[0].trim();
  const lower = text => text.toLowerCase();
  const addressed = row => [row.recipients, row.cc].filter(Boolean)
    .some(header => lower(header).includes(lower(addressee)));
  assert.deepEqual(read('outbound', '--mailbox', values.mailbox, '--to', addressee.toUpperCase()),
    outbound.filter(addressed), '--to differs from the rows addressed so');
  const word = newest.subject.trim().split(/\s+/)[0];
  const mentions = row => lower(row.subject).includes(lower(word)) || lower(row.body).includes(lower(word));
  assert.deepEqual(read('outbound', '--mailbox', values.mailbox, '--contains', word),
    outbound.filter(mentions), '--contains differs from the rows that mention the word');
  assert.deepEqual(read('outbound', '--mailbox', values.mailbox, '--status', newest.status),
    outbound.filter(row => row.status === newest.status), '--status differs from the rows in that status');
  // --attachment keeps the rows that carried a file by that name, case aside;
  // a mailbox that never sent a file is searched for a name nothing carries.
  const filed = outbound.find(row => row.attachments.length);
  const [file] = filed ? filed.attachments : [{ filename: randomUUID() }];
  const fileName = file.filename.toUpperCase();
  const carried = row => row.attachments.some(attached => lower(attached.filename).includes(lower(fileName)));
  assert.deepEqual(read('outbound', '--mailbox', values.mailbox, '--attachment', fileName),
    outbound.filter(carried), '--attachment differs from the rows that carried the file');
  report.attachment_searched = { name: fileName, carried_by: outbound.filter(carried).length };
  // --since/--until keep the rows claimed in the window: since the newest
  // claim keeps the rows claimed at that instant or later, until it the rest.
  const claimed = row => Date.parse(row.created_at);
  const cut = claimed(newest);
  assert.deepEqual(read('outbound', '--mailbox', values.mailbox, '--since', newest.created_at),
    outbound.filter(row => claimed(row) >= cut), '--since differs from the rows claimed from then on');
  assert.deepEqual(read('outbound', '--mailbox', values.mailbox, '--until', newest.created_at),
    outbound.filter(row => claimed(row) < cut), '--until differs from the rows claimed before then');
  // --bounced keeps exactly the rows whose Message-ID a stored report returned.
  const returned = new Set(read('list', '--bounces', '--include-blocked', '--include-snoozed', '--include-muted')
    .map(report => report.bounce.original_message_id).filter(id => typeof id === 'string'));
  assert.deepEqual(read('outbound', '--mailbox', values.mailbox, '--bounced'),
    outbound.filter(row => returned.has(row.provider_message_id)), '--bounced differs from the rows a report returned');
  // --answered keeps exactly the rows a stored message of the mailbox names in
  // In-Reply-To or References (bounce reports aside); --unanswered the rest.
  const answeredIds = new Set(read('list', '--mailbox', values.mailbox, '--include-blocked', '--include-snoozed', '--include-muted')
    .filter(stored => !stored.bounce)
    .flatMap(stored => [stored.in_reply_to, stored.references].filter(header => typeof header === 'string'))
    .flatMap(header => header.split(/\s+/)));
  const isAnswered = row => typeof row.provider_message_id === 'string' && answeredIds.has(row.provider_message_id);
  assert.deepEqual(read('outbound', '--mailbox', values.mailbox, '--answered'), outbound.filter(isAnswered),
    '--answered differs from the rows stored mail answers');
  assert.deepEqual(read('outbound', '--mailbox', values.mailbox, '--unanswered'), outbound.filter(row => !isAnswered(row)),
    '--unanswered differs from the rows nothing answers');
  const backwards = message('outbound', '--mailbox', values.mailbox, '--since', newest.created_at, '--until', newest.created_at);
  assert(backwards.status, 'A window whose start is not before its end was not refused');
  assert.equal(JSON.parse(backwards.stderr).error.code, 'MESSAGE_FILTER_INVALID');
  assert.deepEqual(read('outbound', '--mailbox', values.mailbox, '--contains', randomUUID()), [],
    'A text no message holds still matched');
  refusedWith(message('outbound', '--mailbox', values.mailbox, '--to', ' '), 'MESSAGE_FILTER_INVALID',
    'the to filter is empty; name what it must match or leave the filter out');
  const badStatus = message('outbound', '--mailbox', values.mailbox, '--status', 'delivered');
  assert(badStatus.status, 'An unknown status was not refused');
  assert.equal(JSON.parse(badStatus.stderr).error.code, 'MESSAGE_FILTER_INVALID');
  refusedByParser(message('outbound', '--id', newest.id, '--status', 'sent'), 'cannot be used with');

  // An ID nothing holds is not found; a contradictory or malformed call is
  // refused before dispatch.
  const unknown = randomUUID();
  refusedWith(message('replies', unknown), 'NOT_FOUND', 'message was not found');
  refusedWith(message('outbound', '--id', unknown), 'NOT_FOUND', 'outbound message was not found');
  refusedByParser(message('outbound', '--id', newest.id, '--mailbox', values.mailbox), 'cannot be used with');
  refusedByParser(message('replies', 'not-a-uuid'), "invalid value 'not-a-uuid'");
  report.observed = { replied, outbound: outbound.length };
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
