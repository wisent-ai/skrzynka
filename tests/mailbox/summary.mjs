#!/usr/bin/env node
// Reads a dedicated real mailbox's summary and checks every count against
// the listings `message list` gives for the same filters. Changes nothing.
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
const outputRoot = join(root, '.build/real-tests/mailbox-summary');
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
const listed = (...filters) => skrzynka('message', 'list', '--mailbox', values.mailbox,
  '--include-blocked', '--include-snoozed', '--include-muted', ...filters);
// Without --include-muted the only messages left out are those of muted conversations.
const unmuted = () => skrzynka('message', 'list', '--mailbox', values.mailbox,
  '--include-blocked', '--include-snoozed');
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

  const summary = skrzynka('mailbox', 'summary', values.mailbox);
  const every = listed();
  assert.equal(summary.mailbox_id, values.mailbox);
  assert.equal(summary.messages, every.length, 'messages differs from the listing');
  for (const [field, flag] of [['unanswered', '--unanswered'], ['with_attachments', '--with-attachments'],
    ['bounces', '--bounces'], ['failed_authentication', '--failed-authentication']]) {
    assert.equal(summary[field], listed(flag).length, `${field} differs from message list ${flag}`);
  }
  assert.equal(summary.tagged, every.filter(message => message.tags.length).length, 'tagged differs');
  assert.equal(summary.noted, every.filter(message => message.note).length, 'noted differs');
  const recordedSizes = every.flatMap(message => (message.attachments ? message.attachments : []))
    .map(file => file.size_bytes).filter(size => typeof size === 'number');
  assert.equal(summary.file_bytes, recordedSizes.reduce((sum, size) => sum + size, Number()),
    'file_bytes differs from the recorded sizes of the listed files');
  assert.equal(summary.muted, every.length - unmuted().length, 'muted differs from what --include-muted adds');
  assert.equal(summary.senders, new Set(every.map(message => message.sender.toLowerCase())).size, 'senders differs');
  // The earliest and latest receive instants of the listing; none for an empty mailbox.
  const earliest = every.map(message => message.received_at)
    .reduce((kept, instant) => (kept === null || instant < kept ? instant : kept), null);
  const latest = every.map(message => message.received_at)
    .reduce((kept, instant) => (kept === null || instant > kept ? instant : kept), null);
  assert.equal(summary.first_received_at, earliest, 'first_received_at differs from the listing');
  assert.equal(summary.last_received_at, latest, 'last_received_at differs from the listing');
  // days counts the listing per UTC receive day, oldest first, and adds up to messages.
  const perDay = new Map();
  for (const message of every) {
    const day = new Date(message.received_at).toISOString().slice(Number(), 'YYYY-MM-DD'.length);
    perDay.set(day, [...[perDay.get(day)].filter(Array.isArray).flat(), message.id]);
  }
  assert.deepEqual(summary.days, [...perDay.keys()].sort().map(day => ({ day, received: perDay.get(day).length })),
    'days differs from the listing counted per UTC receive day');

  // --since counts the mail received from then on, as message list --since keeps it.
  if (latest) {
    const windowed = skrzynka('mailbox', 'summary', values.mailbox, '--since', latest);
    assert.equal(Date.parse(windowed.since), Date.parse(latest), 'The summary does not name its window');
    // A span back from now is named as the instant it stood for.
    const minuteMs = Date.parse('1970-01-01T00:01:00Z');
    const before = Date.now() - minuteMs;
    const spanned = skrzynka('mailbox', 'summary', values.mailbox, '--since', '-1m');
    const spanSince = Date.parse(spanned.since);
    assert(spanSince >= before && spanSince <= Date.now() - minuteMs, `--since -1m was named ${spanned.since}`);
    assert.equal(windowed.messages, listed('--since', latest).length, '--since counted other messages than the listing keeps');
    // --until counts what came before, as message list --until keeps it; the
    // two windows split the mailbox's mail.
    const earlier = skrzynka('mailbox', 'summary', values.mailbox, '--until', latest);
    assert.equal(earlier.messages, listed('--until', latest).length, '--until counted other messages than the listing keeps');
    assert.equal(earlier.messages + windowed.messages, summary.messages, '--since and --until do not split the mailbox at one instant');
  }

  const unknown = run(values.binary, cli('mailbox', 'summary', randomUUID()));
  assert(unknown.status, 'An unknown mailbox was not refused');
  assert.equal(JSON.parse(unknown.stderr).error.code, 'NOT_FOUND');
  // --days-csv writes the summary's days as a table, one row per day in
  // order; an existing path is refused before anything is read.
  const daysTable = join(output, 'summary-days.csv');
  assert.equal(skrzynka('mailbox', 'summary', values.mailbox, '--days-csv', daysTable).rows, summary.days.length,
    'mailbox summary --days-csv counted other days than the summary');
  assert.deepEqual(readTable(readFileSync(daysTable, 'utf8')).records,
    summary.days.map(day => ({ day: day.day, received: String(day.received) })), 'mailbox summary --days-csv wrote other rows');
  const again = run(values.binary, cli('mailbox', 'summary', values.mailbox, '--days-csv', daysTable));
  assert.equal(JSON.parse(again.stderr).error.code, 'SUMMARY_OUTPUT_EXISTS', 'An existing --days-csv path was not refused');

  report.observed = summary;
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
