#!/usr/bin/env node
// Reads a dedicated real mailbox that already holds synced mail; never sends,
// tags, deletes or moves a cursor. Proves `message list --recipient --sender
// --contains --since --until --list --with-attachments --bounces` against the
// messages the mailbox really received.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/message-filter');
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
const list = (...filters) => JSON.parse(successful(values.binary, ['--organization',
  values.organization, 'message', 'list', '--mailbox', values.mailbox, ...filters]));
// One To entry's address: the bracketed part of `Name <address>`, or the entry.
function bareAddress(entry) {
  const bracketed = /<([^>]+)>/.exec(entry);
  if (!bracketed) return entry.trim().toLowerCase();
  const [, inner] = bracketed;
  return inner.trim().toLowerCase();
}
const bareAddresses = header => header.split(',').map(bareAddress).filter(Boolean);
const ids = messages => messages.map(message => message.id);
let failure;
// The views this run saved, deleted in `finally` when the run did not.
const viewNames = new Set();
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
    '--organization and --mailbox are required; name a dedicated real mailbox that has synced mail');

  const every = list();
  const [newest] = every;
  assert(newest, 'The mailbox holds no synced message; sync real mail into it first');
  const [address] = bareAddresses(newest.recipients);
  assert(address, `The newest message names no recipient: ${newest.recipients}`);

  // A recipient keeps exactly the messages addressed to it, in order, without case.
  const addressed = list('--recipient', address);
  const expected = every.filter(message => bareAddresses(message.recipients).includes(address));
  assert.deepEqual(ids(addressed), ids(expected), 'The recipient filter skipped, added or reordered a message');
  assert(ids(addressed).includes(newest.id), `${address} did not find the newest message`);
  assert.deepEqual(ids(list('--recipient', address.toUpperCase())), ids(addressed),
    'The recipient filter depends on case');

  // An address that merely ends in the recipient's is a different recipient.
  const longer = `x${address}`;
  assert(!ids(list('--recipient', longer)).includes(newest.id), `${longer} matched mail addressed to ${address}`);

  // --cc keeps exactly the messages whose Cc names the address, without case;
  // a message that states no Cc copies no one, and a mailbox whose mail copies
  // no one is searched for an address nobody was copied.
  const copiedIn = message => [message.cc].filter(Boolean).flatMap(bareAddresses);
  const copiedMessage = every.find(message => copiedIn(message).length);
  const [copiedAddress] = copiedMessage ? copiedIn(copiedMessage) : [`nobody-${randomUUID()}@example.invalid`];
  assert.deepEqual(ids(list('--cc', copiedAddress.toUpperCase())),
    ids(every.filter(message => copiedIn(message).includes(copiedAddress))),
    'The cc filter skipped, added or reordered a message');
  report.cc_searched = copiedAddress;

  // A sender part keeps the messages whose From header contains it.
  const senderPart = newest.sender.toLowerCase();
  const fromSender = list('--recipient', address, '--sender', senderPart);
  assert(ids(fromSender).includes(newest.id), 'The sender filter dropped the newest message');
  for (const message of fromSender) assert(message.sender.toLowerCase().includes(senderPart));

  // A word of a stored subject keeps that message, whatever its case, and
  // every message kept holds the word in its subject or text.
  const titled = every.find(message => message.subject?.trim());
  if (titled) {
    const word = titled.subject.split(/\s+/).reduce((longest, part) =>
      part.length > longest.length ? part : longest).toLowerCase();
    const byText = list('--contains', word.toUpperCase());
    assert(ids(byText).includes(titled.id), `The text filter dropped the message titled ${titled.subject}`);
    for (const message of byText) {
      const inSubject = Boolean(message.subject?.toLowerCase().includes(word));
      assert(inSubject || message.body_text.toLowerCase().includes(word),
        `Message ${message.id} does not hold ${word}`);
    }
  }
  // Text no stored message holds keeps none.
  assert.deepEqual(list('--contains', randomUUID()), [], 'Text no message holds still matched');

  // The receive window keeps exactly the messages received in it: since the
  // newest message's instant keeps it, until that instant drops it.
  const sinceNewest = list('--since', newest.received_at);
  assert(ids(sinceNewest).includes(newest.id), '--since the newest instant dropped the newest message');
  for (const message of sinceNewest) {
    assert(Date.parse(message.received_at) >= Date.parse(newest.received_at),
      `Message ${message.id} was received before --since`);
  }
  const untilNewest = list('--until', newest.received_at);
  assert(!ids(untilNewest).includes(newest.id), '--until the newest instant kept the newest message');
  const reversed = run(values.binary, ['--organization', values.organization, 'message', 'list',
    '--mailbox', values.mailbox, '--since', newest.received_at, '--until', newest.received_at]);
  assert(reversed.status, 'An empty receive window was not refused');
  assert.equal(JSON.parse(reversed.stderr).error.code, 'MESSAGE_FILTER_INVALID');
  // A span back from now reaching before the oldest stored message keeps
  // every message; an unknown unit is refused.
  const [oldestStored] = [...every].reverse();
  const minuteMs = Date.parse('1970-01-01T00:01:00Z');
  const backMinutes = Math.ceil((Date.now() - Date.parse(oldestStored.received_at)) / minuteMs) + minuteMs / minuteMs;
  assert.deepEqual(ids(list('--since', `-${backMinutes}m`)), ids(every),
    `--since -${backMinutes}m lists otherwise than the mailbox's whole listing`);
  const unknownUnit = run(values.binary, ['--organization', values.organization, 'message', 'list',
    '--mailbox', values.mailbox, '--since', '-7x']);
  assert.equal(JSON.parse(unknownUnit.stderr).error.code, 'MESSAGE_FILTER_INVALID', '--since -7x was not refused');

  // The flag filters keep exactly the stored messages their field marks.
  assert.deepEqual(ids(list('--with-attachments')),
    ids(every.filter(message => message.attachments?.length)),
    '--with-attachments differs from the messages with recorded attachments');
  assert.deepEqual(ids(list('--bounces')), ids(every.filter(message => message.bounce)),
    '--bounces differs from the stored bounce reports');
  // A recorded file name keeps exactly the messages carrying a file named so;
  // a message whose attachments were never recorded carries none known.
  const files = message => (message.attachments ? message.attachments : []);
  const filed = every.flatMap(files).find(attachment => attachment.filename?.trim());
  if (filed) {
    const part = filed.filename.toUpperCase();
    const carrying = message => files(message)
      .some(attachment => attachment.filename?.toUpperCase().includes(part));
    assert.deepEqual(ids(list('--attachment', part)), ids(every.filter(carrying)),
      '--attachment differs from the messages carrying a file of that name');
  }
  assert.deepEqual(list('--attachment', randomUUID()), [], 'A file name no message carries still matched');
  // --files-over keeps exactly the messages whose recorded file sizes add up to at least the bytes given:
  // here the size of the largest file stored, so its message is among them.
  const weight = message => files(message).map(attachment => attachment.size_bytes)
    .filter(size => typeof size === 'number').reduce((sum, size) => sum + size, Number());
  const sizes = every.flatMap(files).map(attachment => attachment.size_bytes).filter(size => typeof size === 'number');
  if (sizes.length) {
    const largest = Math.max(...sizes);
    assert.deepEqual(ids(list('--files-over', String(largest))), ids(every.filter(message => weight(message) >= largest)),
      '--files-over differs from the messages whose files reach that size');
  }
  // Unanswered keeps exactly the messages none of whose replies ended `sent`.
  const answered = message => JSON.parse(successful(values.binary, ['--organization',
    values.organization, 'message', 'replies', message.id])).some(reply => reply.status === 'sent');
  assert.deepEqual(ids(list('--unanswered')), ids(every.filter(message => !answered(message))),
    '--unanswered differs from the messages no reply was sent to');
  // Answered keeps exactly the others; both at once are refused by the API.
  assert.deepEqual(ids(list('--answered')), ids(every.filter(answered)),
    '--answered differs from the messages a reply was sent to');
  // Failed authentication keeps exactly the messages with a recorded verdict other than pass.
  const doubted = message => (message.authentication ? message.authentication : [])
    .some(verdict => verdict.result && verdict.result.toLowerCase() !== 'pass');
  assert.deepEqual(ids(list('--failed-authentication')), ids(every.filter(doubted)),
    '--failed-authentication differs from the messages with a verdict other than pass');
  const listed = every.find(message => message.mailing_list?.list_id);
  if (listed) {
    const listId = listed.mailing_list.list_id.toUpperCase();
    assert.deepEqual(ids(list('--list', listId)),
      ids(every.filter(message => message.mailing_list?.list_id?.toUpperCase().includes(listId))),
      '--list differs from the messages of that list');
  }

  // The page is cut after the filters: the second match is the one-message
  // page that skips the first match.
  const [first, second] = addressed;
  if (second) {
    const position = String(addressed.indexOf(second));
    assert.deepEqual(ids(list('--recipient', address, '--limit', position, '--offset', position)),
      [second.id], `The page was cut before the filter (first match ${first.id})`);
  }

  // --oldest-first lists the same messages in the opposite order, unfiltered
  // and filtered, and a page is cut in that order: the first one-message
  // page of the recipient's mail is its oldest message.
  assert.deepEqual(ids(list('--oldest-first')), ids(every).reverse(), '--oldest-first is not the listing turned round');
  const oldestFirst = ids(list('--recipient', address, '--oldest-first'));
  assert.deepEqual(oldestFirst, ids(addressed).reverse(),
    '--oldest-first with a filter is not the filtered listing turned round');
  if (second) {
    const [oldest] = oldestFirst;
    assert.deepEqual(ids(list('--recipient', address, '--oldest-first', '--limit', String(addressed.indexOf(second)))),
      [oldest], 'The --oldest-first page was not cut in that order');
  }

  // A blank filter is refused with its cause, never read as no filter.
  for (const flag of ['--recipient', '--sender', '--contains', '--since', '--until', '--tag', '--list', '--attachment']) {
    const refused = run(values.binary, ['--organization', values.organization,
      'message', 'list', '--mailbox', values.mailbox, flag, ' ']);
    assert(refused.status, `A blank ${flag} was not refused`);
    assert.equal(JSON.parse(refused.stderr).error.code, 'MESSAGE_FILTER_INVALID');
  }

  // A saved view lists what its filters list, alone.
  let viewName = `qualification ${randomUUID()}`;
  viewNames.add(viewName);
  const cli = (...args) => ['--organization', values.organization, ...args];
  assert.deepEqual(ids(list('--recipient', address, '--save-view', viewName)), ids(addressed),
    '--save-view changed the listing it saved');
  const viewed = JSON.parse(successful(values.binary, cli('message', 'list', '--view', viewName)));
  assert.deepEqual(ids(viewed), ids(addressed), '--view lists other messages than its filters');
  const shown = JSON.parse(successful(values.binary, cli('message', 'view', 'show', viewName)));
  assert.equal(shown.filters.mailbox_id, values.mailbox);
  assert.equal(shown.filters.recipient, address);
  // view list --count says how many messages each view lists now: this one
  // lists exactly the recipient's mail.
  const matching = JSON.parse(successful(values.binary, cli('message', 'view', 'list', '--count', '--contains', viewName.toUpperCase())));
  assert.deepEqual(matching.map(view => view.name), [viewName], 'view list --contains kept other views than the one its name holds');
  const [counted] = matching;
  assert.deepEqual([counted.messages, counted.filters], [viewed.length, shown.filters], 'view list --count counted otherwise than --view lists');
  for (const [code, args] of [
    ['MESSAGE_FILTER_INVALID', ['message', 'list', '--view', viewName, '--sender', senderPart]],
    ['VIEW_INVALID', ['message', 'list', '--save-view', `${viewName} empty`]],
    ['VIEW_NOT_FOUND', ['message', 'list', '--view', randomUUID()]],
    ['MESSAGE_FILTER_INVALID', ['message', 'view', 'list', '--contains', ' ']],
  ]) {
    const refused = run(values.binary, cli(...args));
    assert(refused.status, `${args.join(' ')} was not refused`);
    assert.equal(JSON.parse(refused.stderr).error.code, code);
  }

  // A renamed view keeps its filters under the new name only; a name another
  // view carries is refused.
  const renamed = `${viewName} renamed`;
  viewNames.add(renamed);
  const moved = JSON.parse(successful(values.binary, cli('message', 'view', 'rename', viewName, '--to', renamed)));
  assert.equal(moved.name, renamed);
  assert.deepEqual(moved.filters, shown.filters, 'rename changed the filters the view keeps');
  viewNames.delete(viewName);
  const gone = run(values.binary, cli('message', 'view', 'show', viewName));
  assert(gone.status, 'The old name still names the view');
  assert.equal(JSON.parse(gone.stderr).error.code, 'VIEW_NOT_FOUND');
  viewName = renamed;
  // A copy keeps the filters under a second name, the view unchanged; renaming or copying onto it is refused.
  const other = `${viewName} other`;
  viewNames.add(other);
  const copied = JSON.parse(successful(values.binary, cli('message', 'view', 'copy', viewName, '--to', other)));
  assert.deepEqual([copied.name, copied.filters], [other, shown.filters], 'view copy kept other filters or another name');
  assert.deepEqual(JSON.parse(successful(values.binary, cli('message', 'view', 'show', viewName))).filters, shown.filters,
    'view copy changed the view it copied');
  for (const verb of ['rename', 'copy']) {
    assert.equal(JSON.parse(run(values.binary, cli('message', 'view', verb, viewName, '--to', other)).stderr).error.code, 'VIEW_EXISTS');
  }
  for (const name of [other, viewName]) {
    successful(values.binary, cli('message', 'view', 'delete', name));
    viewNames.delete(name);
  }
  report.observed = { address, matching: addressed.length, total: every.length };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  for (const name of viewNames) {
    report.cleanup = run(values.binary, ['--organization', values.organization,
      'message', 'view', 'delete', name]).status;
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
