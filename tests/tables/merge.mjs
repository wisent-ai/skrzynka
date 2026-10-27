#!/usr/bin/env node
// Drafts from a dedicated real mailbox with `message draft save --template
// --merge`, one per row of a CSV table, and reads them back; never sends.
// The template and every draft this run made are deleted before it ends.
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
  'schedule-at': { type: 'string' }, every: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
assert(values.organization && values.mailbox && values.recipient && values['schedule-at'] && values.every,
  'Pass --organization, --mailbox of a dedicated mailbox, --recipient (an address the operator owns), '
  + '--schedule-at (an RFC 3339 instant well ahead; nothing is sent, the drafts are deleted first) and --every (a span such as 5m)');
const outputRoot = join(root, '.build/real-tests/draft-merge');
mkdirSync(outputRoot, { recursive: true });
const output = mkdtempSync(join(outputRoot, 'run-'));
const report = {
  started_at: new Date().toISOString(), source_revision: values['source-revision'],
  commands: [], tables: [], status: 'running',
};
const hash = data => createHash('sha256').update(data).digest('hex');
function run(args) {
  const index = report.commands.length;
  const stdoutPath = join(output, `${index}.stdout`);
  const stderrPath = join(output, `${index}.stderr`);
  const stdout = openSync(stdoutPath, 'wx');
  const stderr = openSync(stderrPath, 'wx');
  let result;
  try {
    result = spawnSync(values.binary, ['--organization', values.organization, 'message', ...args],
      { cwd: root, stdio: ['ignore', stdout, stderr] });
  } finally {
    closeSync(stdout);
    closeSync(stderr);
  }
  report.commands.push({ argv: [values.binary, 'message', ...args], exit_status: result.status,
    signal: result.signal, error: result.error?.message, stdout: `${index}.stdout`, stderr: `${index}.stderr` });
  if (result.error) throw result.error;
  return { ...result, stdout: readFileSync(stdoutPath, 'utf8'), stderr: readFileSync(stderrPath, 'utf8') };
}
function read(...args) {
  const result = run(args);
  assert(!result.status && !result.signal, `message ${args.join(' ')} failed: ${result.stderr}`);
  return JSON.parse(result.stdout);
}
function refused(code, ...args) {
  const result = run(args);
  assert(result.status, `${code} was expected and message ${args.join(' ')} succeeded`);
  assert.equal(JSON.parse(result.stderr).error.code, code, `message ${args.join(' ')} was refused otherwise`);
}
// One CSV line as a spreadsheet writes it: a field holding a comma or a quote
// quoted, its quotes doubled.
const line = fields => `${fields.map(field => (/[",\r\n]/.test(field) ? `"${field.replaceAll('"', '""')}"` : field)).join(',')}\r\n`;
// A table of this run, written to a file the merge reads.
function table(name, text) {
  const path = join(output, name);
  writeFileSync(path, text);
  report.tables.push({ path, sha256: hash(text) });
  return path;
}
const marker = randomUUID();
const template = `merge-${marker}`;
const merge = path => ['draft', 'save', '--mailbox', values.mailbox, '--template', template, '--merge', path];
const mine = () => read('draft', 'list', '--mailbox', values.mailbox, '--contains', marker);
let templateSaved = false;
// The suppression entry this run added, taken off the list before it ends.
let suppressedDomain;
let failure;
try {
  const body = join(output, 'template.txt');
  writeFileSync(body, `Hello {{name}}, about {{company}}. ${marker}`);
  read('template', 'save', template, '--file', body, '--subject', 'Offer for {{company}}');
  templateSaved = true;

  // Two rows: a name holding a comma and a quote, which the table quotes as a
  // spreadsheet would, and a plain one; each becomes its own draft.
  const people = [
    { name: `Smith, "${randomUUID()}"`, company: randomUUID() },
    { name: randomUUID(), company: randomUUID() },
  ];
  const rows = people.map(person => line([values.recipient, person.name, person.company])).join('');
  const merged = read(...merge(table('rows.csv', `to,name,company\r\n${rows}`)));
  assert.deepEqual(merged.map(draft => [draft.subject, draft.body, draft.recipients]),
    people.map(person => [`Offer for ${person.company}`, `Hello ${person.name}, about ${person.company}. ${marker}`, values.recipient]),
    'draft save --merge drafted other messages than the rows fill');
  assert.deepEqual(mine().map(draft => draft.id).sort(), merged.map(draft => draft.id).sort(),
    'draft list does not hold exactly the merged drafts');

  // Every refused table keeps no draft: a column the template does not name,
  // a placeholder no column fills, no `to` column, an unreadable address, a
  // header without rows, a quote never closed; --merge without --template is
  // refused by the argument parser.
  const [person] = people;
  const kept = mine().length;
  refused('TEMPLATE_VALUE_UNUSED', ...merge(table('unused.csv',
    `to,name,company,city\r\n${line([values.recipient, person.name, person.company, marker])}`)));
  refused('TEMPLATE_VALUE_MISSING', ...merge(table('missing.csv', `to,name\r\n${line([values.recipient, person.name])}`)));
  refused('DRAFT_MERGE_INVALID', ...merge(table('no-to.csv', `email,name,company\r\n${rows}`)));
  refused('OUTBOUND_RECIPIENT_INVALID', ...merge(table('bad-address.csv',
    `to,name,company\r\n${rows}${line([marker, person.name, person.company])}`)));
  refused('DRAFT_MERGE_INVALID', ...merge(table('header-only.csv', 'to,name,company\r\n')));
  refused('DRAFT_MERGE_INVALID', ...merge(table('open-quote.csv', `to,name,company\r\n${values.recipient},"${marker},x\r\n`)));
  // A recipient a send would refuse is refused before anything is drafted:
  // a domain of this run alone is suppressed for the check and taken off
  // the list again below.
  const domain = `@${marker}.example`;
  read('suppressed', '--add', domain, '--reason', `merge test ${marker}`);
  suppressedDomain = domain;
  refused('DRAFT_MERGE_INVALID', ...merge(table('suppressed.csv',
    `to,name,company\r\n${rows}${line([`someone${domain}`, person.name, person.company])}`)));
  assert.equal(mine().length, kept, 'A refused table kept a draft');
  const unparsed = run(['draft', 'save', '--mailbox', values.mailbox, '--merge', table('plain.csv', 'to\r\n')]);
  assert(unparsed.status && !unparsed.stdout, 'draft save --merge without --template was not refused');

  // draft schedule --contains spreads the merged drafts out: the earliest
  // written at --schedule-at, each next the same gap after the one before,
  // each under its own key. A span that does not read schedules none; once
  // all are scheduled none waits, which is refused. The drafts are deleted
  // below, before the instant the operator named.
  const at = new Date(values['schedule-at']).toISOString();
  const schedule = (...flags) => ['draft', 'schedule', '--contains', marker, '--mailbox', values.mailbox, '--at', at, ...flags];
  refused('DRAFT_SCHEDULE_INVALID', ...schedule('--every', marker, '--idempotency-key', marker));
  assert(mine().every(draft => !draft.send_at), 'A refused span scheduled a draft');
  const spread = read(...schedule('--every', values.every, '--idempotency-key', marker));
  assert.deepEqual(spread.map(draft => draft.id), merged.map(draft => draft.id),
    'draft schedule --contains scheduled other drafts, or not in the order they were written');
  const [first, ...later] = spread;
  assert.equal(Date.parse(first.send_at), Date.parse(at), 'The earliest draft is not scheduled at --at');
  const gaps = later.map((draft, index) => Date.parse(draft.send_at) - Date.parse(spread[index].send_at));
  const [firstGap] = gaps;
  assert(gaps.every(gap => gap === firstGap) && later.every((draft, index) => draft.send_at > spread[index].send_at),
    'draft schedule --every did not spread the drafts one equal gap apart');
  assert.deepEqual(spread.map(draft => draft.send_key), merged.map(draft => `${marker}-${draft.id}`),
    'draft schedule --contains claimed the drafts by other keys');
  refused('DRAFT_SCHEDULE_INVALID', ...schedule('--idempotency-key', marker));
  // draft unschedule --contains takes exactly the merged drafts off their
  // schedule, keeping them; then none is scheduled, so it is NOT_FOUND.
  const held = read('draft', 'unschedule', '--contains', marker, '--mailbox', values.mailbox);
  assert.deepEqual(held.map(draft => draft.id).sort(), merged.map(draft => draft.id).sort(),
    'draft unschedule --contains unscheduled other drafts than the merged ones');
  assert(held.every(draft => !draft.send_at && !draft.send_key), 'draft unschedule --contains left a draft scheduled');
  assert(mine().every(draft => !draft.send_at), 'A merged draft is still scheduled after draft unschedule --contains');
  assert.equal(mine().length, merged.length, 'draft unschedule --contains removed a draft');
  refused('NOT_FOUND', 'draft', 'unschedule', '--contains', marker);

  // draft delete --contains removes exactly the merged drafts and answers
  // them; once they are gone the same command finds none, and a delete
  // naming neither an ID nor a text is refused by the argument parser.
  const removed = read('draft', 'delete', '--contains', marker, '--mailbox', values.mailbox);
  assert.deepEqual(removed.map(draft => draft.id).sort(), merged.map(draft => draft.id).sort(),
    'draft delete --contains removed other drafts than the merged ones');
  assert.deepEqual(mine(), [], 'draft delete --contains left a merged draft');
  refused('NOT_FOUND', 'draft', 'delete', '--contains', marker);
  const unnamed = run(['draft', 'delete', '--mailbox', values.mailbox]);
  assert(unnamed.status && !unnamed.stdout, 'draft delete without an ID or --contains was not refused');

  report.observed = { merged: merged.length, scheduled: spread.length, gap_ms: firstGap, removed: removed.length };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  for (const draft of mine()) run(['draft', 'delete', draft.id]);
  if (templateSaved) run(['template', 'delete', template]);
  if (suppressedDomain) run(['suppressed', '--remove', suppressedDomain]);
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, '  ')}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
