#!/usr/bin/env node
// Keeps, replaces and removes a note on one stored message of a dedicated
// real mailbox; nothing reaches the provider. A note the message already
// had is put back at the end.
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
const outputRoot = join(root, '.build/real-tests/notes');
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
function noteFile(name, text) {
  const path = join(output, name);
  writeFileSync(path, text, { flag: 'wx' });
  return path;
}
let target;
let earlier;
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

  const [newest] = skrzynka('message', 'list', '--mailbox', values.mailbox);
  assert(newest, 'The mailbox lists no message to keep a note on; sync it first');
  target = newest.id;
  earlier = newest.note;

  const first = `Qualification note ${randomUUID()}`;
  assert.equal(skrzynka('message', 'note', target, '--file', noteFile('first.txt', `${first}\n`)).note, first);
  assert.equal(skrzynka('message', 'show', target).note, first, 'message show does not carry the note');
  assert.equal(skrzynka('message', 'list', '--mailbox', values.mailbox).find(row => row.id === target).note,
    first, 'message list does not carry the note');

  const second = `Replaced note ${randomUUID()}`;
  assert.equal(skrzynka('message', 'note', target, '--file', noteFile('second.txt', second)).note, second);
  // The text filter looks in notes: the note's own unique word finds exactly this message.
  const word = second.split(' ').pop().toUpperCase();
  assert.deepEqual(skrzynka('message', 'list', '--mailbox', values.mailbox, '--contains', word)
    .map(row => row.id), [target], '--contains does not find the message by its note');
  // --noted keeps exactly the messages carrying a note, this one among them.
  const noted = skrzynka('message', 'list', '--mailbox', values.mailbox, '--noted');
  assert(noted.some(row => row.id === target), '--noted does not list the noted message');
  assert(noted.every(row => row.note), '--noted lists a message without a note');

  refused('MESSAGE_NOTE_INVALID', 'message', 'note', target, '--file', noteFile('blank.txt', ' \n'));
  refused('NOT_FOUND', 'message', 'note', randomUUID(), '--clear');
  assert.equal(skrzynka('message', 'show', target).note, second, 'A refused note changed the kept one');

  // --append adds a line of its own and keeps what the note held; blank text is refused.
  const added = `Added line ${randomUUID()}`;
  assert.equal(skrzynka('message', 'note', target, '--append', '--file', noteFile('added.txt', `${added}\n`)).note,
    `${second}\n${added}`, '--append did not add the line below the note');
  refused('MESSAGE_NOTE_INVALID', 'message', 'note', target, '--append', '--file', noteFile('blank-add.txt', ' \n'));
  assert.equal(skrzynka('message', 'show', target).note, `${second}\n${added}`, 'A refused addition changed the note');

  assert.equal(skrzynka('message', 'note', target, '--clear').note, null);
  assert.equal(skrzynka('message', 'show', target).note, null, 'The removed note is still read');
  assert(!skrzynka('message', 'list', '--mailbox', values.mailbox, '--noted').some(row => row.id === target),
    '--noted still lists the message whose note was removed');

  report.observed = { message: target };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (target) {
    const restore = earlier
      ? cli('message', 'note', target, '--file', noteFile('earlier.txt', earlier))
      : cli('message', 'note', target, '--clear');
    report.cleanup = run(values.binary, restore).status;
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
