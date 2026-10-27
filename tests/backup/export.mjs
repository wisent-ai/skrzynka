#!/usr/bin/env node
// Exports a dedicated real mailbox into a new directory under this run's
// report folder and checks every stored message is either a file matching
// its manifest entry and `message source`, or a refusal with a code.
// Nothing changes at the provider.
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
const outputRoot = join(root, '.build/real-tests/export');
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
let failure;
// The tag and view drawn for the --view export, and the message tagged.
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
    '--organization and --mailbox are required; name a dedicated real mailbox holding stored mail');

  const stored = skrzynka('message', 'list', '--mailbox', values.mailbox,
    '--include-blocked', '--include-snoozed', '--include-muted').map(message => message.id);
  assert(stored.length, 'The mailbox lists no message to export; sync it first');

  const directory = join(output, 'export');
  const exported = run(values.binary, cli('message', 'export', '--mailbox', values.mailbox,
    '--output', directory));
  const manifest = JSON.parse(exported.stdout);
  assert.deepEqual(JSON.parse(readFileSync(join(directory, 'manifest.json'), 'utf8')), manifest,
    'The printed object is not the manifest written');
  if (exported.status) {
    assert.equal(JSON.parse(exported.stderr).error.code, 'EXPORT_INCOMPLETE');
    assert(manifest.refused.length, 'EXPORT_INCOMPLETE with nothing refused');
  } else {
    assert(!manifest.refused.length, 'A whole export lists refusals');
  }
  const covered = [...manifest.exported, ...manifest.refused].map(entry => entry.message_id).sort();
  assert.deepEqual(covered, [...stored].sort(), 'The manifest does not account for every stored message');
  for (const entry of manifest.refused) {
    assert(entry.code && entry.message, `Refused ${entry.message_id} carries no code and sentence`);
  }
  for (const entry of manifest.exported) {
    const bytes = readFileSync(join(directory, entry.file));
    assert.equal(entry.file, `${entry.message_id}.eml`);
    assert.equal(bytes.length, entry.size_bytes, `${entry.file} differs from its manifest size`);
    assert.equal(hash(bytes), entry.sha256, `${entry.file} differs from its manifest digest`);
    const source = join(output, `${entry.message_id}.source.eml`);
    successful(values.binary, cli('message', 'source', entry.message_id, '--output', source));
    assert(readFileSync(source).equals(bytes), `${entry.file} is not the original message source reads`);
  }

  const again = run(values.binary, cli('message', 'export', '--mailbox', values.mailbox,
    '--output', directory));
  assert(again.status, 'Exporting into an existing directory was not refused');
  assert.equal(JSON.parse(again.stderr).error.code, 'EXPORT_OUTPUT_EXISTS');

  // --view exports only the mailbox's messages a saved view lists: here one
  // message given a freshly drawn tag, and a view of that tag.
  const [chosen] = stored;
  marker = `export-${randomUUID()}`;
  skrzynka('message', 'tag', chosen, '--add', marker);
  tagged = chosen;
  skrzynka('message', 'list', '--tag', marker, '--save-view', marker);
  const narrowed = join(output, 'view-export');
  const byView = run(values.binary, cli('message', 'export', '--mailbox', values.mailbox,
    '--output', narrowed, '--view', marker));
  const viewManifest = JSON.parse(byView.stdout);
  assert.equal(viewManifest.view, marker, 'The manifest does not name the view');
  assert.deepEqual([...viewManifest.exported, ...viewManifest.refused].map(entry => entry.message_id), [chosen],
    'export --view accounted for other messages than the view lists');
  const unknown = run(values.binary, cli('message', 'export', '--mailbox', values.mailbox,
    '--output', join(output, 'unknown-view'), '--view', randomUUID()));
  assert(unknown.status, 'An unknown view was not refused');
  assert.equal(JSON.parse(unknown.stderr).error.code, 'VIEW_NOT_FOUND');
  // --tag, by the mailbox's own address, exports the tagged message alone, no view needed.
  const [ownMailbox] = skrzynka('mailbox', 'list').filter(mailbox => mailbox.id === values.mailbox);
  const tagManifest = JSON.parse(run(values.binary, cli('message', 'export', '--mailbox', ownMailbox.email,
    '--output', join(output, 'tag-export'), '--tag', marker)).stdout);
  assert.deepEqual([tagManifest.tag, [...tagManifest.exported, ...tagManifest.refused].map(entry => entry.message_id)],
    [marker, [chosen]], 'export --tag accounted for other messages than the tag marks, or does not name the tag');

  // --thread exports exactly the conversation `message thread` lists.
  const conversation = skrzynka('message', 'thread', chosen).map(message => message.id).sort();
  const threadFolder = join(output, 'thread-export');
  const byThread = run(values.binary, cli('message', 'export', '--mailbox', values.mailbox,
    '--output', threadFolder, '--thread', chosen, '--mbox'));
  const threadManifest = JSON.parse(byThread.stdout);
  assert.equal(threadManifest.thread, chosen, 'The manifest does not name the conversation');
  assert.deepEqual([...threadManifest.exported, ...threadManifest.refused].map(entry => entry.message_id).sort(),
    conversation, 'export --thread accounted for other messages than the conversation holds');
  // --mbox writes every exported original, in the manifest's order, as mboxrd.
  const pad = (value, width) => String(value).padStart(width.length, width === 'dd' ? ' ' : '0');
  const asctime = instant => {
    const at = new Date(instant);
    const [weekday, month] = [at.toLocaleString('en-US', { weekday: 'short', timeZone: 'UTC' }),
      at.toLocaleString('en-US', { month: 'short', timeZone: 'UTC' })];
    return `${weekday} ${month} ${pad(at.getUTCDate(), 'dd')} ${pad(at.getUTCHours(), 'HH')}:${pad(at.getUTCMinutes(), 'MM')}:`
      + `${pad(at.getUTCSeconds(), 'SS')} ${at.getUTCFullYear()}`;
  };
  const expectedMbox = Buffer.concat(threadManifest.exported.map(entry => {
    const received = skrzynka('message', 'show', entry.message_id).received_at;
    const lines = readFileSync(join(threadFolder, entry.file), 'latin1').split('\n')
      .map(line => line.replace(/\r$/, ''))
      .map(line => (/^>*From /.test(line) ? `>${line}` : line));
    return Buffer.from(`From MAILER-DAEMON ${asctime(received)}\n${lines.map(line => `${line}\n`).join('')}\n`, 'latin1');
  }));
  const mboxBytes = readFileSync(join(threadFolder, 'mailbox.mbox'));
  assert(mboxBytes.equals(expectedMbox), 'mailbox.mbox differs from the exported originals written as mboxrd');
  assert.deepEqual(threadManifest.mbox, { file: 'mailbox.mbox', size_bytes: mboxBytes.length, sha256: hash(mboxBytes),
    messages: threadManifest.exported.length }, 'The manifest describes the mbox otherwise than it was written');

  // --since exports exactly the messages `message list --since` keeps for the mailbox.
  const [newestStored] = skrzynka('message', 'list', '--mailbox', values.mailbox,
    '--include-blocked', '--include-snoozed', '--include-muted');
  const since = newestStored.received_at;
  const recent = skrzynka('message', 'list', '--mailbox', values.mailbox, '--since', since,
    '--include-blocked', '--include-snoozed', '--include-muted').map(message => message.id).sort();
  const bySince = run(values.binary, cli('message', 'export', '--mailbox', values.mailbox,
    '--output', join(output, 'since-export'), '--since', since));
  const sinceManifest = JSON.parse(bySince.stdout);
  assert.equal(Date.parse(sinceManifest.since), Date.parse(since), 'The manifest does not name the instant');
  assert.deepEqual([...sinceManifest.exported, ...sinceManifest.refused].map(entry => entry.message_id).sort(),
    recent, 'export --since accounted for other messages than message list --since keeps');
  const badSince = run(values.binary, cli('message', 'export', '--mailbox', values.mailbox,
    '--output', join(output, 'bad-since'), '--since', 'not an instant'));
  assert.equal(JSON.parse(badSince.stderr).error.code, 'MESSAGE_FILTER_INVALID', 'An unreadable --since was not refused');
  // --until the same instant exports exactly what `message list --until` keeps: everything before it.
  const earlier = skrzynka('message', 'list', '--mailbox', values.mailbox, '--until', since,
    '--include-blocked', '--include-snoozed', '--include-muted').map(message => message.id).sort();
  const byUntil = run(values.binary, cli('message', 'export', '--mailbox', values.mailbox,
    '--output', join(output, 'until-export'), '--until', since));
  const untilManifest = JSON.parse(byUntil.stdout);
  assert.equal(Date.parse(untilManifest.until), Date.parse(since), 'The manifest does not name the end instant');
  assert.deepEqual([...untilManifest.exported, ...untilManifest.refused].map(entry => entry.message_id).sort(),
    earlier, 'export --until accounted for other messages than message list --until keeps');
  // A span back from now is read once: the manifest names the instant it
  // stood for, between the moments before and after the export.
  const minuteMs = Date.parse('1970-01-01T00:01:00Z');
  const before = Date.now() - minuteMs;
  const bySpan = run(values.binary, cli('message', 'export', '--mailbox', values.mailbox,
    '--output', join(output, 'span-export'), '--since', '-1m'));
  const after = Date.now() - minuteMs;
  const spanSince = Date.parse(JSON.parse(bySpan.stdout).since);
  assert(spanSince >= before && spanSince <= after, `export --since -1m named ${spanSince}, not an instant a minute back`);

  report.observed = { exported: manifest.exported.length, refused: manifest.refused.length };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (marker) {
    report.cleanup = [run(values.binary, cli('message', 'view', 'delete', marker)).status];
  }
  if (tagged) {
    report.cleanup_tag = run(values.binary, cli('message', 'tag', tagged, '--remove', marker)).status;
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
