#!/usr/bin/env node
// Keeps a template on a dedicated real mailbox's organization, sends one
// message from it to a recipient the operator names, and deletes it. Proves
// save/list/show, the refusals, that --template becomes the sent body, and
// that a deleted template is refused by name.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' }, mailbox: { type: 'string' }, recipient: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/templates');
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
let saved;
// A second template saved to be refused as a rename target.
let other;
// A template with placeholders, and the draft started from it.
let filled;
let draft;
// A template filled from a stored message.
let greeted;
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
  assert(values.organization && values.mailbox && values.recipient,
    '--organization, --mailbox and --recipient are required; name a dedicated real mailbox and a recipient that accepts its mail');

  const name = `qualification ${randomUUID()}`;
  const text = `Sent from a template by the Skrzynka qualification ${randomUUID()}.\n`;
  const file = join(output, 'template.txt');
  writeFileSync(file, text, { flag: 'wx' });
  const blank = join(output, 'blank.txt');
  writeFileSync(blank, ' \n', { flag: 'wx' });

  refused('TEMPLATE_INVALID', 'message', 'template', 'save', ' ', '--file', file);
  refused('TEMPLATE_INVALID', 'message', 'template', 'save', name, '--file', blank);
  refused('TEMPLATE_INVALID', 'message', 'template', 'save', name, '--file', file, '--subject', ' ');

  // The template names its subject, which a send without --subject takes.
  const subject = `Skrzynka template ${name}`;
  saved = skrzynka('message', 'template', 'save', name, '--file', file, '--subject', ` ${subject} `);
  assert.equal(saved.name, name);
  assert.equal(saved.subject, subject, 'The template does not keep its subject trimmed');
  assert.equal(saved.body, text);
  assert(skrzynka('message', 'template', 'list').some(template => template.name === name),
    'template list does not list the saved template');
  // --contains finds it by its name in upper case, every kept template holds
  // the text, a text none holds keeps none, and a blank one is refused.
  const byName = skrzynka('message', 'template', 'list', '--contains', name.toUpperCase());
  assert(byName.some(template => template.name === name), 'template list --contains does not find the template by its name');
  assert(byName.every(template => `${template.name}\n${template.body}`.toLowerCase().includes(name.toLowerCase())),
    'template list --contains kept a template without the text');
  assert.deepEqual(skrzynka('message', 'template', 'list', '--contains', randomUUID()), [],
    'template list --contains kept a template for a text none holds');
  refused('MESSAGE_FILTER_INVALID', 'message', 'template', 'list', '--contains', ' ');
  assert.equal(skrzynka('message', 'template', 'show', name).body, text);

  const sent = skrzynka('message', 'send', '--mailbox', values.mailbox, '--to', values.recipient,
    '--template', name, '--idempotency-key', randomUUID());
  assert.equal(sent.status, 'sent', `The send ended ${sent.status}: ${sent.error_message}`);
  assert.equal(sent.subject, subject, 'A send without --subject did not take the template subject');
  assert.equal(sent.body.trim(), text.trim(), 'The sent body is not the template text');

  // A renamed template keeps its text under the new name only; a name another
  // template carries is refused.
  const renamed = `${name} renamed`;
  saved = skrzynka('message', 'template', 'rename', name, '--to', renamed);
  assert.equal(saved.name, renamed);
  assert.equal(saved.body, text, 'rename changed the template text');
  refused('TEMPLATE_NOT_FOUND', 'message', 'template', 'show', name);
  other = skrzynka('message', 'template', 'save', `${name} other`, '--file', file);
  refused('TEMPLATE_EXISTS', 'message', 'template', 'rename', renamed, '--to', other.name);
  skrzynka('message', 'template', 'delete', other.name);
  other = undefined;

  // A copy keeps the text under the new name too, and the original stays;
  // copying onto a name a template carries, its own included, is refused.
  const copyName = `${name} copy`;
  other = skrzynka('message', 'template', 'copy', renamed, '--to', copyName);
  assert.equal(other.name, copyName);
  assert.equal(other.body, text, 'copy changed the template text');
  assert.equal(other.subject, subject, 'copy did not keep the template subject');
  assert.equal(skrzynka('message', 'template', 'show', renamed).body, text, 'copy changed the original');
  refused('TEMPLATE_EXISTS', 'message', 'template', 'copy', renamed, '--to', renamed);
  refused('TEMPLATE_NOT_FOUND', 'message', 'template', 'copy', `${name} absent`, '--to', `${name} absent copy`);
  skrzynka('message', 'template', 'delete', other.name);
  other = undefined;

  // {{key}} placeholders are listed once each, in order, and filled by --set;
  // a key left without a value, a value for no key and a malformed --set are refused.
  const person = `Person ${randomUUID()}`;
  const topic = `topic ${randomUUID()}`;
  const withKeys = join(output, 'placeholders.txt');
  writeFileSync(withKeys, 'Dear {{ person }}, about {{topic}}. Thanks, {{person}}. {{not a key}}\n', { flag: 'wx' });
  filled = skrzynka('message', 'template', 'save', `${name} filled`, '--file', withKeys, '--subject', 'For {{person}} on {{when}}');
  assert.deepEqual(filled.placeholders, ['person', 'when', 'topic'], 'The template lists other placeholders than its subject and text name');
  // list --placeholder keeps exactly the templates naming that key, case
  // aside: the filled one by its subject's {{when}}, not the plain one;
  // a key none names keeps none and a blank one is refused.
  const naming = skrzynka('message', 'template', 'list', '--placeholder', 'WHEN', '--contains', name);
  assert.deepEqual(naming.map(template => template.name), [filled.name], 'template list --placeholder kept other templates');
  assert.deepEqual(skrzynka('message', 'template', 'list', '--placeholder', randomUUID()), [],
    'template list --placeholder kept a template for a key none names');
  refused('MESSAGE_FILTER_INVALID', 'message', 'template', 'list', '--placeholder', ' ');
  const when = `day ${randomUUID()}`;
  const sets = ['--set', `person=${person}`, '--set', `topic=${topic}`, '--set', `when=${when}`];
  const expected = `Dear ${person}, about ${topic}. Thanks, ${person}. {{not a key}}\n`;
  const rendered = skrzynka('message', 'template', 'render', filled.name, ...sets);
  assert.equal(rendered.body, expected, 'render filled the text otherwise');
  assert.equal(rendered.subject, `For ${person} on ${when}`, 'render filled the subject otherwise');
  refused('TEMPLATE_VALUE_MISSING', 'message', 'template', 'render', filled.name, '--set', `person=${person}`, '--set', `topic=${topic}`);
  refused('TEMPLATE_VALUE_UNUSED', 'message', 'template', 'render', filled.name, ...sets, '--set', 'persn=typo');
  refused('TEMPLATE_VALUE_INVALID', 'message', 'template', 'render', filled.name, '--set', 'person');
  // --message fills {{sender}} from a stored message's From address where no
  // --set does; a --set given wins, and an unknown message is NOT_FOUND.
  const [stored] = skrzynka('message', 'list', '--mailbox', values.mailbox, '--include-blocked');
  if (stored) {
    const greeting = join(output, 'greeting.txt');
    writeFileSync(greeting, 'Hello {{sender}}\n', { flag: 'wx' });
    greeted = skrzynka('message', 'template', 'save', `${name} greeting`, '--file', greeting);
    const greet = greeted;
    // The address a From entry carries: inside its angle brackets, or the entry itself.
    const from = stored.sender.split('<').pop().split('>').shift().trim();
    assert.equal(skrzynka('message', 'template', 'render', greet.name, '--message', stored.id).body, `Hello ${from}\n`,
      'render --message did not fill the sender from the message');
    assert.equal(skrzynka('message', 'template', 'render', greet.name, '--message', stored.id, '--set', `sender=${person}`).body,
      `Hello ${person}\n`, 'A --set given did not win over the message');
    refused('NOT_FOUND', 'message', 'template', 'render', greet.name, '--message', randomUUID());
    skrzynka('message', 'template', 'delete', greet.name);
    greeted = undefined;
  }
  draft = skrzynka('message', 'draft', 'save', '--mailbox', values.mailbox, '--template', filled.name, ...sets);
  assert.equal(draft.body, expected, 'A draft started from the template holds another body');
  assert.equal(draft.subject, `For ${person} on ${when}`, 'A draft started from the template holds another subject');
  skrzynka('message', 'draft', 'delete', draft.id);
  draft = skrzynka('message', 'draft', 'save', '--mailbox', values.mailbox, '--template', filled.name, ...sets,
    '--subject', subject);
  assert.equal(draft.subject, subject, 'A --subject given did not win over the template subject');
  skrzynka('message', 'draft', 'delete', draft.id);
  draft = undefined;
  skrzynka('message', 'template', 'delete', filled.name);
  filled = undefined;
  // A template without a subject leaves a send without --subject nothing to send under.
  other = skrzynka('message', 'template', 'save', `${name} bare`, '--file', file);
  assert.equal(other.subject, null, 'A template saved without --subject names one');
  refused('OUTBOUND_SUBJECT_INVALID', 'message', 'send', '--mailbox', values.mailbox, '--to', values.recipient,
    '--template', other.name, '--idempotency-key', randomUUID());
  // template export writes the templates --contains keeps as JSON
  // [{name, subject, body}]; an existing path is refused; template import
  // of that file gives the template back as it was, and a file naming one
  // template twice is refused and keeps nothing.
  const exported = join(output, 'templates.json');
  const written = skrzynka('message', 'template', 'export', '--output', exported, '--contains', other.name);
  const document = JSON.parse(readFileSync(exported, 'utf8'));
  assert.deepEqual(document, [{ name: other.name, subject: null, body: other.body }],
    'template export wrote another document than the template');
  assert.equal(written.templates, document.length, 'template export counted other templates than it wrote');
  refused('TEMPLATE_OUTPUT_EXISTS', 'message', 'template', 'export', '--output', exported);
  skrzynka('message', 'template', 'delete', other.name);
  const imported = skrzynka('message', 'template', 'import', '--input', exported).templates;
  assert.deepEqual(imported.map(template => [template.name, template.subject, template.body]),
    [[other.name, other.subject, other.body]], 'template import kept another template than the export held');
  const twice = join(output, 'twice.json');
  const repeated = { name: `${name} twice`, body: 'twice' };
  writeFileSync(twice, JSON.stringify([repeated, repeated]));
  refused('TEMPLATE_INVALID', 'message', 'template', 'import', '--input', twice);
  refused('TEMPLATE_NOT_FOUND', 'message', 'template', 'show', repeated.name);
  skrzynka('message', 'template', 'delete', other.name);
  other = undefined;
  assert.equal(skrzynka('message', 'template', 'delete', renamed).name, renamed);
  saved = undefined;
  assert(refused('TEMPLATE_NOT_FOUND', 'message', 'template', 'show', renamed).includes(renamed),
    'The refusal does not name the template');
  refused('TEMPLATE_NOT_FOUND', 'message', 'send', '--mailbox', values.mailbox, '--to', values.recipient,
    '--subject', 'never sent', '--template', renamed, '--idempotency-key', randomUUID());

  report.observed = { template: name, outbound: sent.id };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  for (const left of [saved, other, filled, greeted].filter(Boolean)) {
    report.cleanup = run(values.binary, cli('message', 'template', 'delete', left.name)).status;
  }
  if (draft) run(values.binary, cli('message', 'draft', 'delete', draft.id));
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
