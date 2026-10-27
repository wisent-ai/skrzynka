#!/usr/bin/env node
// Syncs a dedicated real mailbox whose provider writes Authentication-Results
// and proves the import recorded the provider's verdicts: at least one stored
// message carries them, each names its checker, and each is either
// method=result or a whole clause kept in detail. Sends nothing.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
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
const outputRoot = join(root, '.build/real-tests/mailbox-authentication');
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
    '--organization and --mailbox are required; name a dedicated real mailbox whose provider writes Authentication-Results');

  skrzynka('sync', '--mailbox', values.mailbox);
  const messages = skrzynka('message', 'list', '--mailbox', values.mailbox);
  const recorded = messages.filter(message => Array.isArray(message.authentication));
  const verdicts = recorded.flatMap(message => message.authentication);
  assert(verdicts.length, 'No stored message of the mailbox carries a recorded Authentication-Results verdict');
  for (const verdict of verdicts) {
    assert(verdict.authserv_id, `A verdict names no checker: ${JSON.stringify(verdict)}`);
    assert((verdict.method && verdict.result) || (!verdict.method && !verdict.result && verdict.detail),
      `A verdict is neither method=result nor a whole clause: ${JSON.stringify(verdict)}`);
  }
  // A single message read carries the same verdicts as the listing.
  const sample = recorded.find(message => message.authentication.length);
  assert.deepEqual(skrzynka('message', 'show', sample.id).authentication, sample.authentication,
    'message show and message list disagree about a message\'s verdicts');

  report.observed = { messages: messages.length, recorded: recorded.length,
    methods: [...new Set(verdicts.map(verdict => verdict.method))] };
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
