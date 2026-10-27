#!/usr/bin/env node
// Keeps a saved view of a dedicated real mailbox's organization, exports it,
// deletes it, imports it back and deletes it again; nothing reaches the
// provider. Proves `message view export` writes the view with its mailbox
// named by address, `message view import` gives it back as it was, and the
// refusals of both.
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
const outputRoot = join(root, '.build/real-tests/view-transfer');
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
  const { error } = JSON.parse(result.stderr);
  assert.equal(error.code, code);
  return error.message;
}
let viewName;
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
  assert(values.organization && values.mailbox, '--organization and --mailbox are required; name a dedicated real mailbox');

  // A view kept to the mailbox, saved as message list --save-view saves one.
  viewName = `transfer ${randomUUID()}`;
  const text = `qualification-${randomUUID()}`;
  skrzynka('message', 'list', '--mailbox', values.mailbox, '--contains', text, '--save-view', viewName);
  const saved = skrzynka('message', 'view', 'show', viewName);
  const address = skrzynka('mailbox', 'list').find(mailbox => mailbox.id === values.mailbox).email;

  // view export writes the view with filters.mailbox_id left empty and the
  // mailbox named by address; the count printed is what it wrote.
  const exported = join(output, 'views.json');
  const written = skrzynka('message', 'view', 'export', '--output', exported, '--contains', viewName);
  const document = JSON.parse(readFileSync(exported, 'utf8'));
  assert.deepEqual(document, [{ name: viewName, mailbox: address, filters: { ...saved.filters, mailbox_id: null } }],
    'view export wrote another document than the view');
  assert.equal(written.views, document.length, 'view export counted other views than it wrote');
  refused('VIEW_OUTPUT_EXISTS', 'message', 'view', 'export', '--output', exported);

  // view import of that file after the view is gone gives it back with its
  // mailbox resolved from the address and the same filters.
  skrzynka('message', 'view', 'delete', viewName);
  refused('VIEW_NOT_FOUND', 'message', 'view', 'show', viewName);
  const imported = skrzynka('message', 'view', 'import', '--input', exported).views;
  assert.deepEqual(imported.map(view => [view.name, view.filters]), [[viewName, saved.filters]],
    'view import kept another view than the export held');
  assert.deepEqual(skrzynka('message', 'view', 'show', viewName).filters, saved.filters, 'the imported view reads back changed');

  // Refusals that keep nothing: an empty list, a view naming no filter, a
  // view naming one twice, filters.mailbox_id set, and an unknown mailbox.
  const refusedImport = (code, name, views) => {
    const file = join(output, `${name}.json`);
    writeFileSync(file, JSON.stringify(views), { flag: 'wx' });
    refused(code, 'message', 'view', 'import', '--input', file);
  };
  const fresh = `transfer ${randomUUID()}`;
  refusedImport('VIEW_INVALID', 'empty', []);
  refusedImport('VIEW_INVALID', 'no-filter', [{ name: fresh, mailbox: null, filters: {} }]);
  refusedImport('VIEW_INVALID', 'twice', [document[0], document[0]]);
  refusedImport('VIEW_INVALID', 'mailbox-id', [{ name: fresh, mailbox: null, filters: { text, mailbox_id: values.mailbox } }]);
  refusedImport('NOT_FOUND', 'unknown-mailbox', [{ name: fresh, mailbox: randomUUID(), filters: { text } }]);
  refused('VIEW_NOT_FOUND', 'message', 'view', 'show', fresh);

  skrzynka('message', 'view', 'delete', viewName);
  viewName = undefined;
  report.observed = { view: saved.name, mailbox: address };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (viewName) report.cleanup = run(values.binary, cli('message', 'view', 'delete', viewName)).status;
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
