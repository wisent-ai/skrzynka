#!/usr/bin/env node
// Sends one message from a dedicated real mailbox to its own address,
// follows it up, answers it from the same mailbox and checks the follow-up
// moves from due to answered. Nothing reaches anyone but that mailbox.
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
const outputRoot = join(root, '.build/real-tests/follow-ups');
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
function refused(code, ...args) {
  const result = run(values.binary, cli(...args));
  assert(result.status, `${args.join(' ')} was not refused`);
  assert.equal(JSON.parse(result.stderr).error.code, code);
}
// Sync the mailbox, named by its own address as `sync --mailbox` takes it,
// until a stored message passes `test`; each pass is a real IMAP read of
// that mailbox, and the loop ends when the provider has delivered it.
function arrived(test) {
  const [self] = skrzynka('mailbox', 'list').filter(mailbox => mailbox.id === values.mailbox);
  for (;;) {
    const pass = JSON.parse(successful(values.binary, cli('sync', '--mailbox', self.email)));
    assert.equal(pass.mailbox_id, values.mailbox, 'sync --mailbox by address read another mailbox');
    const found = skrzynka('message', 'list', '--mailbox', values.mailbox, '--include-snoozed',
      '--include-blocked').find(test);
    if (found) return found;
  }
}
let followed;
let nudgeTemplate;
let nudgeDrafts = [];
let replyView;
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
    '--organization and --mailbox are required; name a dedicated real mailbox that may mail itself');

  const own = skrzynka('mailbox', 'show', values.mailbox).email;
  const subject = `Skrzynka follow-up ${randomUUID()}`;
  const bodyFile = join(output, 'body.txt');
  writeFileSync(bodyFile, 'Sent to itself by the Skrzynka follow-up qualification.\n', { flag: 'wx' });
  // An unreadable --follow-up on a send is refused before anything is claimed.
  const refusedKey = randomUUID();
  refused('FOLLOW_UP_INVALID', 'message', 'send', '--mailbox', values.mailbox, '--to', own,
    '--subject', subject, '--body-file', bodyFile, '--follow-up', 'tomorrow', '--idempotency-key', refusedKey);
  assert(!skrzynka('message', 'outbound', '--mailbox', values.mailbox).some(row => row.idempotency_key === refusedKey),
    'A send refused for its follow-up claimed a row');

  // A send with --follow-up +1d expects an answer a day after it, once sent.
  const before = Date.now();
  const day = Date.parse('1970-01-02T00:00:00Z');
  const sent = skrzynka('message', 'send', '--mailbox', values.mailbox, '--to', own,
    '--subject', subject, '--body-file', bodyFile, '--follow-up', '+1d', '--idempotency-key', randomUUID());
  assert.equal(sent.status, 'sent', `The send ended ${sent.status}: ${sent.error_message}`);
  followed = sent.id;
  const expected = skrzynka('message', 'followup', 'list').find(row => row.outbound_id === sent.id);
  assert(expected, 'message send --follow-up set no follow-up');
  assert.equal(expected.state, 'waiting', `The follow-up set by the send is ${expected.state}`);
  assert(Date.parse(expected.due_at) - before >= day, 'send --follow-up +1d did not expect an answer a day ahead');

  refused('FOLLOW_UP_INVALID', 'message', 'followup', 'set', sent.id, '--by', 'tomorrow');
  refused('FOLLOW_UP_INVALID', 'message', 'followup', 'set', sent.id, '--by', '+2y');
  refused('NOT_FOUND', 'message', 'followup', 'set', randomUUID(), '--by', new Date().toISOString());

  // A span from now is read as that much later than now: +1d waits, a day ahead.
  const waiting = skrzynka('message', 'followup', 'set', sent.id, '--by', '+1d');
  assert.equal(waiting.state, 'waiting', `A follow-up a day ahead is ${waiting.state}`);
  assert(Date.parse(waiting.due_at) - before >= day, '+1d did not set the instant a day ahead');
  // --due-by keeps the follow-ups expecting an answer before its bound: two
  // days ahead holds this one, an hour ahead does not; nonsense is refused.
  const dueBy = (bound) => skrzynka('message', 'followup', 'list', '--due-by', bound).map(row => row.outbound_id);
  assert.deepEqual([dueBy('+2d').includes(sent.id), dueBy('+1h').includes(sent.id)], [true, false],
    'followup list --due-by split the follow-ups elsewhere than at its bound');
  refused('MESSAGE_FILTER_INVALID', 'message', 'followup', 'list', '--due-by', 'soon');

  // An instant already passed: due at once, until something answers.
  followed = sent.id;
  const due = skrzynka('message', 'followup', 'set', sent.id, '--by', new Date().toISOString());
  assert.equal(due.state, 'due', `A past-due follow-up is ${due.state}`);
  const standing = (state) => skrzynka('message', 'followup', 'list', '--state', state).some(row => row.outbound_id === sent.id);
  assert.deepEqual([standing('due'), standing('waiting'), standing('answered')], [true, false, false],
    'followup list --state lists the due follow-up under another standing');
  // --mailbox keeps the follow-ups of what that mailbox sent, this one among them;
  // a mailbox the organization does not hold is NOT_FOUND.
  const ofMailbox = skrzynka('message', 'followup', 'list', '--mailbox', values.mailbox);
  assert(ofMailbox.some(row => row.outbound_id === sent.id), 'followup list --mailbox does not list it');
  assert(ofMailbox.every(row => row.mailbox_id === values.mailbox), 'followup list --mailbox lists another mailbox');
  const unknownMailbox = run(values.binary, cli('message', 'followup', 'list', '--mailbox', randomUUID()));
  assert.equal(JSON.parse(unknownMailbox.stderr).error.code, 'NOT_FOUND', 'An unknown --mailbox was not refused');
  // --to keeps the follow-ups whose recipients hold the text, case aside;
  // a blank one is refused.
  const toRecipient = skrzynka('message', 'followup', 'list', '--to', sent.recipients.toUpperCase());
  assert(toRecipient.some(row => row.outbound_id === sent.id), 'followup list --to does not list it');
  assert(toRecipient.every(row => row.recipients.toLowerCase().includes(sent.recipients.toLowerCase())),
    'followup list --to lists a follow-up to someone else');
  const blankRecipient = run(values.binary, cli('message', 'followup', 'list', '--to', ' '));
  assert.equal(JSON.parse(blankRecipient.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'A blank --to was not refused');
  // --contains keeps the follow-ups whose sent message's subject or body holds
  // the text, case aside: the subject in upper case finds this one, a fresh
  // UUID nothing sent holds finds none; a blank one is refused.
  const mentioning = (text) => skrzynka('message', 'followup', 'list', '--contains', text).map(row => row.outbound_id);
  assert(mentioning(subject.toUpperCase()).includes(sent.id), 'followup list --contains does not list it by its subject');
  assert.deepEqual(mentioning(randomUUID()), [], 'followup list --contains lists a message that does not hold the text');
  refused('MESSAGE_FILTER_INVALID', 'message', 'followup', 'list', '--contains', ' ');
  // --csv writes the follow-ups listed as a table, this one's row standing
  // `due` with its subject; the same path again is refused and left as it was.
  const followTable = join(output, 'follow-ups.csv');
  const followListed = skrzynka('message', 'followup', 'list');
  assert.equal(skrzynka('message', 'followup', 'list', '--csv', followTable).follow_ups, followListed.length,
    'followup list --csv counted other follow-ups than the listing');
  const writtenFollowUps = readFileSync(followTable, 'utf8');
  const { records: followRows } = readTable(writtenFollowUps);
  // Each row is a listed follow-up, in order; a standing read at another
  // instant may differ for one coming due between the two reads.
  assert.deepEqual(followRows.map(row => [row.outbound_id, row.due_at, row.subject]),
    followListed.map(row => [row.outbound_id, row.due_at, row.subject]), 'followup list --csv rows differ from the listing');
  const followRow = followRows.find(row => row.outbound_id === sent.id);
  assert.deepEqual([followRow.state, followRow.subject], ['due', subject], 'followup list --csv wrote this follow-up otherwise');
  refused('FOLLOW_UP_OUTPUT_EXISTS', 'message', 'followup', 'list', '--csv', followTable);
  assert.equal(readFileSync(followTable, 'utf8'), writtenFollowUps, 'The refused --csv path was changed');
  // nudge drafts one reminder from a run-only template to each follow-up
  // owed an answer that the filters keep: this due one alone, to its
  // recipients, `Re: ` its subject, the subject filled in the text. Answered
  // ones and a filter keeping none owed an answer are refused, and so is a
  // template naming a placeholder nothing fills; none of those drafts.
  nudgeTemplate = `follow-up-nudge-${randomUUID()}`;
  const nudgeText = join(output, 'nudge.txt');
  writeFileSync(nudgeText, 'Any news on {{subject}}, expected by {{due_at}}?\n', { flag: 'wx' });
  skrzynka('message', 'template', 'save', nudgeTemplate, '--file', nudgeText);
  nudgeDrafts = skrzynka('message', 'followup', 'nudge', '--template', nudgeTemplate, '--state', 'due', '--contains', subject);
  assert.deepEqual(nudgeDrafts.map(draft => [draft.mailbox_id, draft.recipients, draft.subject]),
    [[values.mailbox, sent.recipients, `Re: ${subject}`]], 'followup nudge drafted otherwise than one reminder to this follow-up');
  const [nudged] = nudgeDrafts;
  assert(nudged.body.includes(subject) && nudged.body.includes(due.due_at),
    'followup nudge left the subject or the due instant unfilled');
  refused('FOLLOW_UP_INVALID', 'message', 'followup', 'nudge', '--template', nudgeTemplate, '--state', 'answered');
  refused('FOLLOW_UP_INVALID', 'message', 'followup', 'nudge', '--template', nudgeTemplate, '--contains', randomUUID());
  const strayText = join(output, 'stray.txt');
  writeFileSync(strayText, `{{${randomUUID().replaceAll('-', '_')}}}\n`, { flag: 'wx' });
  skrzynka('message', 'template', 'save', nudgeTemplate, '--file', strayText);
  refused('TEMPLATE_VALUE_MISSING', 'message', 'followup', 'nudge', '--template', nudgeTemplate, '--contains', subject);
  assert.deepEqual(skrzynka('message', 'draft', 'list', '--contains', subject).map(draft => draft.id),
    nudgeDrafts.map(draft => draft.id), 'A refused nudge kept a draft');
  // outbound --follow-up expects an answer to every sent message the listing
  // keeps: this run's subject keeps this one alone, waiting a day ahead, and
  // nothing is skipped. A listing narrowed by nothing, one listing nothing,
  // and an unreadable instant are refused; then it is due again.
  const expecting = skrzynka('message', 'outbound', '--contains', subject, '--follow-up', '+1d');
  assert.deepEqual([expecting.follow_ups.map(row => [row.outbound_id, row.state]), expecting.skipped],
    [[[sent.id, 'waiting']], []], 'outbound --follow-up set otherwise than this one waiting follow-up');
  assert(Date.parse(expecting.follow_ups[0].due_at) - before >= day, 'outbound --follow-up +1d did not set a day ahead');
  refused('FOLLOW_UP_INVALID', 'message', 'outbound', '--follow-up', '+1d');
  refused('NOT_FOUND', 'message', 'outbound', '--contains', randomUUID(), '--follow-up', '+1d');
  refused('FOLLOW_UP_INVALID', 'message', 'outbound', '--contains', subject, '--follow-up', 'tomorrow');
  const outboundCsv = run(values.binary, cli('message', 'outbound', '--contains', subject, '--follow-up', '+1d',
    '--csv', join(output, 'outbound.csv')));
  assert(outboundCsv.status, 'outbound --follow-up with --csv was not refused');
  // --resend-failed sends only failures: the listing holding this sent
  // message alone sends nothing again; a listing narrowed by nothing and a
  // blank key are refused, and the flag without a key never reaches the core.
  refused('NOT_FOUND', 'message', 'outbound', '--contains', subject, '--resend-failed', '--idempotency-key', randomUUID());
  refused('OUTBOUND_RESEND_INVALID', 'message', 'outbound', '--resend-failed', '--idempotency-key', randomUUID());
  refused('IDEMPOTENCY_KEY_INVALID', 'message', 'outbound', '--contains', subject, '--resend-failed', '--idempotency-key', ' ');
  assert(run(values.binary, cli('message', 'outbound', '--contains', subject, '--resend-failed')).status,
    'outbound --resend-failed without a key was not refused');
  assert.deepEqual(skrzynka('message', 'outbound', '--contains', subject).map(row => row.id), [sent.id],
    'outbound --resend-failed sent a sent message again');
  assert.equal(skrzynka('message', 'followup', 'set', sent.id, '--by', new Date().toISOString()).state, 'due');

  // The copy arrives; the mailbox answers it quoting it, and the answer arrives.
  const copy = arrived(message => message.message_id === sent.provider_message_id);
  const reply = skrzynka('message', 'reply', copy.id, '--body-file', bodyFile, '--quote',
    '--idempotency-key', randomUUID());
  assert.equal(reply.status, 'sent', `The reply ended ${reply.status}: ${reply.error_message}`);
  for (const line of copy.body_text.trim().split('\n').filter(line => line.trim())) {
    assert(reply.body.includes(`> ${line}`), `The quoted reply does not quote ${JSON.stringify(line)}`);
  }
  assert(reply.body.includes(`${copy.sender} wrote:`), 'The quoted reply does not name the sender');
  const answer = arrived(message => message.in_reply_to === sent.provider_message_id);
  assert(answer.body_text.includes('> '), 'The received answer does not carry the quote');

  const answered = skrzynka('message', 'followup', 'list').find(row => row.outbound_id === sent.id);
  assert.equal(answered.state, 'answered', `The answered follow-up is ${answered.state}`);
  assert.equal(answered.answer, answer.id, 'The follow-up names another answer');
  assert.deepEqual([standing('due'), standing('answered')], [false, true],
    'followup list --state does not move the answered follow-up from due to answered');
  // A standing that is not one of the three is refused by Clap before dispatch.
  const unknownState = run(values.binary, cli('message', 'followup', 'list', '--state', 'late'));
  assert(unknownState.status && unknownState.stderr.includes('invalid value'),
    'followup list --state late was not refused as an invalid value');
  // followup show reads that one follow-up exactly as the listing holds it.
  assert.deepEqual(skrzynka('message', 'followup', 'show', sent.id), answered,
    'followup show reads the follow-up otherwise than the listing');
  // reply --view answers every message a saved view lists: a run-only tag
  // keeps the arrived copy alone, so exactly one reply is sent to the
  // mailbox itself, claimed by <key>-<message id>; the same request again
  // sends nothing new, and an unknown view is VIEW_NOT_FOUND.
  replyView = { tag: `reply-view-${randomUUID()}`, name: `reply-view-${randomUUID()}`, message: copy.id };
  skrzynka('message', 'tag', copy.id, '--add', replyView.tag);
  skrzynka('message', 'list', '--mailbox', values.mailbox, '--tag', replyView.tag, '--include-snoozed', '--include-blocked',
    '--save-view', replyView.name);
  const viewKey = randomUUID();
  const viewReply = ['message', 'reply', '--view', replyView.name, '--body-file', bodyFile, '--idempotency-key', viewKey];
  const answeredView = skrzynka(...viewReply);
  assert.deepEqual([answeredView.replied.map(row => [row.message_id, row.idempotency_key, row.status]), answeredView.refused],
    [[[copy.id, `${viewKey}-${copy.id}`, 'sent']], []], 'reply --view answered otherwise than the one tagged copy');
  assert.deepEqual(skrzynka(...viewReply).replied.map(row => row.id), answeredView.replied.map(row => row.id),
    'Repeating reply --view with the same key sent again');
  refused('VIEW_NOT_FOUND', 'message', 'reply', '--view', randomUUID(), '--body-file', bodyFile, '--idempotency-key', randomUUID());

  assert.equal(skrzynka('message', 'followup', 'clear', sent.id).state, 'answered');
  followed = undefined;
  refused('FOLLOW_UP_NOT_FOUND', 'message', 'followup', 'clear', sent.id);
  // clear --state clears every follow-up standing so that the narrowing keeps;
  // the subject is this run's alone, so no other follow-up is touched: due
  // clears none, answered exactly this one. Without --state or an ID, Clap refuses.
  skrzynka('message', 'followup', 'set', sent.id, '--by', '+1d');
  followed = sent.id;
  assert.deepEqual(skrzynka('message', 'followup', 'clear', '--state', 'due', '--contains', subject), [],
    'followup clear --state due cleared an answered follow-up');
  assert.deepEqual(skrzynka('message', 'followup', 'clear', '--state', 'answered', '--contains', subject)
    .map(row => [row.outbound_id, row.state]), [[sent.id, 'answered']],
    'followup clear --state answered cleared otherwise than this run\'s follow-up');
  followed = undefined;
  const unnamed = run(values.binary, cli('message', 'followup', 'clear', '--contains', subject));
  assert(unnamed.status && !unnamed.stdout, 'followup clear without an ID or --state was not refused');
  refused('FOLLOW_UP_NOT_FOUND', 'message', 'followup', 'show', sent.id);
  refused('NOT_FOUND', 'message', 'followup', 'show', randomUUID());

  report.observed = { outbound: sent.id, copy: copy.id, answer: answer.id };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (followed) {
    report.cleanup = run(values.binary, cli('message', 'followup', 'clear', followed)).status;
  }
  for (const draft of nudgeDrafts) run(values.binary, cli('message', 'draft', 'delete', draft.id));
  if (nudgeTemplate) run(values.binary, cli('message', 'template', 'delete', nudgeTemplate));
  if (replyView) {
    run(values.binary, cli('message', 'view', 'delete', replyView.name));
    run(values.binary, cli('message', 'tag', replyView.message, '--remove', replyView.tag));
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
