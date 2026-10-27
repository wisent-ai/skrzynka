#!/usr/bin/env node
// Proves a mailbox's automatic reply against a dedicated real mailbox and a
// sender address the operator names (one that accepts mail). Four messages of
// the run are put into the mailbox's INBOX with `message restore`: two from
// the sender, one stating Auto-Submitted and one from a mailing list. After a
// sync and an explicit responder pass, exactly one of the sender's two
// messages is answered with a reply attempt that ended `sent` carrying the
// reply's text, the other is skipped as answered within the span, and the
// machine and list messages are skipped with why; a second pass decides
// nothing again. Bad settings are refused with AUTO_REPLY_INVALID. The
// mailbox's prior automatic reply is restored at the end.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' }, sender: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/auto-reply');
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
const refusedWith = (code, ...args) => {
  const result = run(values.binary, cli(...args));
  assert(result.status, `${args.join(' ')} was not refused`);
  assert.equal(JSON.parse(result.stderr).error.code, code, `${args.join(' ')} was refused otherwise: ${result.stderr}`);
};
const runId = randomUUID();
const bodyFile = join(output, 'reply.txt');
let failure;
let prior;
try {
  report.harness_revision = successful('git', ['rev-parse', 'HEAD']).trim();
  report.test_sha256 = hash(readFileSync(fileURLToPath(import.meta.url)));
  assert(values['source-revision']?.trim(), '--source-revision is required: name the exact candidate commit');
  report.source_revision = successful('git', ['rev-parse', '--verify', `${values['source-revision']}^{commit}`]).trim();
  report.binary_identity = JSON.parse(successful(values.binary, ['version']));
  assert.equal(report.binary_identity.product, 'skrzynka', 'Candidate identifies a different product');
  assert.equal(report.binary_identity.source, report.source_revision,
    'Candidate source identity differs from --source-revision; qualify a release built from that commit');
  assert(values.organization && values.mailbox && values.sender,
    '--organization, --mailbox and --sender are required; name a dedicated real mailbox and an address that accepts its mail');
  const [, senderDomain] = values.sender.split('@');
  writeFileSync(bodyFile, `Away for the auto-reply qualification ${runId}.\n`, { flag: 'wx' });

  prior = skrzynka('mailbox', 'responder', values.mailbox).auto_reply;
  refusedWith('AUTO_REPLY_INVALID', 'mailbox', 'responder', values.mailbox, '--file', bodyFile, '--every', '0d');
  refusedWith('AUTO_REPLY_INVALID', 'mailbox', 'responder', values.mailbox, '--file', bodyFile, '--every', '7d',
    '--from', '+2d', '--until', '+1d');
  const set = skrzynka('mailbox', 'responder', values.mailbox, '--file', bodyFile, '--every', '7d');
  assert.equal(set.auto_reply.body, readFileSync(bodyFile, 'utf8').trimEnd(), 'The reply reads back otherwise than written');

  // The run's four messages, put into INBOX as an export would put them back.
  const input = join(output, 'input');
  mkdirSync(input);
  const messages = {
    first: { from: values.sender, extra: [] },
    second: { from: values.sender, extra: [] },
    machine: { from: `machine-${runId}@${senderDomain}`, extra: ['Auto-Submitted: auto-generated'] },
    list: { from: `list-${runId}@${senderDomain}`, extra: [`List-Id: <qualification-${runId}.${senderDomain}>`] },
  };
  const exported = Object.entries(messages).map(([name, message]) => {
    const file = `${name}.eml`;
    const text = [`From: ${message.from}`, `To: auto-reply qualification <${values.mailbox}@invalid>`,
      `Subject: auto-reply qualification ${runId} ${name}`, `Message-ID: <${name}-${runId}@${senderDomain}>`,
      `Date: ${new Date().toUTCString()}`, ...message.extra, 'Content-Type: text/plain; charset=utf-8', '',
      `The ${name} message of run ${runId}.`, ''].join('\r\n');
    writeFileSync(join(input, file), text, { flag: 'wx' });
    return { file, sha256: hash(text) };
  });
  writeFileSync(join(input, 'manifest.json'), JSON.stringify({ exported }), { flag: 'wx' });
  skrzynka('message', 'restore', '--mailbox', values.mailbox, '--input', input, '--folder', 'INBOX');
  skrzynka('sync', '--mailbox', values.mailbox);
  const stored = skrzynka('message', 'list', '--mailbox', values.mailbox, '--contains', runId);
  const byName = Object.fromEntries(Object.keys(messages).map(name => {
    const found = stored.find(message => message.subject?.endsWith(`${runId} ${name}`));
    assert(found, `The ${name} message of the run was not imported`);
    return [name, found];
  }));

  skrzynka('mailbox', 'responder', values.mailbox, '--answer');
  const decisions = skrzynka('mailbox', 'responder', values.mailbox, '--decisions');
  const decisionOf = name => {
    const decision = decisions.find(each => each.message_id === byName[name].id);
    assert(decision, `The responder decided nothing for the ${name} message`);
    return decision;
  };
  const people = [decisionOf('first'), decisionOf('second')];
  const answered = people.filter(decision => decision.reply_attempt_id);
  assert.equal(answered.length, Math.sign(Infinity), `The sender's two messages were answered ${answered.length} times`);
  const [reply] = answered;
  const attempts = skrzynka('message', 'replies', reply.message_id);
  const attempt = attempts.find(each => each.id === reply.reply_attempt_id);
  assert(attempt, 'The decision names a reply attempt the message does not hold');
  assert.equal(attempt.status, 'sent', `The automatic reply ended ${attempt.status}: ${attempt.error_message}`);
  assert(attempt.body.includes(runId), 'The automatic reply does not carry the reply text');
  const throttled = people.find(decision => !decision.reply_attempt_id);
  assert.match(throttled.skipped, /was answered automatically/, `The second message was skipped otherwise: ${throttled.skipped}`);
  assert.match(decisionOf('machine').skipped, /Auto-Submitted/, 'The machine message was not skipped as machine mail');
  assert.match(decisionOf('list').skipped, /mailing list/, 'The list message was not skipped as list mail');

  const again = skrzynka('mailbox', 'responder', values.mailbox, '--answer');
  assert(!again.some(each => Object.values(byName).some(message => message.id === each.message_id)),
    'A second pass decided a message of the run again');
  report.observed = { set: set.auto_reply, decisions: Object.fromEntries(Object.keys(messages).map(name => [name, decisionOf(name)])) };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (prior !== undefined) {
    const restoreArgs = prior === null ? ['--clear'] : (() => {
      const priorFile = join(output, 'prior.txt');
      writeFileSync(priorFile, prior.body, { flag: 'wx' });
      const every = `${Math.round(prior.resend_after_seconds / (Date.parse('1970-01-01T00:01:00Z') / Date.parse('1970-01-01T00:00:01Z')))}m`;
      return ['--file', priorFile, '--every', every, '--from', prior.starts_at, ...(prior.ends_at ? ['--until', prior.ends_at] : [])];
    })();
    const restored = run(values.binary, cli('mailbox', 'responder', values.mailbox, ...restoreArgs));
    report.restored = { exit_status: restored.status, stderr: restored.stderr };
    if (restored.status && !failure) {
      report.status = 'failed';
      failure = new Error(`The mailbox's prior automatic reply could not be restored: ${restored.stderr}`);
      report.error = failure.message;
    }
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
