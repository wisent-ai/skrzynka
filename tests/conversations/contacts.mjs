#!/usr/bin/env node
// Reads a dedicated real mailbox that holds stored mail; sends nothing.
// Proves `message contacts` agrees with the stored messages and outbound
// rows: every sender is listed, each count equals the mail behind it, and
// the list is ordered most exchanged first; and `--vcard` writes them as
// vCards to a new file under this run's report folder.
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
const outputRoot = join(root, '.build/real-tests/message-contacts');
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
// The address one header entry names: its last bracketed part, or the entry.
function address(entry) {
  const bracketed = /<(?<inner>[^<>]*)>[^<]*$/.exec(entry.trim());
  const bare = bracketed ? bracketed.groups.inner : entry;
  return bare.trim().toLowerCase();
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
  assert(values.organization && values.mailbox,
    '--organization and --mailbox are required; name a dedicated real mailbox holding stored mail');

  const messages = skrzynka('message', 'list', '--mailbox', values.mailbox);
  assert(messages.length, 'The mailbox holds no stored message; sync it first');
  const entries = skrzynka('message', 'outbound', '--mailbox', values.mailbox)
    .filter(row => row.status !== 'failed')
    .flatMap(row => [row.recipients, row.cc, row.bcc].filter(Boolean))
    .flatMap(header => header.split(','))
    .filter(entry => entry.trim())
    .map(address);
  const contacts = skrzynka('message', 'contacts', '--mailbox', values.mailbox);
  const byAddress = new Map(contacts.map(contact => [contact.address, contact]));

  for (const sender of new Set(messages.map(message => address(message.sender)))) {
    const contact = byAddress.get(sender);
    assert(contact, `Sender ${sender} is not listed`);
    assert.equal(contact.received, messages.filter(message => address(message.sender) === sender).length,
      `The received count of ${sender} differs from the stored mail`);
  }
  // `bounced` says exactly whether `message bounced` lists the address,
  // `suppressed` whether a `message suppressed` entry covers it or its
  // domain, and `blocked` whether a `message blocked` entry does.
  const bounced = new Set(skrzynka('message', 'bounced').map(entry => entry.address));
  const suppressed = skrzynka('message', 'suppressed').map(kept => kept.entry);
  const covered = recipient => suppressed.some(entry => entry === recipient || (entry.startsWith('@') && recipient.endsWith(entry)));
  const blockedEntries = skrzynka('message', 'blocked').blocked;
  for (const contact of contacts) {
    assert.equal(contact.written, entries.filter(entry => entry === contact.address).length,
      `The written count of ${contact.address} differs from the outbound rows`);
    assert.equal(contact.bounced, bounced.has(contact.address),
      `The bounced flag of ${contact.address} differs from message bounced`);
    assert.equal(contact.suppressed, covered(contact.address),
      `The suppressed flag of ${contact.address} differs from message suppressed`);
    assert.equal(contact.blocked, blockedEntries.some(entry => entry === contact.address
      || (entry.startsWith('@') && contact.address.endsWith(entry))),
      `The blocked flag of ${contact.address} differs from message blocked`);
  }
  // --bounced keeps exactly the contacts flagged bounced, in the same order.
  assert.deepEqual(skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--bounced'),
    contacts.filter(contact => contact.bounced), '--bounced keeps other contacts than those flagged bounced');
  // --suppressed keeps exactly the contacts flagged suppressed, in the same order.
  assert.deepEqual(skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--suppressed'),
    contacts.filter(contact => contact.suppressed), '--suppressed keeps other contacts than those flagged suppressed');
  // --blocked keeps exactly the contacts flagged blocked, in the same order.
  assert.deepEqual(skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--blocked'),
    contacts.filter(contact => contact.blocked), '--blocked keeps other contacts than those flagged blocked');
  // --never-written keeps exactly those who wrote and were never written to,
  // --never-received those written to who never wrote, in the same order.
  assert.deepEqual(skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--never-written'),
    contacts.filter(contact => contact.received && !contact.written), '--never-written keeps other contacts');
  assert.deepEqual(skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--never-received'),
    contacts.filter(contact => contact.written && !contact.received), '--never-received keeps other contacts');
  const exchanged = contacts.map(contact => contact.received + contact.written);
  assert.deepEqual(exchanged, [...exchanged].sort((left, right) => right - left),
    'The contacts are not ordered most exchanged first');
  // --recent lists the same contacts, the latest exchanged with first.
  const latest = contact => [contact.last_received_at, contact.last_written_at]
    .filter(instant => typeof instant === 'string').map(Date.parse).reduce((kept, at) => Math.max(kept, at), -Infinity);
  const recent = skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--recent');
  assert.deepEqual(recent.map(contact => contact.address).sort(), contacts.map(contact => contact.address).sort(),
    '--recent lists other contacts');
  const instants = recent.map(latest);
  assert.deepEqual(instants, [...instants].sort((left, right) => right - left),
    'The contacts --recent lists are not ordered latest first');
  // --since the latest contact's latest instant keeps exactly the contacts
  // exchanged with at or after it, in the same order; an unreadable one is refused.
  const [latestContact] = recent;
  const cut = latest(latestContact);
  const sinceInstant = new Date(cut).toISOString();
  assert.deepEqual(skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--since', sinceInstant),
    contacts.filter(contact => latest(contact) >= cut), '--since keeps other contacts than those exchanged with since');
  const unreadableSince = run(values.binary, ['--organization', values.organization, 'message', 'contacts', '--since', 'not an instant']);
  assert.equal(JSON.parse(unreadableSince.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'An unreadable --since was not refused');
  // --until the same instant keeps exactly the contacts last exchanged with
  // before it, who went quiet, in order; a window not forward is refused.
  assert.deepEqual(skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--until', sinceInstant),
    contacts.filter(contact => latest(contact) < cut), '--until keeps other contacts than those quiet since');
  const backwards = run(values.binary, ['--organization', values.organization, 'message', 'contacts',
    '--since', sinceInstant, '--until', sinceInstant]);
  assert.equal(JSON.parse(backwards.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'A window whose start is not before its end was not refused');

  // --contains keeps exactly the contacts whose address or a name contains it, case aside.
  const [first] = contacts;
  const part = first.address.split('@').pop().toUpperCase();
  const found = skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--contains', part);
  const expected = contacts.filter(contact => contact.address.includes(part.toLowerCase())
    || contact.names.some(name => name.toLowerCase().includes(part.toLowerCase())));
  assert.deepEqual(found.map(contact => contact.address), expected.map(contact => contact.address),
    `--contains ${part} kept other contacts than the full list holds`);
  const blank = run(values.binary, ['--organization', values.organization, 'message', 'contacts', '--contains', ' ']);
  assert(blank.status, 'A blank --contains was not refused');
  assert.equal(JSON.parse(blank.stderr).error.code, 'MESSAGE_FILTER_INVALID');

  // --vcard writes one card per listed contact, in order, each with its address.
  const cardFile = join(output, 'contacts.vcf');
  const written = skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--vcard', cardFile);
  assert.equal(written.contacts, contacts.length, 'The vCard file holds another count of contacts');
  const cards = readFileSync(cardFile, 'utf8').split('BEGIN:VCARD\r\n').filter(Boolean);
  assert.equal(cards.length, contacts.length, 'The vCard file holds another number of cards');
  cards.forEach((card, index) => {
    assert(card.includes(`EMAIL:${contacts[index].address}\r\n`), `Card ${index} is not ${contacts[index].address}`);
  });
  const again = run(values.binary, ['--organization', values.organization, 'message', 'contacts',
    '--mailbox', values.mailbox, '--vcard', cardFile]);
  assert(again.status, 'An existing vCard path was not refused');
  assert.equal(JSON.parse(again.stderr).error.code, 'CONTACTS_OUTPUT_EXISTS');
  // --csv writes the header and one row per listed contact, in order, with
  // its address, counts and marks as the listing prints them.
  const tableFile = join(output, 'contacts.csv');
  assert.equal(skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--csv', tableFile).contacts, contacts.length,
    'The CSV file holds another count of contacts');
  const contactTable = readTable(readFileSync(tableFile, 'utf8'));
  assert.equal(contactTable.header.join(','), 'address,name,received,last_received_at,written,last_written_at,bounced,suppressed,blocked');
  assert.deepEqual(contactTable.records.map(row => row.address), contacts.map(contact => contact.address),
    'The CSV rows name other addresses, or in another order');
  assert.deepEqual(contactTable.records.map(row => [row.received, row.written, row.bounced]),
    contacts.map(contact => [String(contact.received), String(contact.written), String(contact.bounced)]),
    'The CSV rows carry other counts or marks than the listing');
  const both = run(values.binary, ['--organization', values.organization, 'message', 'contacts',
    '--vcard', join(output, 'both.vcf'), '--csv', join(output, 'both.csv')]);
  assert(both.status, '--vcard beside --csv was not refused');

  // --suppress-listed puts exactly the listed contacts the list does not
  // cover yet on it, with the reason; the same listing again finds none
  // left, a listing narrowed by nothing and a blank reason are refused.
  // The entries added are removed again, so the operator's list stays.
  const target = contacts.find(contact => !contact.suppressed);
  assert(target, 'Every contact is already suppressed; the mailbox needs one that is not');
  const listed = contacts.filter(contact => !contact.suppressed && (contact.address.includes(target.address)
    || contact.names.some(name => name.toLowerCase().includes(target.address))));
  const why = `real test ${randomUUID()}`;
  const added = skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--contains', target.address,
    '--suppress-listed', '--reason', why);
  try {
    assert.deepEqual(added.entries.map(kept => [kept.entry, kept.reason]), listed.map(contact => [contact.address, why]),
      '--suppress-listed added other entries than the contacts listed');
    assert(skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--contains', target.address)
      .every(contact => contact.suppressed), 'A contact --suppress-listed added is not flagged suppressed');
    const twice = run(values.binary, ['--organization', values.organization, 'message', 'contacts',
      '--mailbox', values.mailbox, '--contains', target.address, '--suppress-listed']);
    assert.equal(JSON.parse(twice.stderr).error.code, 'NOT_FOUND', 'A listing left with no unsuppressed contact was not refused');
  } finally {
    for (const kept of added.entries) skrzynka('message', 'suppressed', '--remove', kept.entry);
  }
  assert.deepEqual(skrzynka('message', 'suppressed').map(kept => kept.entry), suppressed,
    'The suppression list differs from before --suppress-listed after its entries were removed');
  const everyone = run(values.binary, ['--organization', values.organization, 'message', 'contacts',
    '--mailbox', values.mailbox, '--suppress-listed']);
  assert.equal(JSON.parse(everyone.stderr).error.code, 'SUPPRESSION_INVALID', 'An unnarrowed --suppress-listed was not refused');
  const blankWhy = run(values.binary, ['--organization', values.organization, 'message', 'contacts',
    '--contains', target.address, '--suppress-listed', '--reason', ' ']);
  assert.equal(JSON.parse(blankWhy.stderr).error.code, 'SUPPRESSION_INVALID', 'A blank --reason was not refused');

  // --block-listed blocks exactly the listed contacts no blocked entry covers
  // yet; the same listing again finds none left and a listing narrowed by
  // nothing is refused. They are unblocked again, so the operator's list stays.
  const blockTarget = contacts.find(contact => !contact.blocked);
  assert(blockTarget, 'Every contact is already blocked; the mailbox needs one that is not');
  const toBlock = contacts.filter(contact => !contact.blocked && (contact.address.includes(blockTarget.address)
    || contact.names.some(name => name.toLowerCase().includes(blockTarget.address))));
  const blockedNow = skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--contains', blockTarget.address, '--block-listed');
  try {
    assert.deepEqual(blockedNow.blocked, toBlock.map(contact => contact.address), '--block-listed blocked other addresses than the contacts listed');
    assert(skrzynka('message', 'contacts', '--mailbox', values.mailbox, '--contains', blockTarget.address)
      .every(contact => contact.blocked), 'A contact --block-listed blocked is not flagged blocked');
    const blockedTwice = run(values.binary, ['--organization', values.organization, 'message', 'contacts',
      '--mailbox', values.mailbox, '--contains', blockTarget.address, '--block-listed']);
    assert.equal(JSON.parse(blockedTwice.stderr).error.code, 'NOT_FOUND', 'A listing left with no unblocked contact was not refused');
  } finally {
    for (const entry of blockedNow.blocked) skrzynka('message', 'unblock', entry);
  }
  assert.deepEqual(skrzynka('message', 'blocked').blocked, blockedEntries,
    'The blocked list differs from before --block-listed after its entries were unblocked');
  const blockEveryone = run(values.binary, ['--organization', values.organization, 'message', 'contacts',
    '--mailbox', values.mailbox, '--block-listed']);
  assert.equal(JSON.parse(blockEveryone.stderr).error.code, 'BLOCKED_SENDER_INVALID', 'An unnarrowed --block-listed was not refused');

  report.observed = { contacts: contacts.length, messages: messages.length, written: entries.length };
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
