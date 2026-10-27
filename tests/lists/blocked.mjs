#!/usr/bin/env node
// Changes only Skrzynka's block list on a dedicated real mailbox's
// organization and restores it; nothing reaches the provider. Proves blocking
// leaves exactly the blocked sender's mail out of `message list`,
// --include-blocked lists it, domains block their addresses, and the refusals.
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
const outputRoot = join(root, '.build/real-tests/blocked-senders');
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
const listed = (...flags) => skrzynka('message', 'list', '--mailbox', values.mailbox, ...flags)
  .map(message => message.id);
function refused(code, ...args) {
  const result = run(values.binary, cli(...args));
  assert(result.status, `${args.join(' ')} was not refused`);
  assert.equal(JSON.parse(result.stderr).error.code, code);
}
// The address a From header names: its last bracketed part, or the header.
function address(header) {
  const bracketed = /<(?<inner>[^<>]*)>[^<]*$/.exec(header.trim());
  return (bracketed ? bracketed.groups.inner : header).trim().toLowerCase();
}
let failure;
let prior;
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

  prior = skrzynka('message', 'blocked').blocked;
  const every = skrzynka('message', 'list', '--mailbox', values.mailbox, '--include-blocked');
  const [newest] = every;
  assert(newest, 'The mailbox holds no stored message; sync it first');
  const sender = address(newest.sender);
  const fromSender = every.filter(message => address(message.sender) === sender).map(message => message.id);

  assert(skrzynka('message', 'block', sender.toUpperCase()).blocked.includes(sender),
    'The blocked address is not listed in lower case');
  const shown = listed();
  assert(fromSender.every(id => !shown.includes(id)), 'A blocked sender\'s mail is still listed');
  assert.deepEqual(listed('--include-blocked'), every.map(message => message.id),
    '--include-blocked does not list every message');
  // --count says how much stored mail each entry leaves out: for the sender, every
  // message of the organization from that address.
  const counted = skrzynka('message', 'blocked', '--count').find(entry => entry.entry === sender);
  const organizationWide = skrzynka('message', 'list', '--include-blocked')
    .filter(message => address(message.sender) === sender).length;
  assert(counted && counted.messages === organizationWide, 'blocked --count counted other mail than the sender sent');
  // --contains the sender in upper case keeps the entries holding it, the
  // sender among them, alone, with or without --count; a blank text is refused.
  const holding = skrzynka('message', 'blocked').blocked.filter(entry => entry.includes(sender));
  assert.deepEqual(skrzynka('message', 'blocked', '--contains', sender.toUpperCase()).blocked, holding,
    'blocked --contains kept other entries than those holding the text');
  assert.deepEqual(skrzynka('message', 'blocked', '--count', '--contains', sender).map(entry => entry.entry), holding,
    'blocked --count --contains counted other entries than those holding the text');
  const blankBlocked = run(values.binary, ['--organization', values.organization, 'message', 'blocked', '--contains', ' ']);
  assert.equal(JSON.parse(blankBlocked.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'A blank --contains was not refused');

  const domain = `@${sender.split('@').pop()}`;
  if (!prior.includes(domain)) {
    skrzynka('message', 'block', domain);
    assert(!listed().some(id => fromSender.includes(id)), 'A blocked domain still lists its mail');
    skrzynka('message', 'unblock', domain);
  }
  if (!prior.includes(sender)) {
    skrzynka('message', 'unblock', sender);
    assert(fromSender.every(id => listed().includes(id) || prior.some(entry => sender.endsWith(entry))),
      'An unblocked sender\'s mail is not listed again');
  }

  refused('BLOCKED_SENDER_INVALID', 'message', 'block', 'not an address');
  refused('BLOCKED_SENDER_NOT_FOUND', 'message', 'unblock', `${randomUUID()}@example.invalid`);

  // --import blocks every entry of a file once, in line order and lower
  // case, skipping a comment and a blank line and the note after an entry;
  // a file with one unreadable line is refused quoting it and blocks nothing.
  const first = `${randomUUID()}@example.invalid`;
  const second = `@${randomUUID()}.invalid`;
  const list = join(output, 'blocked.txt');
  writeFileSync(list, `# exported spam list\n\n${first.toUpperCase()} spam since spring\n${second}\n${first}\n`);
  const imported = skrzynka('message', 'blocked', '--import', list);
  assert.deepEqual(imported.imported, [first, second], 'blocked --import kept other entries than the file names');
  assert([first, second].every(entry => imported.blocked.includes(entry)), 'blocked --import did not block every entry');
  const unwritten = `${randomUUID()}@example.invalid`;
  const broken = join(output, 'broken.txt');
  writeFileSync(broken, `${unwritten}\nnot an address\n`);
  refused('BLOCKED_SENDER_INVALID', 'message', 'blocked', '--import', broken);
  assert(!skrzynka('message', 'blocked').blocked.includes(unwritten), 'A refused import blocked a line');
  // --export writes every entry, or those --contains keeps, as --import
  // reads them; an existing path is refused before anything is written.
  const exported = join(output, 'exported.txt');
  const everyEntry = skrzynka('message', 'blocked').blocked;
  assert.equal(skrzynka('message', 'blocked', '--export', exported).entries, everyEntry.length,
    'blocked --export counted other entries than the list holds');
  assert.deepEqual(readFileSync(exported, 'utf8').split('\n').filter(line => line && !line.startsWith('#')), everyEntry,
    'blocked --export wrote other lines than the entries');
  refused('BLOCKED_OUTPUT_EXISTS', 'message', 'blocked', '--export', exported);
  const narrowed = join(output, 'narrowed.txt');
  skrzynka('message', 'blocked', '--export', narrowed, '--contains', second);
  assert.deepEqual(readFileSync(narrowed, 'utf8').split('\n').filter(line => line && !line.startsWith('#')), [second],
    'blocked --export --contains wrote other entries than those holding the text');
  // --csv writes the entries --contains keeps, each with the mail it hides,
  // the rows --count prints; the same path again is refused.
  const blockedTable = join(output, 'blocked.csv');
  const counted = skrzynka('message', 'blocked', '--count', '--contains', second);
  assert.equal(skrzynka('message', 'blocked', '--csv', blockedTable, '--contains', second).entries, counted.length,
    'blocked --csv counted other entries than --count lists');
  assert.deepEqual(readTable(readFileSync(blockedTable, 'utf8')).records,
    counted.map(row => ({ entry: row.entry, messages: String(row.messages) })), 'blocked --csv wrote other rows than --count');
  refused('BLOCKED_OUTPUT_EXISTS', 'message', 'blocked', '--csv', blockedTable);

  report.observed = { sender, messages: fromSender.length };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (prior) {
    const now = JSON.parse(successful(values.binary, cli('message', 'blocked'))).blocked;
    report.restored = now.filter(entry => !prior.includes(entry)).map(entry => {
      const removed = run(values.binary, cli('message', 'unblock', entry));
      return { entry, exit_status: removed.status, stderr: removed.stderr };
    });
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
