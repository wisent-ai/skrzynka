#!/usr/bin/env node
// Changes only Skrzynka's own tags on a dedicated real mailbox's newest
// stored message, and leaves the tags it had; nothing reaches the provider.
// Proves `message tag`, `message tags` and `message list --tag` agree, that a
// repeated add is no change, and the refusals.
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
const outputRoot = join(root, '.build/real-tests/message-tags');
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
let failure;
let tagged;
let tag;
let view;
// A view keeping the tag, and the tag's new name, while the run holds them.
let tagView;
let renamed;
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
  assert(newest, 'The mailbox holds no stored message; sync it first');
  tag = `qualification-${randomUUID()}`;

  tagged = skrzynka('message', 'tag', newest.id, '--add', tag.toUpperCase());
  assert(tagged.tags.includes(tag), 'The added tag is not stored in lower case on the message');
  assert.deepEqual(skrzynka('message', 'show', newest.id).tags, tagged.tags, 'message show disagrees');
  const listed = skrzynka('message', 'list', '--tag', tag);
  assert.deepEqual(listed.map(message => message.id), [newest.id],
    'message list --tag does not keep exactly the tagged message');
  const counted = skrzynka('message', 'tags').find(entry => entry.tag === tag);
  assert(counted && counted.messages === listed.length, 'message tags does not count the tagged messages');
  // --mailbox counts in the tagged message's mailbox only, named by ID or by
  // its own address; a mailbox the organization does not hold is NOT_FOUND.
  const inMailbox = skrzynka('message', 'tags', '--mailbox', newest.mailbox_id).find(entry => entry.tag === tag);
  assert(inMailbox && inMailbox.messages === listed.length, 'message tags --mailbox does not count the tagged message');
  const [ownMailbox] = skrzynka('mailbox', 'list').filter(mailbox => mailbox.id === newest.mailbox_id);
  assert.deepEqual(skrzynka('message', 'tags', '--mailbox', ownMailbox.email).find(entry => entry.tag === tag), inMailbox,
    'message tags --mailbox by address counts otherwise than by ID');
  refused('NOT_FOUND', 'message', 'tags', '--mailbox', randomUUID());
  // --contains the freshly drawn tag, in upper case, keeps it alone; a blank text is refused.
  assert.deepEqual(skrzynka('message', 'tags', '--contains', tag.toUpperCase()).map(entry => entry.tag), [tag],
    'message tags --contains kept other tags than the one holding the text');
  refused('MESSAGE_FILTER_INVALID', 'message', 'tags', '--contains', ' ');
  // --csv writes the tags --contains keeps as a table of tag and count: the
  // drawn tag alone, with the count the listing gives it; the same path again
  // is refused.
  const tagTable = join(output, 'tags.csv');
  const kept = skrzynka('message', 'tags', '--contains', tag);
  assert.equal(skrzynka('message', 'tags', '--contains', tag, '--csv', tagTable).tags, kept.length,
    'message tags --csv counted other tags than --contains keeps');
  assert.deepEqual(readTable(readFileSync(tagTable, 'utf8')).records,
    kept.map(entry => ({ tag: entry.tag, messages: String(entry.messages) })), 'message tags --csv wrote other rows than the listing');
  refused('TAG_OUTPUT_EXISTS', 'message', 'tags', '--contains', tag, '--csv', tagTable);
  // --since the tagged message's receive instant counts it, --until that
  // instant does not; a backwards window is refused.
  const counting = (...window) => skrzynka('message', 'tags', '--contains', tag, ...window)
    .map(entry => [entry.tag, entry.messages]);
  const before = listed.filter(message => Date.parse(message.received_at) < Date.parse(newest.received_at)).length;
  assert.deepEqual(counting('--since', newest.received_at), [[tag, listed.length - before]],
    'message tags --since did not count the tagged mail received from then on');
  assert.deepEqual(counting('--until', newest.received_at), before ? [[tag, before]] : [],
    'message tags --until counted mail received from then on');
  refused('MESSAGE_FILTER_INVALID', 'message', 'tags', '--since', newest.received_at, '--until', newest.received_at);
  assert.deepEqual(skrzynka('message', 'tag', newest.id, '--add', tag).tags, tagged.tags,
    'A repeated add changed the tags');
  // --untagged lists exactly the messages carrying no tag: the tagged one is not among them.
  const carriesNone = message => !message.tags.length;
  const bare = skrzynka('message', 'list', '--mailbox', newest.mailbox_id, '--untagged');
  assert(bare.every(carriesNone), 'message list --untagged kept a tagged message');
  assert(!bare.some(message => message.id === newest.id), 'message list --untagged kept the message just tagged');
  const stored = skrzynka('message', 'list', '--mailbox', newest.mailbox_id);
  assert.deepEqual(bare.map(message => message.id), stored.filter(carriesNone).map(message => message.id),
    'message list --untagged differs from the messages carrying no tag');

  const untagged = skrzynka('message', 'tag', newest.id, '--remove', tag);
  assert(!untagged.tags.includes(tag), 'The removed tag is still on the message');
  assert.deepEqual(untagged.tags, newest.tags, 'Removing the tag did not leave the tags the message had');
  assert.deepEqual(skrzynka('message', 'list', '--tag', tag), [], 'A removed tag still lists the message');
  tagged = undefined;

  refused('MESSAGE_TAG_INVALID', 'message', 'tag', newest.id);
  refused('MESSAGE_TAG_INVALID', 'message', 'tag', newest.id, '--add', ' ');
  refused('MESSAGE_TAG_INVALID', 'message', 'tag', newest.id, '--add', tag, '--remove', tag);
  refused('NOT_FOUND', 'message', 'tag', randomUUID(), '--add', tag);

  // A conversation's messages are tagged together, as `message thread` lists
  // them, and untagged together.
  const conversation = skrzynka('message', 'thread', newest.id).map(message => message.id).sort();
  const threaded = skrzynka('message', 'tag', '--thread', newest.id, '--add', tag);
  assert.deepEqual([threaded.thread, [...threaded.messages].sort()], [newest.id, conversation],
    'tag --thread changed other messages than the conversation holds');
  assert.deepEqual(skrzynka('message', 'list', '--tag', tag, '--include-blocked', '--include-snoozed', '--include-muted')
    .map(message => message.id).sort(), conversation, 'The conversation tag is not on exactly its messages');
  skrzynka('message', 'tag', '--thread', newest.id, '--remove', tag);
  assert.deepEqual(skrzynka('message', 'list', '--tag', tag), [], 'tag --thread --remove left the tag');
  assert(run(values.binary, cli('message', 'tag', newest.id, '--thread', newest.id, '--add', tag)).status,
    'An ID with --thread was accepted');

  // A saved view's messages are tagged together, and untagged together.
  view = `qualification ${randomUUID()}`;
  const kept = skrzynka('message', 'list', '--mailbox', values.mailbox, '--sender', newest.sender,
    '--save-view', view).map(message => message.id);
  const viewed = skrzynka('message', 'tag', '--view', view, '--add', tag);
  assert.deepEqual(viewed.messages, kept, 'tag --view changed other messages than the view lists');
  assert.deepEqual(skrzynka('message', 'list', '--tag', tag).map(message => message.id).sort(),
    [...kept].sort(), 'The view tag is not on exactly the view messages');
  // Renaming carries the tag, and a view naming it, to the new name; removing
  // takes it off every message.
  tagView = `qualification ${randomUUID()}`;
  skrzynka('message', 'list', '--tag', tag, '--save-view', tagView);
  renamed = `${tag}-renamed`;
  const change = skrzynka('message', 'tags', '--rename', tag, '--to', renamed);
  assert.equal(change.messages, kept.length, 'rename did not count the tagged messages');
  assert.deepEqual(skrzynka('message', 'list', '--tag', tag), [], 'The old name still lists messages');
  assert.deepEqual(skrzynka('message', 'list', '--tag', renamed).map(message => message.id).sort(),
    [...kept].sort(), 'The new name is not on exactly the tagged messages');
  assert.equal(skrzynka('message', 'view', 'show', tagView).filters.tag, renamed, 'The view kept the old tag');
  refused('MESSAGE_TAG_NOT_FOUND', 'message', 'tags', '--rename', tag, '--to', renamed);
  assert.equal(skrzynka('message', 'tags', '--remove', renamed).messages, kept.length,
    'remove did not count the tagged messages');
  assert.deepEqual(skrzynka('message', 'list', '--tag', renamed), [], 'tags --remove left the tag');
  refused('MESSAGE_TAG_NOT_FOUND', 'message', 'tags', '--remove', renamed);
  for (const name of [view, tagView]) skrzynka('message', 'view', 'delete', name);
  view = undefined;
  tagView = undefined;
  renamed = undefined;
  refused('VIEW_NOT_FOUND', 'message', 'tag', '--view', randomUUID(), '--add', tag);

  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (view) {
    report.view_cleanup = [
      run(values.binary, cli('message', 'tag', '--view', view, '--remove', tag)).status,
      run(values.binary, cli('message', 'view', 'delete', view)).status,
    ];
  }
  if (renamed) report.rename_cleanup = run(values.binary, cli('message', 'tags', '--remove', renamed)).status;
  if (tagView) report.tag_view_cleanup = run(values.binary, cli('message', 'view', 'delete', tagView)).status;
  if (tagged) {
    const cleanup = run(values.binary, cli('message', 'tag', tagged.id, '--remove', tag));
    report.cleanup = { exit_status: cleanup.status, stderr: cleanup.stderr };
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
