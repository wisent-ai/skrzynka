#!/usr/bin/env node
// Reads a dedicated real mailbox whose synced mail includes a message with an
// attachment; never sends, tags, deletes or moves mail. Proves `message
// attachment list` and `message attachment save` against the bytes the
// provider holds, and their refusals.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { readTable } from '../tables/csv.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/message-attachments');
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
function refused(args, code) {
  const result = run(values.binary, ['--organization', values.organization, ...args]);
  assert(result.status, `${args.join(' ')} was not refused`);
  const { error } = JSON.parse(result.stderr);
  assert.equal(error.code, code, `${args.join(' ')} was refused for another reason: ${error.message}`);
  return error.message;
}
let failure;
// The freshly drawn tag the --tag check puts on one message, and that message.
let marker;
let tagged;
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
    '--organization and --mailbox are required; name a dedicated real mailbox whose synced mail has an attachment');

  // Every message's attachments, recorded at import or read from the
  // provider once; the first message that has a readable one is used.
  const messages = skrzynka('message', 'list', '--mailbox', values.mailbox);
  assert(messages.length, 'The mailbox holds no synced message; sync real mail into it first');
  let chosen;
  for (const message of messages) {
    const listed = skrzynka('message', 'attachment', 'list', message.id);
    assert.deepEqual(listed.map(attachment => attachment.index), [...listed.keys()],
      `Message ${message.id} attachments are not indexed in order from zero`);
    // Listing records them: the stored message now carries the same list.
    assert.deepEqual(skrzynka('message', 'show', message.id).attachments, listed,
      `Message ${message.id} does not carry the attachments its listing answered`);
    const readable = listed.find(attachment => attachment.size_bytes !== null);
    if (readable) {
      chosen = { message, listed, attachment: readable };
      break;
    }
  }
  assert(chosen, 'No synced message has a readable attachment; deliver one to the dedicated mailbox and sync');

  // Saving writes exactly the recorded size, and the digest printed is the file's.
  const target = join(output, 'saved-attachment');
  const saved = skrzynka('message', 'attachment', 'save', chosen.message.id,
    '--index', String(chosen.attachment.index), '--output', target);
  const bytes = readFileSync(target);
  assert.equal(statSync(target).size, chosen.attachment.size_bytes, 'The saved file differs from the recorded size');
  assert.equal(saved.size_bytes, bytes.length, 'The printed size differs from the file');
  assert.equal(saved.sha256, hash(bytes), 'The printed digest differs from the file');
  assert.equal(saved.filename, chosen.attachment.filename);
  assert.equal(saved.content_type, chosen.attachment.content_type);

  // An existing file is never overwritten, and the refusal comes before the provider is read.
  const existing = refused(['message', 'attachment', 'save', chosen.message.id,
    '--index', String(chosen.attachment.index), '--output', target], 'ATTACHMENT_OUTPUT_EXISTS');
  assert(existing.includes(target), `The refusal does not name the occupied path: ${existing}`);
  assert.equal(hash(readFileSync(target)), saved.sha256, 'A refused save changed the existing file');

  // An index past the last attachment names the indexes the message has.
  const [first] = chosen.listed;
  const last = chosen.listed.at(-1);
  const missing = refused(['message', 'attachment', 'save', chosen.message.id,
    '--index', String(chosen.listed.length), '--output', join(output, 'never-written')],
  'ATTACHMENT_NOT_FOUND');
  assert(missing.includes(`indexed ${first.index} to ${last.index}`),
    `The refusal does not name the indexes the message has: ${missing}`);

  // --all saves every attachment into a new directory as <index>-<name>,
  // accounting for each index once, and refuses an existing directory.
  const folder = join(output, 'all-attachments');
  const all = run(values.binary, ['--organization', values.organization, 'message', 'attachment', 'save',
    chosen.message.id, '--all', '--output', folder]);
  const everyOne = JSON.parse(all.stdout);
  if (all.status) assert.equal(JSON.parse(all.stderr).error.code, 'ATTACHMENT_SAVE_INCOMPLETE');
  assert.deepEqual([...everyOne.saved, ...everyOne.refused].map(entry => entry.index).sort((a, b) => a - b),
    chosen.listed.map(attachment => attachment.index), '--all did not account for every attachment once');
  for (const entry of everyOne.saved) {
    assert.equal(hash(readFileSync(entry.path)), entry.sha256, `${entry.path} differs from its printed digest`);
    assert(entry.path.startsWith(folder), `${entry.path} was written outside the directory`);
  }
  const savedChosen = everyOne.saved.find(entry => entry.index === chosen.attachment.index);
  assert.equal(savedChosen?.sha256, saved.sha256, '--all saved the chosen attachment otherwise than --index did');
  refused(['message', 'attachment', 'save', chosen.message.id, '--all', '--output', folder], 'ATTACHMENT_OUTPUT_EXISTS');
  assert(run(values.binary, ['--organization', values.organization, 'message', 'attachment', 'save',
    chosen.message.id, '--all', '--index', String(chosen.attachment.index), '--output', join(output, 'both')]).status,
  '--all with --index was accepted');
  // A freshly drawn message ID the organization cannot hold is refused, not read.
  refused(['message', 'attachment', 'list', randomUUID()], 'NOT_FOUND');

  // attachment find lists every recorded file of the mailbox's mail, newest
  // message first, and its filters keep exactly the files they describe.
  const everyStored = skrzynka('message', 'list', '--mailbox', values.mailbox,
    '--with-attachments', '--include-blocked', '--include-snoozed', '--include-muted');
  const recorded = everyStored.flatMap(message => message.attachments.map(file => ({
    message_id: message.id, mailbox_id: message.mailbox_id, sender: message.sender, subject: message.subject,
    received_at: message.received_at, index: file.index, filename: file.filename,
    content_type: file.content_type, size_bytes: file.size_bytes,
  })));
  const find = (...filters) => skrzynka('message', 'attachment', 'find', '--mailbox', values.mailbox, ...filters);
  assert.deepEqual(find(), recorded, 'attachment find lists other files than the stored mail records');
  const chosenType = chosen.attachment.content_type.split('/').pop().toUpperCase();
  assert.deepEqual(find('--type', chosenType),
    recorded.filter(file => file.content_type.toLowerCase().includes(chosenType.toLowerCase())), '--type keeps other files');
  assert.deepEqual(find('--over', String(chosen.attachment.size_bytes)),
    recorded.filter(file => typeof file.size_bytes === 'number' && file.size_bytes >= chosen.attachment.size_bytes),
    '--over keeps other files');
  if (chosen.attachment.filename) {
    const part = chosen.attachment.filename.toUpperCase();
    assert.deepEqual(find('--name', part),
      recorded.filter(file => typeof file.filename === 'string' && file.filename.toLowerCase().includes(part.toLowerCase())),
      '--name keeps other files');
  }
  assert.deepEqual(find('--name', randomUUID()), [], '--name kept a file for a text no name holds');
  refused(['message', 'attachment', 'find', '--name', ' '], 'MESSAGE_FILTER_INVALID');
  // --since and --until at the chosen message's receive instant split the
  // files there; a backwards window is refused.
  const at = chosen.message.received_at;
  assert.deepEqual([...find('--since', at), ...find('--until', at)].sort((left, right) => left.received_at.localeCompare(right.received_at) || left.message_id.localeCompare(right.message_id) || left.index - right.index),
    [...recorded].sort((left, right) => left.received_at.localeCompare(right.received_at) || left.message_id.localeCompare(right.message_id) || left.index - right.index),
    '--since and --until do not split the files between them');
  assert(find('--since', at).every(file => Date.parse(file.received_at) >= Date.parse(at)), '--since kept a file of older mail');
  refused(['message', 'attachment', 'find', '--since', at, '--until', at], 'MESSAGE_FILTER_INVALID');
  // --largest-first lists the same files, sizes falling, unrecorded sizes last.
  const largest = find('--largest-first');
  const sized = file => (typeof file.size_bytes === 'number' ? file.size_bytes : -Infinity);
  assert.deepEqual(largest.map(sized), recorded.map(sized).sort((left, right) => right - left),
    '--largest-first does not list the files largest first');
  // --tag keeps exactly the files of the mail carrying a freshly drawn tag.
  marker = `files-${randomUUID()}`;
  tagged = chosen.message.id;
  skrzynka('message', 'tag', chosen.message.id, '--add', marker);
  assert.deepEqual(find('--tag', marker), recorded.filter(file => file.message_id === chosen.message.id),
    '--tag keeps other files than those of the tagged message');
  // --csv writes the files found as a table, one row per file as find prints
  // it, in order; the same path again is refused and left as it was.
  const filesTable = join(output, 'attachments.csv');
  assert.equal(skrzynka('message', 'attachment', 'find', '--mailbox', values.mailbox, '--csv', filesTable).files,
    recorded.length, 'attachment find --csv counted other files than the search finds');
  const writtenFiles = readFileSync(filesTable, 'utf8');
  const { records: fileRows } = readTable(writtenFiles);
  const asWritten = value => (typeof value === 'string' || typeof value === 'number' ? String(value) : '');
  assert.deepEqual(fileRows, recorded.map(file => Object.fromEntries(Object.entries(file).map(([name, value]) => [name, asWritten(value)]))),
    'attachment find --csv wrote other rows than the search finds');
  refused(['message', 'attachment', 'find', '--mailbox', values.mailbox, '--csv', filesTable], 'ATTACHMENT_OUTPUT_EXISTS');
  assert.equal(readFileSync(filesTable, 'utf8'), writtenFiles, 'The refused --csv path was changed');
  // --save with the tag reads exactly the tagged message's files into a new
  // directory as <message id>-<index>-<name>, each matching its printed
  // digest; the same directory again is refused, a name no file holds is
  // NOT_FOUND.
  const savedFolder = join(output, 'found-files');
  const foundRun = run(values.binary, ['--organization', values.organization, 'message', 'attachment', 'find',
    '--tag', marker, '--save', savedFolder]);
  const foundSaved = JSON.parse(foundRun.stdout);
  if (foundRun.status) assert.equal(JSON.parse(foundRun.stderr).error.code, 'ATTACHMENT_SAVE_INCOMPLETE');
  assert.deepEqual([...foundSaved.saved, ...foundSaved.refused].map(entry => entry.index).sort((a, b) => a - b),
    chosen.listed.map(attachment => attachment.index), 'find --save did not account for every tagged file once');
  for (const entry of foundSaved.saved) {
    assert.equal(hash(readFileSync(entry.path)), entry.sha256, `${entry.path} differs from its printed digest`);
    assert(entry.path.startsWith(join(savedFolder, `${chosen.message.id}-`)), `${entry.path} is not named by its message`);
  }
  refused(['message', 'attachment', 'find', '--tag', marker, '--save', savedFolder], 'ATTACHMENT_OUTPUT_EXISTS');
  refused(['message', 'attachment', 'find', '--name', randomUUID(), '--save', join(output, 'none-found')], 'NOT_FOUND');

  report.observed = { message_id: chosen.message.id, attachment: chosen.attachment,
    sha256: saved.sha256, messages_listed: messages.length };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (marker) {
    report.cleanup_tag = run(values.binary, ['--organization', values.organization,
      'message', 'tag', tagged, '--remove', marker]).status;
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
