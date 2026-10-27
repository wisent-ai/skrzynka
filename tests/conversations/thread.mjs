#!/usr/bin/env node
// Reads a dedicated real mailbox that already holds a reply to another stored
// message; sends nothing. Proves `message thread` answers both messages of
// such a pair from either side, in receipt order, and only messages the
// headers tie together, and refuses an unknown message.
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
const outputRoot = join(root, '.build/real-tests/message-thread');
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
// Every Message-ID a message carries or names.
const identities = message => [message.message_id, message.in_reply_to, message.references]
  .filter(Boolean).flatMap(header => header.split(/\s+/)).filter(Boolean);
let failure;
let mutedFrom;
let marker;
let markedId;
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
    '--organization and --mailbox are required; name a dedicated real mailbox holding a reply to a stored message');

  const stored = skrzynka('message', 'list', '--mailbox', values.mailbox);
  const byMessageId = new Map(stored.filter(message => message.message_id)
    .map(message => [message.message_id.trim(), message]));
  const reply = stored.find(message => message.in_reply_to && byMessageId.has(message.in_reply_to.trim()));
  assert(reply, 'The mailbox holds no reply to another stored message; receive one first');
  const original = byMessageId.get(reply.in_reply_to.trim());

  for (const start of [reply, original]) {
    const thread = skrzynka('message', 'thread', start.id);
    const ids = thread.map(message => message.id);
    assert(ids.includes(reply.id) && ids.includes(original.id),
      `The conversation of ${start.id} lacks the reply or the message it answers`);
    const received = thread.map(message => message.received_at);
    assert.deepEqual(received, [...received].sort(), `The conversation of ${start.id} is not in receipt order`);
    // Every member shares a Message-ID with another member.
    for (const member of thread) {
      const others = new Set(thread.filter(other => other.id !== member.id).flatMap(identities));
      assert(identities(member).some(identity => others.has(identity)),
        `Message ${member.id} is in the conversation of ${start.id} without a header tying it`);
    }
    report[`thread_of_${start === reply ? 'reply' : 'original'}`] = ids;
  }

  const missing = run(values.binary, ['--organization', values.organization,
    'message', 'thread', randomUUID()]);
  assert(missing.status, 'An unknown message was not refused');
  assert.equal(JSON.parse(missing.stderr).error.code, 'NOT_FOUND');

  // Muting leaves the whole conversation out of listings until it is unmuted.
  const listedIds = (...flags) => skrzynka('message', 'list', '--mailbox', values.mailbox, ...flags)
    .map(message => message.id);
  const conversation = skrzynka('message', 'thread', reply.id).map(message => message.id);
  const muted = skrzynka('message', 'mute', reply.id);
  mutedFrom = reply.id;
  assert.equal(muted.messages, conversation.length, 'mute counted another number of messages');
  assert(muted.changed, 'mute kept no Message-ID as muted');
  const quiet = listedIds();
  assert(conversation.every(id => !quiet.includes(id)), 'A muted conversation is still listed');
  const loud = listedIds('--include-muted');
  assert(conversation.every(id => loud.includes(id)), '--include-muted does not list the muted conversation');
  // --muted lists the muted mail alone: the conversation, and nothing else.
  const only = listedIds('--muted');
  assert(conversation.every(id => only.includes(id)), '--muted does not list the muted conversation');
  assert(only.every(id => !quiet.includes(id)), '--muted lists mail that is not muted');
  assert(!skrzynka('message', 'mute', original.id).changed,
    'Muting the same conversation from its other end kept new Message-IDs');
  skrzynka('message', 'unmute', original.id);
  mutedFrom = undefined;
  const again = listedIds();
  assert(conversation.every(id => again.includes(id)), 'An unmuted conversation is not listed again');
  const notMuted = run(values.binary, ['--organization', values.organization, 'message', 'unmute', reply.id]);
  assert(notMuted.status, 'Unmuting a conversation that is not muted was not refused');
  assert.equal(JSON.parse(notMuted.stderr).error.code, 'CONVERSATION_NOT_MUTED');

  // A saved view of the reply (a freshly drawn tag) mutes its conversation
  // once, a second mute changes none, and unmute --view lists it again.
  marker = `qualification-${randomUUID()}`;
  markedId = reply.id;
  skrzynka('message', 'tag', reply.id, '--add', marker);
  skrzynka('message', 'list', '--tag', marker, '--save-view', marker);
  mutedFrom = reply.id;
  const viewMuted = skrzynka('message', 'mute', '--view', marker);
  assert.deepEqual([viewMuted.conversations.map(change => change.messages), viewMuted.refused], [[conversation.length], []],
    'mute --view changed other conversations than the view keeps');
  assert(conversation.every(id => !listedIds().includes(id)), 'A conversation its view muted is still listed');
  assert.deepEqual(skrzynka('message', 'mute', '--view', marker).conversations, [], 'A second mute --view changed a conversation');
  assert.deepEqual(skrzynka('message', 'unmute', '--view', marker).conversations.map(change => change.messages), [conversation.length],
    'unmute --view unmuted other conversations than the view keeps');
  mutedFrom = undefined;
  assert(conversation.every(id => listedIds().includes(id)), 'A conversation its view unmuted is not listed again');
  skrzynka('message', 'view', 'delete', marker);
  skrzynka('message', 'tag', reply.id, '--remove', marker);
  marker = undefined;

  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (mutedFrom) {
    report.cleanup = run(values.binary, ['--organization', values.organization, 'message', 'unmute', mutedFrom]).status;
  }
  if (marker) {
    const org = ['--organization', values.organization];
    report.view_cleanup = run(values.binary, [...org, 'message', 'view', 'delete', marker]).status;
    report.tag_cleanup = run(values.binary, [...org, 'message', 'tag', markedId, '--remove', marker]).status;
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
