#!/usr/bin/env node
// Proves `skrzynka message watch` against a dedicated real mailbox. A watch
// of the mailbox is started; two messages of the run are put into its INBOX
// with `message restore` and imported with `sync`, and the running watch
// prints both, in the order they were stored, as `{stored_order, message}`
// lines. A second watch started with `--after` the first message's number
// prints the second message first and never the first. An unknown mailbox is
// refused with NOT_FOUND. The watches are ended by the test once they printed
// what it reads; nothing else is changed in the mailbox but the two messages.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/watch');
mkdirSync(outputRoot, { recursive: true });
const output = mkdtempSync(join(outputRoot, 'run-'));
const report = { started_at: new Date().toISOString(), commands: [], watches: [], status: 'running' };
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

/** A running `message watch`: every line it printed, and a promise per wanted message. */
function startWatch(...extra) {
  const args = cli('message', 'watch', '--mailbox', values.mailbox, ...extra);
  const child = spawn(values.binary, args, { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] });
  const watch = { argv: [values.binary, ...args], lines: [], stderr: '', waiting: [] };
  report.watches.push(watch);
  child.stderr.on('data', chunk => { watch.stderr += chunk; });
  const ended = new Promise(settle => child.on('close', (code, signal) => {
    watch.exit = { code, signal };
    for (const { reject } of watch.waiting) reject(new Error(`message watch ended (${code ?? signal}) before printing what was wanted: ${watch.stderr}`));
    settle();
  }));
  createInterface({ input: child.stdout }).on('line', line => {
    const watched = JSON.parse(line);
    watch.lines.push(watched);
    watch.waiting = watch.waiting.filter(({ wanted, accept }) => {
      if (!wanted(watched)) return true;
      accept(watched);
      return false;
    });
  });
  /** The first printed line `wanted` accepts, printed already or later. */
  const printed = wanted => {
    const already = watch.lines.find(wanted);
    if (already) return Promise.resolve(already);
    return new Promise((accept, reject) => watch.waiting.push({ wanted, accept, reject }));
  };
  const stop = () => { child.kill('SIGTERM'); return ended; };
  return { watch, printed, stop };
}

const runId = randomUUID();
// The run's two messages, put into INBOX in this order.
const messages = {
  first: `The message of run ${runId} restored first.`,
  second: `The message of run ${runId} restored second.`,
};
let failure;
const running = [];
try {
  report.harness_revision = successful('git', ['rev-parse', 'HEAD']).trim();
  report.test_sha256 = hash(readFileSync(fileURLToPath(import.meta.url)));
  assert(values['source-revision']?.trim(), '--source-revision is required: name the exact candidate commit');
  report.source_revision = successful('git', ['rev-parse', '--verify', `${values['source-revision']}^{commit}`]).trim();
  report.binary_identity = JSON.parse(successful(values.binary, ['version']));
  assert.equal(report.binary_identity.product, 'skrzynka', 'Candidate identifies a different product');
  assert.equal(report.binary_identity.source, report.source_revision,
    'Candidate source identity differs from --source-revision; qualify a release built from that commit');
  assert(values.organization && values.mailbox, '--organization and --mailbox are required; name a dedicated real mailbox');

  const unknown = run(values.binary, cli('message', 'watch', '--mailbox', `watch-${runId}@invalid`));
  assert(unknown.status, 'A watch of an unknown mailbox was not refused');
  assert.equal(JSON.parse(unknown.stderr).error.code, 'NOT_FOUND', `An unknown mailbox was refused otherwise: ${unknown.stderr}`);

  const live = startWatch();
  running.push(live);
  const ofRun = name => watched => watched.message.subject === `watch qualification ${runId} ${name}`;

  const input = join(output, 'input');
  mkdirSync(input);
  const exported = Object.entries(messages).map(([name, body]) => {
    const file = `${name}.eml`;
    const text = [`From: watch qualification <watch-${runId}@invalid>`, `To: <${values.mailbox}@invalid>`,
      `Subject: watch qualification ${runId} ${name}`, `Message-ID: <${name}-${runId}@invalid>`,
      `Date: ${new Date().toUTCString()}`, 'Content-Type: text/plain; charset=utf-8', '', body, ''].join('\r\n');
    writeFileSync(join(input, file), text, { flag: 'wx' });
    return { file, sha256: hash(text) };
  });
  writeFileSync(join(input, 'manifest.json'), JSON.stringify({ exported }), { flag: 'wx' });
  skrzynka('message', 'restore', '--mailbox', values.mailbox, '--input', input, '--folder', 'INBOX');
  skrzynka('sync', '--mailbox', values.mailbox);

  const [first, second] = await Promise.all(Object.keys(messages).map(name => live.printed(ofRun(name))));
  assert(first.stored_order < second.stored_order, 'The message restored second was stored before the one restored first');
  const runLines = live.watch.lines.filter(watched => watched.message.subject?.includes(runId));
  assert.equal(runLines.length, new Set(runLines.map(watched => watched.stored_order)).size, 'The watch printed a message twice');
  await live.stop();

  const resumed = startWatch('--after', String(first.stored_order));
  running.push(resumed);
  const again = await resumed.printed(watched => watched.message.id === second.message.id);
  const [opening] = resumed.watch.lines;
  assert(opening.stored_order > first.stored_order, 'The resumed watch printed a message stored before --after');
  assert(!resumed.watch.lines.some(watched => watched.message.id === first.message.id), 'The resumed watch printed the first message again');
  await resumed.stop();

  report.observed = { first: first.stored_order, second: again.stored_order, resumed_first_line: opening.stored_order };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  await Promise.all(running.map(each => each.stop()));
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
