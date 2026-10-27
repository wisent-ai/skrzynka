#!/usr/bin/env node
// Changes two messages of a dedicated real mailbox at its provider: one is
// marked unseen and seen and then moved into the named folder, the other is
// deleted. Name only messages that exist to be changed. Proves `message mark`,
// `message move`, `message delete` and `message actions` against the real
// provider, and their refusals.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, 'move-message': { type: 'string' },
  'delete-message': { type: 'string' }, folder: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/message-actions');
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
const message = (...args) => run(values.binary, ['--organization', values.organization, 'message', ...args]);
const read = (...args) => JSON.parse(successful(values.binary,
  ['--organization', values.organization, 'message', ...args]));
function refusedWith(result, code, words) {
  assert(result.status, `${code} was expected and the command succeeded: ${result.stdout}`);
  const { error } = JSON.parse(result.stderr);
  assert.equal(error.code, code);
  assert(error.message.includes(words), `expected "${words}" in: ${error.message}`);
}
function done(record, id, action) {
  assert.equal(record.message_id, id);
  assert.equal(record.action, action);
  assert.equal(record.status, 'done', `${action} ended ${record.status}: ${record.error_message}`);
  assert(record.completed_at, `${action} is done without a completion time`);
  return record;
}
let failure;
let marker;
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
  const moved = values['move-message'];
  const deleted = values['delete-message'];
  assert(values.organization && moved && deleted && values.folder?.trim(),
    '--organization, --move-message, --delete-message and --folder are required: two messages of a dedicated mailbox that may be changed, and a folder its provider lists');
  assert.notEqual(moved, deleted, '--move-message and --delete-message must be two different messages');

  // Refused before anything reaches the provider.
  refusedWith(message('delete', deleted), 'CONFIRMATION_REQUIRED', 'needs --confirm');
  refusedWith(message('move', moved, '--folder', ' '), 'MESSAGE_FOLDER_INVALID', 'the folder to move to is empty');
  refusedWith(message('move', moved, '--folder', 'inbox'), 'MESSAGE_FOLDER_INVALID', 'already in INBOX');
  refusedWith(message('mark', randomUUID(), '--as', 'seen'), 'NOT_FOUND', 'message was not found');
  assert(message('mark', moved, '--as', 'read').status, 'An --as value other than seen, unseen, flagged or unflagged was accepted');

  // Marked unseen, then seen, then flagged and unflagged, each observed at the provider.
  const unseen = done(read('mark', moved, '--as', 'unseen'), moved, 'unseen');
  const seen = done(read('mark', moved, '--as', 'seen'), moved, 'seen');
  const flagged = done(read('mark', moved, '--as', 'flagged'), moved, 'flagged');
  const unflagged = done(read('mark', moved, '--as', 'unflagged'), moved, 'unflagged');

  // A saved view of just this message marks it as its ID does, as its own action.
  marker = `qualification-${randomUUID()}`;
  read('tag', moved, '--add', marker);
  read('list', '--tag', marker, '--save-view', marker);
  const viewed = read('mark', '--view', marker, '--as', 'seen');
  assert.deepEqual(viewed.refused, [], 'mark --view refused the message');
  assert.deepEqual(viewed.actions.map(action => action.message_id), [moved],
    'mark --view changed other messages than the view lists');
  viewed.actions.forEach(action => done(action, moved, 'seen'));
  successful(values.binary, ['--organization', values.organization, 'message', 'view', 'delete', marker]);
  read('tag', moved, '--remove', marker);
  marker = undefined;

  // --thread marks every message of the conversation as its ID would: seen,
  // the gentlest mark, so the conversation's other messages lose nothing.
  const conversation = read('thread', moved).map(member => member.id).sort();
  const threaded = read('mark', '--thread', moved, '--as', 'seen');
  assert.equal(threaded.thread, moved, 'mark --thread names another conversation');
  assert.deepEqual([...threaded.actions, ...threaded.refused].map(entry => entry.message_id).sort(), conversation,
    'mark --thread changed other messages than message thread lists');
  threaded.actions.forEach(action => done(action, action.message_id, 'seen'));
  assert(message('mark', moved, '--thread', moved, '--as', 'seen').status, 'An ID with --thread was accepted');

  // The provider lists the folder, as `mailbox folders` reads it now.
  const { mailbox_id: mailbox } = read('show', moved);
  const folders = JSON.parse(successful(values.binary,
    ['--organization', values.organization, 'mailbox', 'folders', mailbox]));
  const named = folders.find(folder => folder.name === values.folder.trim());
  assert(named, `mailbox folders does not list ${values.folder}: ${folders.map(folder => folder.name).join(', ')}`);
  assert(named.selectable, `${values.folder} is listed as \\Noselect`);
  assert(folders.some(folder => folder.name.toUpperCase() === 'INBOX'), 'mailbox folders does not list INBOX');
  report.folders = folders.map(folder => folder.name);
  assert(message('move', moved, '--folder', values.folder, '--to', 'archive').status, '--folder with --to was accepted');

  // Moved out of INBOX: by the use the provider declares the folder for, when
  // it declares one --to takes, else by its name; the provider's copy is then
  // gone from INBOX either way.
  // A use --to does not take (such as sent) is refused by the parser in plain
  // text before dispatch; a refusal of the core is a JSON error and fails here.
  let byUse = named.special_use ? message('move', moved, '--to', named.special_use) : null;
  if (byUse?.status) {
    assert(!byUse.stderr.trimStart().startsWith('{'), `move --to ${named.special_use} was refused: ${byUse.stderr}`);
    byUse = null;
  }
  report.moved_by = byUse ? `--to ${named.special_use}` : '--folder';
  const move = done(byUse ? JSON.parse(byUse.stdout) : read('move', moved, '--folder', values.folder), moved, 'move');
  assert.equal(move.folder, values.folder.trim());
  refusedWith(message('mark', moved, '--as', 'unseen'), 'MESSAGE_SOURCE_GONE', 'no longer holds UID');
  assert.equal(read('show', moved).id, moved, 'The stored copy of a moved message is gone');

  // The record lists every action, newest first, the refused one as failed.
  const actions = read('actions', moved);
  const asked = [unseen.id, seen.id, flagged.id, unflagged.id, move.id];
  assert.deepEqual(actions.map(row => row.id).filter(id => asked.includes(id)), [...asked].reverse(),
    'The record does not hold the five actions newest first');
  const [latest] = actions;
  assert.equal(latest.status, 'failed');
  assert.equal(latest.error_code, 'MESSAGE_SOURCE_GONE');

  // Deleted at the provider; the stored copy stays.
  done(read('delete', deleted, '--confirm'), deleted, 'delete');
  refusedWith(message('delete', deleted, '--confirm'), 'MESSAGE_SOURCE_GONE', 'no longer holds UID');
  assert.equal(read('show', deleted).id, deleted, 'The stored copy of a deleted message is gone');

  report.observed = { moved, deleted, folder: values.folder, actions: actions.length };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (marker) {
    report.marker_cleanup = [
      message('view', 'delete', marker).status,
      message('tag', values['move-message'], '--remove', marker).status,
    ];
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
