#!/usr/bin/env node
// Changes only Skrzynka's suppression list and drafts on a dedicated real
// mailbox's organization, and takes back what it added; sends nothing,
// because every send it asks for must be refused before anything is claimed.
// Proves `message suppressed` adds, lists and removes entries, and that a
// send, a draft send and a schedule naming a suppressed recipient, by address
// or by domain, are refused with OUTBOUND_RECIPIENT_SUPPRESSED and claim no row.
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
const outputRoot = join(root, '.build/real-tests/suppression');
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
  const { error } = JSON.parse(result.stderr);
  assert.equal(error.code, code);
  return error.message;
}
let failure;
const added = [];
const removed = new Set();
let draft;
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
  assert(values.organization && values.mailbox, '--organization and --mailbox are required; name a dedicated real mailbox');

  // Addresses at a freshly drawn domain nobody holds: nothing could be sent there.
  const domain = `${randomUUID()}.invalid`;
  const person = `person@${domain}`;
  const reason = `asked not to be written to ${randomUUID()}`;
  const byAddress = skrzynka('message', 'suppressed', '--add', ` ${person.toUpperCase()} `, '--reason', reason);
  added.push(byAddress.entry);
  assert.deepEqual([byAddress.entry, byAddress.reason], [person, reason], 'The entry is not kept trimmed, in lower case, with its reason');
  const byDomain = skrzynka('message', 'suppressed', '--add', `@${domain}`);
  added.push(byDomain.entry);
  assert.equal(byDomain.reason, null, 'An entry added without a reason names one');
  const listed = skrzynka('message', 'suppressed');
  assert.deepEqual(listed.filter(kept => added.includes(kept.entry)), [byDomain, byAddress],
    'message suppressed lists other entries than were added, or out of entry order');

  // A send naming the address, and one naming another address of the domain,
  // are refused before anything is claimed: no row carries their keys.
  const body = join(output, 'body.txt');
  writeFileSync(body, 'Never sent: the suppression qualification.\n', { flag: 'wx' });
  for (const to of [person, `other@${domain}`]) {
    const key = randomUUID();
    const sentence = refused('OUTBOUND_RECIPIENT_SUPPRESSED', 'message', 'send', '--mailbox', values.mailbox,
      '--to', to, '--subject', 'Never sent', '--body-file', body, '--idempotency-key', key);
    assert(sentence.includes(to), `The refusal does not name ${to}: ${sentence}`);
    assert(!skrzynka('message', 'outbound', '--mailbox', values.mailbox).some(row => row.idempotency_key === key),
      `A refused send to ${to} claimed a row`);
  }
  assert(refused('OUTBOUND_RECIPIENT_SUPPRESSED', 'message', 'send', '--mailbox', values.mailbox, '--to', person,
    '--subject', 'Never sent', '--body-file', body, '--idempotency-key', randomUUID(), '--allow-bounced').includes(reason),
  'The refusal does not carry the reason, or --allow-bounced overrode the list');
  // check-recipients says the same before anything is sent: the address and
  // another of the domain are suppressed by their own entries, an address of
  // another fresh domain would be sent, and an unreadable one is refused.
  const elsewhere = `someone@${randomUUID()}.invalid`;
  const checked = skrzynka('message', 'recipients', person, `other@${domain}, ${elsewhere}`);
  assert.deepEqual(checked.map(entry => [entry.address, entry.suppressed?.entry, entry.sendable]),
    [[person, person, false], [`other@${domain}`, `@${domain}`, false], [elsewhere, undefined, true]],
    'message recipients met the addresses otherwise than a send does');
  refused('OUTBOUND_RECIPIENT_INVALID', 'message', 'recipients', 'not an address');

  // A draft to the address is refused on send and on schedule, and stays a draft.
  draft = skrzynka('message', 'draft', 'save', '--mailbox', values.mailbox, '--to', person, '--subject', 'Never sent',
    '--body-file', body);
  refused('OUTBOUND_RECIPIENT_SUPPRESSED', 'message', 'draft', 'send', draft.id, '--idempotency-key', randomUUID());
  const later = new Date(Date.now() + Date.parse('1970-01-02T00:00:00Z')).toISOString();
  refused('OUTBOUND_RECIPIENT_SUPPRESSED', 'message', 'draft', 'schedule', draft.id, '--at', later,
    '--idempotency-key', randomUUID());
  assert.equal(skrzynka('message', 'draft', 'show', draft.id).send_at, null, 'A refused schedule left the draft scheduled');
  assert(skrzynka('message', 'draft', 'list', '--mailbox', values.mailbox, '--suppressed').some(listed => listed.id === draft.id),
    'draft list --suppressed leaves out a draft to a suppressed recipient');

  // Refusals of the list itself.
  refused('SUPPRESSION_INVALID', 'message', 'suppressed', '--add', 'not an address');
  refused('SUPPRESSION_INVALID', 'message', 'suppressed', '--add', person, '--reason', ' ');
  refused('SUPPRESSION_NOT_FOUND', 'message', 'suppressed', '--remove', `nobody@${randomUUID()}.invalid`);

  // --import keeps every line of a file, its own reason or --reason, and
  // skips blank and # lines; one unreadable line refuses the whole file.
  const [first, second] = [`first@${domain}`, `second@${domain}`];
  const ownReason = `own reason ${randomUUID()}`;
  const sharedReason = `shared reason ${randomUUID()}`;
  const list = join(output, 'import.txt');
  writeFileSync(list, `# exported do-not-contact list\n\n${first.toUpperCase()}  ${ownReason}\n${second}\n`, { flag: 'wx' });
  const imported = skrzynka('message', 'suppressed', '--import', list, '--reason', sharedReason);
  added.push(...imported.entries.map(kept => kept.entry));
  assert.deepEqual(imported.entries.map(kept => [kept.entry, kept.reason]), [[first, ownReason], [second, sharedReason]],
    '--import kept other entries or reasons than the file holds');
  const broken = join(output, 'broken.txt');
  const unwritten = `unwritten@${domain}`;
  writeFileSync(broken, `${unwritten}\nnot an address\n`, { flag: 'wx' });
  assert(refused('SUPPRESSION_INVALID', 'message', 'suppressed', '--import', broken).includes('not an address'),
    'The import refusal does not quote the unreadable line');
  assert(!skrzynka('message', 'suppressed').some(kept => kept.entry === unwritten), 'A refused import kept a line');
  // --contains keeps the entries whose entry or reason holds the text, case
  // aside: the domain finds all four, the shared reason the one line using it.
  assert.deepEqual(skrzynka('message', 'suppressed', '--contains', domain.toUpperCase()).map(kept => kept.entry).sort(),
    [...added].sort(), '--contains the domain kept other entries');
  assert.deepEqual(skrzynka('message', 'suppressed', '--contains', sharedReason).map(kept => kept.entry), [second],
    '--contains a reason kept other entries');
  refused('MESSAGE_FILTER_INVALID', 'message', 'suppressed', '--contains', ' ');
  // --since keeps the entries added at or after the instant, --until those
  // added before it: the four were added within the last day, so
  // --since -1d lists them and --until -1d does not; a backwards window and
  // an unreadable instant are refused.
  const lately = (...window) => skrzynka('message', 'suppressed', '--contains', domain, ...window).map(kept => kept.entry).sort();
  assert.deepEqual(lately('--since', '-1d'), [...added].sort(), '--since -1d left out an entry added just now');
  assert.deepEqual(lately('--until', '-1d'), [], '--until -1d kept an entry added just now');
  refused('MESSAGE_FILTER_INVALID', 'message', 'suppressed', '--since', '-1d', '--until', '-2d');
  refused('MESSAGE_FILTER_INVALID', 'message', 'suppressed', '--since', 'not an instant');

  // Removing takes the entry off the list and answers it.
  for (const entry of added) {
    assert.equal(skrzynka('message', 'suppressed', '--remove', entry).entry, entry, `--remove ${entry} answered another entry`);
    removed.add(entry);
  }
  assert(!skrzynka('message', 'suppressed').some(kept => kept.entry.endsWith(domain)), 'A removed entry is still listed');

  // --export writes the entries kept as the text --import reads: importing
  // it back gives the same entries with the same reasons. An existing path
  // is refused before anything is written.
  const before = skrzynka('message', 'suppressed');
  const exported = join(output, 'suppressed.txt');
  const written = skrzynka('message', 'suppressed', '--export', exported);
  assert.equal(written.entries, before.length, 'suppressed --export counted other entries than the list holds');
  refused('SUPPRESSION_OUTPUT_EXISTS', 'message', 'suppressed', '--export', exported);
  const lines = readFileSync(exported, 'utf8').split('\n').filter(line => line && !line.startsWith('#'));
  assert.deepEqual(lines, before.map(kept => [kept.entry, kept.reason].filter(Boolean).join(' ')),
    'suppressed --export wrote other lines than the entries and their reasons');
  // --csv writes the same entries as a table of entry, reason and added_at;
  // the same path again is refused.
  const suppressedTable = join(output, 'suppressed.csv');
  assert.equal(skrzynka('message', 'suppressed', '--csv', suppressedTable).entries, before.length,
    'suppressed --csv counted other entries than the list holds');
  const { header: entryHeader, records: entryRows } = readTable(readFileSync(suppressedTable, 'utf8'));
  assert.equal(entryHeader.join(','), 'entry,reason,added_at', 'suppressed --csv wrote another header');
  assert.deepEqual(entryRows.map(row => [row.entry, row.added_at]), before.map(kept => [kept.entry, kept.added_at]),
    'suppressed --csv rows differ from the list');
  refused('SUPPRESSION_OUTPUT_EXISTS', 'message', 'suppressed', '--csv', suppressedTable);
  if (before.length) {
    const again = skrzynka('message', 'suppressed', '--import', exported).entries;
    assert.deepEqual(again.map(kept => [kept.entry, kept.reason]), before.map(kept => [kept.entry, kept.reason]),
      'Importing the export changed an entry or its reason');
  }

  report.observed = { domain };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  report.cleanup = added.filter(entry => !removed.has(entry))
    .map(entry => run(values.binary, cli('message', 'suppressed', '--remove', entry)).status);
  if (draft) report.draft_cleanup = run(values.binary, cli('message', 'draft', 'delete', draft.id)).status;
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
