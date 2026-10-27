#!/usr/bin/env node
// Lists the organization's real mailboxes by state and checks each listing
// holds exactly the mailboxes the full listing shows in that state, and that
// --csv writes the same mailboxes as a table without a credential. Changes
// nothing but the run's own output directory.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { readTable } from '../tables/csv.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/mailbox-states');
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
const ids = mailboxes => mailboxes.map(mailbox => mailbox.id).sort();
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
  assert(values.organization, '--organization is required; name an organization holding real mailboxes');

  // Each state lists exactly the mailboxes the full listing shows in it.
  const every = skrzynka('mailbox', 'list');
  const expected = {
    failing: every.filter(mailbox => mailbox.enabled && mailbox.last_error_code),
    undeclared: every.filter(mailbox => !mailbox.enabled),
    unsynchronized: every.filter(mailbox => !mailbox.last_sync_at),
  };
  report.observed = {};
  for (const [state, mailboxes] of Object.entries(expected)) {
    const listed = skrzynka('mailbox', 'list', '--state', state);
    assert.deepEqual(ids(listed), ids(mailboxes), `mailbox list --state ${state} lists other mailboxes`);
    report.observed[state] = listed.length;
  }
  // An unknown state, and a state with --inbound, are refused before anything is read.
  for (const args of [['--state', 'asleep'], ['--state', 'failing', '--inbound']]) {
    const refused = run(values.binary, cli('mailbox', 'list', ...args));
    assert(refused.status, `mailbox list ${args.join(' ')} was not refused`);
  }
  // --csv writes every listed mailbox, in the listing's order, with its
  // address, a failing one's error code beside it, and no column naming its
  // Skarbiec item; the same path again is refused before anything is read.
  const tablePath = join(output, 'mailboxes.csv');
  const written = skrzynka('mailbox', 'list', '--csv', tablePath);
  assert.equal(written.mailboxes, every.length, 'mailbox list --csv counted other mailboxes than the listing');
  const table = readTable(readFileSync(tablePath, 'utf8'));
  assert(!table.header.includes('skarbiec_item_id'), 'mailbox list --csv wrote the Skarbiec item');
  assert.deepEqual(table.records.map(record => [record.id, record.email]),
    every.map(mailbox => [mailbox.id, mailbox.email]), 'mailbox list --csv wrote other rows than the listing');
  for (const failing of expected.failing) {
    assert.equal(table.records.find(record => record.id === failing.id).last_error_code, failing.last_error_code,
      `mailbox list --csv lost the error of ${failing.email}`);
  }
  const occupied = run(values.binary, cli('mailbox', 'list', '--csv', tablePath));
  assert(occupied.status && occupied.stderr.includes('MAILBOX_OUTPUT_EXISTS'),
    `mailbox list --csv over an existing file was not refused with MAILBOX_OUTPUT_EXISTS: ${occupied.stderr}`);
  const inbound = run(values.binary, cli('mailbox', 'list', '--inbound', '--csv', join(output, 'inbound.csv')));
  assert(inbound.status, 'mailbox list --inbound --csv was not refused');
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
