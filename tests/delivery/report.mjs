#!/usr/bin/env node
// Reads a dedicated real mailbox that has already sent mail; sends nothing.
// Proves `mailbox delivery` counts exactly the outbound rows `message
// outbound` lists for the mailbox, by status and refusal code, and refuses a
// mailbox the organization does not hold.
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
const outputRoot = join(root, '.build/real-tests/mailbox-delivery');
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
    '--organization and --mailbox are required; name a dedicated real mailbox that has sent mail');

  const delivery = skrzynka('mailbox', 'delivery', values.mailbox);
  assert.equal(delivery.mailbox_id, values.mailbox);
  const outbound = skrzynka('message', 'outbound', '--mailbox', values.mailbox);
  assert(outbound.length, 'The mailbox has sent nothing; originate a message from it first');
  const counted = delivery.kinds.find(kind => kind.kind === 'outbound');
  assert(counted, 'mailbox delivery reports no outbound kind for a mailbox that sent mail');

  // Every status count equals the rows `message outbound` lists in that status.
  for (const status of ['pending', 'sending', 'sent', 'failed', 'uncertain']) {
    assert.equal(counted[status], outbound.filter(row => row.status === status).length,
      `The ${status} count differs from the outbound rows`);
  }
  assert.equal(counted.total, outbound.length, 'The total differs from the outbound rows');

  // Every refusal code is counted as often as the rows carry it.
  const carried = [...new Set(outbound.map(row => row.error_code).filter(Boolean))];
  const expected = new Map(carried.map(code =>
    [code, outbound.filter(row => row.error_code === code).length]));
  assert.deepEqual(new Map(counted.refusals.map(refusal => [refusal.code, refusal.count])), expected,
    'The refusal counts differ from the outbound rows');

  // Every bounce of an originated message names one of the mailbox's sends.
  assert(Array.isArray(delivery.bounces), 'mailbox delivery reports no bounces list');
  const sendIds = new Set(outbound.map(row => row.id));
  for (const bounce of delivery.bounces.filter(bounce => bounce.kind === 'outbound')) {
    assert(sendIds.has(bounce.send_id), `Bounce ${bounce.report_message_id} names a send this mailbox did not make`);
    assert(bounce.unreadable || bounce.action || !bounce.recipient,
      `Bounce ${bounce.report_message_id} has a recipient but no action`);
  }
  // Each kind's `bounced` counts the distinct sends of that kind its bounces name.
  for (const kind of delivery.kinds) {
    const returned = new Set(delivery.bounces.filter(bounce => bounce.kind === kind.kind).map(bounce => bounce.send_id));
    assert.equal(kind.bounced, returned.size, `The ${kind.kind} bounced count differs from the sends its bounces name`);
  }
  // `days` adds up to the kinds, oldest day first; each outbound row counts
  // on the UTC day it was claimed, and a day's `bounced` names the distinct
  // sends the reports received that day returned.
  const sum = (rows, field) => rows.reduce((total, row) => total + row[field], Number());
  for (const field of ['total', 'sent', 'failed', 'uncertain']) {
    assert.equal(sum(delivery.days, field), sum(delivery.kinds, field), `The days' ${field} differs from the kinds'`);
  }
  const dayList = delivery.days.map(day => day.day);
  assert.deepEqual(dayList, [...dayList].sort(), 'The days are not oldest first');
  const utcDay = instant => new Date(instant).toISOString().slice(Number(), 'YYYY-MM-DD'.length);
  for (const row of outbound) {
    assert(dayList.includes(utcDay(row.created_at)), `No day counts outbound row ${row.id}`);
  }
  for (const day of delivery.days) {
    const returned = new Set(delivery.bounces.filter(bounce => utcDay(bounce.received_at) === day.day)
      .map(bounce => `${bounce.kind} ${bounce.send_id}`));
    assert.equal(day.bounced, returned.size, `The bounced count of ${day.day} differs from the reports received that day`);
  }
  // `domains` names every recipient domain of the rows, counting the
  // recipients of rows that ended `sent` and `failed` and the outbound
  // bounces' failed recipients, most sent first.
  const domainOf = written => written.trim().replace(/>$/, '').split('@').pop().trim().toLowerCase();
  const seen = [];
  for (const row of outbound) {
    const named = [row.recipients, row.cc, row.bcc].filter(header => typeof header === 'string')
      .flatMap(header => header.split(',')).filter(written => written.includes('@'));
    seen.push(...named.map(written => ({ domain: domainOf(written), how: row.status })));
  }
  for (const bounce of delivery.bounces.filter(bounce => bounce.kind === 'outbound'
    && typeof bounce.action === 'string' && bounce.action.trim().toLowerCase() === 'failed'
    && typeof bounce.recipient === 'string' && bounce.recipient.includes('@'))) {
    seen.push({ domain: domainOf(bounce.recipient.split(';').pop()), how: 'bounced' });
  }
  const ofDomain = (domain, how) => seen.filter(entry => entry.domain === domain && entry.how === how).length;
  // Domain order first, then a stable sort by sent, most first.
  const expectedDomains = [...new Set(seen.map(entry => entry.domain))].sort()
    .map(domain => ({ domain, sent: ofDomain(domain, 'sent'), failed: ofDomain(domain, 'failed'), bounced: ofDomain(domain, 'bounced') }))
    .sort((left, right) => right.sent - left.sent);
  assert.deepEqual(delivery.domains, expectedDomains, 'The domains differ from the rows and bounces counted by recipient domain');

  // --since counts only the sends claimed from that instant on: from the
  // newest row's own instant, the rows claimed then or later.
  const [newest] = outbound;
  const windowed = skrzynka('mailbox', 'delivery', values.mailbox, '--since', newest.created_at);
  const inWindow = outbound.filter(row => Date.parse(row.created_at) >= Date.parse(newest.created_at));
  assert.equal(windowed.kinds.find(kind => kind.kind === 'outbound')?.total, inWindow.length,
    '--since counted other outbound rows than were claimed in the window');
  assert(windowed.bounces.every(bounce => Date.parse(bounce.received_at) >= Date.parse(newest.created_at)),
    '--since kept a bounce received before the window');
  const backwards = run(values.binary, ['--organization', values.organization, 'mailbox', 'delivery',
    values.mailbox, '--since', newest.created_at, '--until', newest.created_at]);
  assert(backwards.status, 'A window whose since is not before its until was not refused');
  assert.equal(JSON.parse(backwards.stderr).error.code, 'MESSAGE_FILTER_INVALID');
  // --days-csv and --domains-csv write the report's days and domains as tables,
  // one row each in the report's order; an existing path is refused.
  const daysTable = join(output, 'delivery-days.csv');
  assert.equal(skrzynka('mailbox', 'delivery', values.mailbox, '--days-csv', daysTable).rows, delivery.days.length,
    'mailbox delivery --days-csv counted other days than the report');
  assert.deepEqual(readTable(readFileSync(daysTable, 'utf8')).records.map(row => [row.day, row.total, row.sent, row.bounced]),
    delivery.days.map(day => [day.day, String(day.total), String(day.sent), String(day.bounced)]),
    'mailbox delivery --days-csv wrote other rows than the report');
  const domainsTable = join(output, 'delivery-domains.csv');
  assert.equal(skrzynka('mailbox', 'delivery', values.mailbox, '--domains-csv', domainsTable).rows, delivery.domains.length,
    'mailbox delivery --domains-csv counted other domains than the report');
  assert.deepEqual(readTable(readFileSync(domainsTable, 'utf8')).records.map(row => [row.domain, row.sent, row.failed, row.bounced]),
    delivery.domains.map(domain => [domain.domain, String(domain.sent), String(domain.failed), String(domain.bounced)]),
    'mailbox delivery --domains-csv wrote other rows than the report');
  const daysAgain = run(values.binary, ['--organization', values.organization, 'mailbox', 'delivery', values.mailbox,
    '--days-csv', daysTable]);
  assert.equal(JSON.parse(daysAgain.stderr).error.code, 'DELIVERY_OUTPUT_EXISTS', 'An existing --days-csv path was not refused');

  // `message bounced` names exactly the addresses a stored report says failed for good.
  const reports = skrzynka('message', 'list', '--bounces', '--include-blocked', '--include-snoozed', '--include-muted');
  const failed = new Set(reports.flatMap(message => message.bounce.recipients)
    .filter(outcome => typeof outcome.action === 'string' && outcome.action.trim().toLowerCase() === 'failed'
      && typeof outcome.recipient === 'string')
    .map(outcome => outcome.recipient.split(';').pop().trim().toLowerCase())
    .filter(address => address.length));
  // Every listed address is one a stored report says failed; an address the
  // operator cleared is left out, so the list may hold fewer.
  const bounced = skrzynka('message', 'bounced');
  const listed = bounced.map(entry => entry.address);
  assert.deepEqual(listed, [...listed].sort(), 'message bounced is not in address order');
  assert(listed.every(address => failed.has(address)),
    'message bounced names an address no stored report says failed');
  // --since the newest report's instant keeps exactly the addresses reported
  // then or later, and --until it exactly the others; a backwards window is refused.
  const newestReported = bounced.map(entry => Date.parse(entry.last_reported_at))
    .reduce((kept, at) => Math.max(kept, at), -Infinity);
  if (Number.isFinite(newestReported)) {
    const at = new Date(newestReported).toISOString();
    const lately = skrzynka('message', 'bounced', '--since', at);
    assert.deepEqual(lately, bounced.filter(entry => Date.parse(entry.last_reported_at) >= newestReported),
      'message bounced --since kept other addresses than those reported since');
    assert.deepEqual(skrzynka('message', 'bounced', '--until', at),
      bounced.filter(entry => Date.parse(entry.last_reported_at) < newestReported),
      'message bounced --until kept other addresses than those last reported before');
    const backwards = run(values.binary, ['--organization', values.organization, 'message', 'bounced', '--since', at, '--until', at]);
    assert.equal(JSON.parse(backwards.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'A backwards window was not refused');
  }
  const unreadableSince = run(values.binary, ['--organization', values.organization, 'message', 'bounced', '--since', 'not an instant']);
  assert.equal(JSON.parse(unreadableSince.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'An unreadable --since was not refused');
  // --contains a listed address's domain, in upper case, keeps exactly the
  // addresses holding it; a blank text is refused.
  for (const domain of new Set(listed.map(address => address.slice(address.lastIndexOf('@'))))) {
    assert.deepEqual(skrzynka('message', 'bounced', '--contains', domain.toUpperCase()),
      bounced.filter(entry => entry.address.includes(domain)), `message bounced --contains ${domain} kept other addresses`);
  }
  // --csv writes the listed addresses as a table, each row the listed entry;
  // the same path again is refused and left as it was.
  const bouncedTable = join(output, 'bounced.csv');
  assert.equal(skrzynka('message', 'bounced', '--csv', bouncedTable).addresses, bounced.length,
    'message bounced --csv counted other addresses than the listing');
  const writtenBounced = readFileSync(bouncedTable, 'utf8');
  const { header: bouncedHeader, records: bouncedRows } = readTable(writtenBounced);
  assert.equal(bouncedHeader.join(','), 'address,reports,last_report_id,last_reported_at,status,diagnostic');
  assert.deepEqual(bouncedRows.map(row => [row.address, row.reports, row.last_report_id, row.last_reported_at]),
    bounced.map(entry => [entry.address, String(entry.reports), entry.last_report_id, entry.last_reported_at]),
    'message bounced --csv rows differ from the listing');
  const bouncedAgain = run(values.binary, ['--organization', values.organization, 'message', 'bounced', '--csv', bouncedTable]);
  assert.equal(JSON.parse(bouncedAgain.stderr).error.code, 'BOUNCED_OUTPUT_EXISTS', 'An existing --csv path was not refused');
  assert.equal(readFileSync(bouncedTable, 'utf8'), writtenBounced, 'The refused --csv path was changed');
  const blankContains = run(values.binary, ['--organization', values.organization, 'message', 'bounced', '--contains', ' ']);
  assert.equal(JSON.parse(blankContains.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'A blank --contains was not refused');
  // Clearing an address the list does not hold is refused and clears nothing;
  // no listed address is cleared, so the operator's list stays as it was.
  const unknownAddress = `${randomUUID()}@example.invalid`;
  const notListed = run(values.binary, ['--organization', values.organization, 'message', 'bounced', '--clear', unknownAddress]);
  assert.equal(JSON.parse(notListed.stderr).error.code, 'BOUNCE_NOT_FOUND', 'Clearing an unlisted address was not refused');
  assert.deepEqual(skrzynka('message', 'bounced').map(entry => entry.address), listed, 'A refused clear changed the list');
  // --clear-listed clears only a narrowed listing: narrowed by nothing it is
  // MESSAGE_FILTER_INVALID, narrowed to a text no address holds
  // BOUNCE_NOT_FOUND; neither clears anything.
  const everyOne = run(values.binary, ['--organization', values.organization, 'message', 'bounced', '--clear-listed']);
  assert.equal(JSON.parse(everyOne.stderr).error.code, 'MESSAGE_FILTER_INVALID', '--clear-listed of every address was not refused');
  const noneListed = run(values.binary, ['--organization', values.organization, 'message', 'bounced', '--clear-listed',
    '--contains', randomUUID()]);
  assert.equal(JSON.parse(noneListed.stderr).error.code, 'BOUNCE_NOT_FOUND', '--clear-listed of no address was not refused');
  assert.deepEqual(skrzynka('message', 'bounced').map(entry => entry.address), listed, 'A refused --clear-listed changed the list');
  // A send to one of them is refused before anything is claimed: no row carries the key.
  const [firstBounced] = bounced;
  if (firstBounced) {
    const bodyFile = join(output, 'bounced-body.txt');
    writeFileSync(bodyFile, 'Never sent: the address bounced for good.\n', { flag: 'wx' });
    const key = randomUUID();
    const refusedSend = run(values.binary, ['--organization', values.organization, 'message', 'send',
      '--mailbox', values.mailbox, '--to', firstBounced.address, '--subject', 'Bounced address check',
      '--body-file', bodyFile, '--idempotency-key', key]);
    assert.equal(JSON.parse(refusedSend.stderr).error.code, 'OUTBOUND_RECIPIENT_BOUNCED', 'A send to a bounced address was not refused');
    assert(!skrzynka('message', 'outbound', '--mailbox', values.mailbox).some(row => row.idempotency_key === key),
      'The refused send claimed a row');
  }
  report.bounced = bounced.length;

  // A freshly drawn mailbox ID the organization cannot hold is refused.
  const missing = run(values.binary, ['--organization', values.organization,
    'mailbox', 'delivery', randomUUID()]);
  assert(missing.status, 'An unknown mailbox was not refused');
  assert.equal(JSON.parse(missing.stderr).error.code, 'NOT_FOUND');

  report.observed = { delivery };
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
