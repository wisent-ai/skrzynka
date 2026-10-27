#!/usr/bin/env node
// Writes a draft on a dedicated real mailbox and sends it to a recipient the
// operator names. Proves a change keeps the fields it does not name, a file
// kept with the draft is listed, kept and sent, an incomplete draft is refused
// and kept, and a sent draft becomes a `sent` outbound row and is removed.
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
  organization: { type: 'string' }, mailbox: { type: 'string' }, recipient: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/message-drafts');
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
let draft;
let second;
let again;
let copy;
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
  assert(values.organization && values.mailbox && values.recipient,
    '--organization, --mailbox and --recipient are required; name a dedicated real mailbox and a recipient that accepts its mail');

  const subject = `Skrzynka draft qualification ${randomUUID()}`;
  const filePath = join(output, 'notes.txt');
  const fileBytes = Buffer.from(`Kept with the draft ${randomUUID()}\n`);
  writeFileSync(filePath, fileBytes, { flag: 'wx' });
  const expectedFile = { filename: 'notes.txt', content_type: 'text/plain',
    size_bytes: fileBytes.length, sha256: hash(fileBytes) };
  draft = skrzynka('message', 'draft', 'save', '--mailbox', values.mailbox,
    '--to', values.recipient, '--subject', 'first subject', '--attach', `text/plain=${filePath}`);
  assert.equal(draft.mailbox_id, values.mailbox, 'The draft belongs to another mailbox');
  assert.equal(draft.body, '', 'A draft started without a body holds one');
  assert.deepEqual(draft.attachments, [expectedFile], 'The draft does not list its file as it was read');
  assert(!JSON.stringify(draft).includes(fileBytes.toString('base64')), 'The draft printed its file bytes');

  const changed = skrzynka('message', 'draft', 'update', draft.id, '--subject', subject);
  assert.equal(changed.subject, subject, 'The subject was not changed');
  assert.equal(changed.recipients, draft.recipients, 'Changing the subject changed the recipients');
  assert.deepEqual(changed.attachments, [expectedFile], 'Changing the subject changed the files');
  assert(skrzynka('message', 'draft', 'list', '--mailbox', values.mailbox)
    .some(listed => listed.id === draft.id), 'draft list does not show the draft');
  // --contains finds it by its subject, case aside, and a text it lacks does
  // not; --scheduled leaves it out, since nothing schedules it.
  const listedIds = (...filters) => skrzynka('message', 'draft', 'list', '--mailbox', values.mailbox, ...filters)
    .map(listed => listed.id);
  assert(listedIds('--contains', subject.toUpperCase()).includes(draft.id), 'draft list --contains does not find the draft');
  assert(!listedIds('--contains', randomUUID()).includes(draft.id), 'draft list --contains kept a draft without the text');
  assert(!listedIds('--scheduled').includes(draft.id), 'draft list --scheduled kept an unscheduled draft');
  // --bounced keeps it exactly when its recipient is an address that bounced for good.
  const recipientBounced = skrzynka('message', 'bounced').some(entry => entry.address === values.recipient.trim().toLowerCase());
  assert.equal(listedIds('--bounced').includes(draft.id), recipientBounced,
    'draft list --bounced disagrees with message bounced about the recipient');
  // The recipient takes this draft's send below, so no suppression covers it
  // and --suppressed leaves the draft out.
  assert(!listedIds('--suppressed').includes(draft.id), 'draft list --suppressed kept a draft to a recipient no entry covers');
  refused('MESSAGE_FILTER_INVALID', 'message', 'draft', 'list', '--contains', ' ');
  // --since and --until at the draft's last change split the drafts there:
  // since keeps it, until leaves it out, and a backwards window is refused.
  assert(listedIds('--since', changed.updated_at).includes(draft.id), 'draft list --since its last change left it out');
  assert(!listedIds('--until', changed.updated_at).includes(draft.id), 'draft list --until its last change kept it');
  assert(!listedIds('--until', '-1d').includes(draft.id), 'draft list --until -1d kept a draft changed just now');
  refused('MESSAGE_FILTER_INVALID', 'message', 'draft', 'list', '--since', changed.updated_at, '--until', changed.updated_at);
  // --csv writes the drafts listed as a table: this draft's row carries its
  // subject, recipients and file by name, its body left out; the same path
  // again is refused and left as it was.
  const draftTable = join(output, 'drafts.csv');
  const draftsListed = skrzynka('message', 'draft', 'list', '--mailbox', values.mailbox);
  assert.equal(skrzynka('message', 'draft', 'list', '--mailbox', values.mailbox, '--csv', draftTable).drafts,
    draftsListed.length, 'draft list --csv counted other drafts than the listing');
  const writtenDrafts = readFileSync(draftTable, 'utf8');
  const { header: draftHeader, records: draftRows } = readTable(writtenDrafts);
  assert(!draftHeader.includes('body'), 'draft list --csv wrote the bodies');
  assert.deepEqual(draftRows.map(row => row.id), draftsListed.map(listed => listed.id),
    'draft list --csv rows name other drafts, or in another order');
  const draftRow = draftRows.find(row => row.id === draft.id);
  assert.deepEqual([draftRow.subject, draftRow.recipients, draftRow.attachments],
    [subject, changed.recipients, expectedFile.filename], 'draft list --csv wrote this draft otherwise');
  refused('DRAFT_OUTPUT_EXISTS', 'message', 'draft', 'list', '--mailbox', values.mailbox, '--csv', draftTable);
  assert.equal(readFileSync(draftTable, 'utf8'), writtenDrafts, 'The refused --csv path was changed');
  refused('OUTBOUND_ATTACHMENT_INVALID', 'message', 'draft', 'update', draft.id, '--attach', filePath);
  assert.deepEqual(skrzynka('message', 'draft', 'show', draft.id).attachments, [expectedFile],
    'A refused file changed the files the draft keeps');

  // A draft without a body is held to message send's rules and kept.
  refused('OUTBOUND_BODY_INVALID', 'message', 'draft', 'send', draft.id, '--idempotency-key', randomUUID());
  assert.equal(skrzynka('message', 'draft', 'show', draft.id).id, draft.id, 'A refused send removed the draft');

  const bodyFile = join(output, 'body.txt');
  writeFileSync(bodyFile, 'Written as a draft by the Skrzynka qualification.\n', { flag: 'wx' });
  skrzynka('message', 'draft', 'update', draft.id, '--body-file', bodyFile);
  const sent = skrzynka('message', 'draft', 'send', draft.id, '--idempotency-key', randomUUID());
  assert.equal(sent.status, 'sent', `The draft's send ended ${sent.status}: ${sent.error_message}`);
  assert.equal(sent.subject, subject, 'The outbound row carries another subject');
  assert.deepEqual(sent.attachments, [expectedFile], 'The outbound row does not record the draft\'s file');
  refused('NOT_FOUND', 'message', 'draft', 'show', draft.id);
  draft = undefined;

  // draft again starts a new draft from the sent message, without its files.
  again = skrzynka('message', 'draft', 'again', sent.id);
  assert.equal(again.mailbox_id, sent.mailbox_id, 'draft again chose another mailbox');
  assert.equal(again.recipients, sent.recipients, 'draft again changed the recipients');
  assert.equal(again.subject, sent.subject, 'draft again changed the subject');
  assert.equal(again.body, sent.body, 'draft again changed the body');
  assert.deepEqual(again.attachments, [], 'draft again kept files whose bytes the row does not hold');
  assert(skrzynka('message', 'outbound').some(row => row.id === sent.id), 'draft again changed the sent row');
  refused('NOT_FOUND', 'message', 'draft', 'again', randomUUID());
  skrzynka('message', 'draft', 'delete', again.id);
  again = undefined;

  // --no-attachments removes every file a draft keeps.
  second = skrzynka('message', 'draft', 'save', '--mailbox', values.mailbox,
    '--attach', `text/plain=${filePath}`);
  // draft copy holds the same fields and file, from the same mailbox, and
  // leaves the original; an unknown draft or mailbox is NOT_FOUND.
  const copied = skrzynka('message', 'draft', 'copy', second.id);
  copy = copied;
  assert.notEqual(copied.id, second.id, 'draft copy answered the original');
  for (const field of ['mailbox_id', 'recipients', 'cc', 'bcc', 'subject', 'body', 'attachments']) {
    assert.deepEqual(copied[field], second[field], `draft copy changed ${field}`);
  }
  assert.deepEqual(skrzynka('message', 'draft', 'show', second.id), second, 'draft copy changed the original');
  refused('NOT_FOUND', 'message', 'draft', 'copy', randomUUID());
  refused('NOT_FOUND', 'message', 'draft', 'copy', second.id, '--mailbox', randomUUID());
  skrzynka('message', 'draft', 'delete', copied.id);
  copy = undefined;
  // --with-attachments keeps a draft while it carries a file, and leaves it
  // out once --no-attachments removed them.
  assert(listedIds('--with-attachments').includes(second.id), 'draft list --with-attachments left out a draft with a file');
  assert.deepEqual(skrzynka('message', 'draft', 'update', second.id, '--no-attachments').attachments, [],
    '--no-attachments kept a file');
  assert(!listedIds('--with-attachments').includes(second.id), 'draft list --with-attachments kept a draft without files');
  skrzynka('message', 'draft', 'delete', second.id);
  second = undefined;

  report.observed = { sent: sent.id };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  for (const left of [draft, second, again, copy].filter(Boolean)) {
    const cleanup = run(values.binary, cli('message', 'draft', 'delete', left.id));
    report.cleanup = { exit_status: cleanup.status, stderr: cleanup.stderr };
  }
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
