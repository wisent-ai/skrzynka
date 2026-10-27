#!/usr/bin/env node
// Changes only Skrzynka's tag rules and tags on a dedicated real mailbox's
// organization, and takes back what it added; nothing reaches the provider.
// Proves `message rule apply` tags exactly the stored mail a rule matches,
// removing a rule keeps its tags, and the refusals.
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
const outputRoot = join(root, '.build/real-tests/tag-rules');
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
let rule;
let tagged = [];
let tag;
let scoped;
// The rule `rule copy` makes from the mailbox rule; it is never applied.
let copied;
let scopedTag;
let scopedTagged = [];
let addressed;
let addressedTag;
let addressedTagged = [];
let filed;
let filedTag;
let filedTagged = [];
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

  const every = skrzynka('message', 'list', '--include-blocked');
  const [newest] = every.filter(message => message.mailbox_id === values.mailbox);
  assert(newest, 'The mailbox holds no stored message; sync it first');
  const sender = newest.sender.toLowerCase();
  tag = `rule-${randomUUID()}`;

  rule = skrzynka('message', 'rule', 'add', '--tag', tag, '--sender', sender);
  assert(skrzynka('message', 'rule', 'list').some(listed => listed.id === rule.id), 'rule list does not show the rule');
  const applied = skrzynka('message', 'rule', 'apply').rules.find(entry => entry.rule_id === rule.id);
  const expected = every.filter(message => message.sender.toLowerCase().includes(sender)).map(message => message.id);
  assert.equal(applied.messages, expected.length, 'rule apply counted another number of messages');
  tagged = skrzynka('message', 'list', '--tag', tag, '--include-blocked').map(message => message.id);
  assert.deepEqual([...tagged].sort(), [...expected].sort(), 'The rule tagged other messages than it matches');

  // rule update replaces the tag and every condition and keeps the rule's
  // ID and place; one left without a condition, or an unknown rule, is refused.
  const renamedTag = `rule-${randomUUID()}`;
  const phrase = randomUUID();
  const updated = skrzynka('message', 'rule', 'update', rule.id, '--tag', renamedTag, '--contains', phrase.toUpperCase());
  assert.deepEqual(updated, { ...rule, tag: renamedTag, sender: null, contains: phrase },
    'rule update answered otherwise than the rule with the new tag and condition alone');
  assert.deepEqual(skrzynka('message', 'rule', 'list').find(listed => listed.id === rule.id), updated,
    'rule list reads the updated rule otherwise than update answered it');
  refused('TAG_RULE_INVALID', 'message', 'rule', 'update', rule.id, '--tag', renamedTag);
  refused('NOT_FOUND', 'message', 'rule', 'update', randomUUID(), '--tag', renamedTag, '--contains', phrase);
  // rule show reads that one rule exactly as update answered it; an unknown ID is NOT_FOUND.
  assert.deepEqual(skrzynka('message', 'rule', 'show', rule.id), updated, 'rule show reads the rule otherwise than update answered it');
  refused('NOT_FOUND', 'message', 'rule', 'show', randomUUID());

  // A paused rule keeps its place and conditions, is left out of `rule
  // apply` while `--rule` still applies it, keeps when it was first paused,
  // and tags again once resumed.
  const paused = skrzynka('message', 'rule', 'pause', rule.id);
  assert.deepEqual({ ...paused, paused_at: null }, updated, 'Pausing changed the rule otherwise than paused_at');
  assert(paused.paused_at, 'A paused rule does not say when it was paused');
  assert(!skrzynka('message', 'rule', 'apply').rules.some(entry => entry.rule_id === rule.id), 'rule apply ran a paused rule');
  assert.deepEqual(skrzynka('message', 'rule', 'apply', '--rule', rule.id).rules.map(entry => entry.rule_id), [rule.id],
    'rule apply --rule did not apply the paused rule it names');
  assert.equal(skrzynka('message', 'rule', 'pause', rule.id).paused_at, paused.paused_at, 'Pausing again moved when it was paused');
  // rule list --paused holds it and --active does not while it is paused;
  // --contains its tag in upper case lists it alone and a blank text is refused.
  const listedIds = (...filters) => skrzynka('message', 'rule', 'list', ...filters).map(listed => listed.id);
  assert.deepEqual([listedIds('--paused').includes(rule.id), listedIds('--active').includes(rule.id)], [true, false],
    'rule list --paused and --active split the rules elsewhere than at paused_at');
  assert.deepEqual(listedIds('--contains', renamedTag.toUpperCase()), [rule.id], 'rule list --contains kept other rules');
  refused('MESSAGE_FILTER_INVALID', 'message', 'rule', 'list', '--contains', ' ');
  assert.deepEqual(skrzynka('message', 'rule', 'resume', rule.id), updated, 'Resuming did not give the rule back as it was');
  refused('NOT_FOUND', 'message', 'rule', 'pause', randomUUID());

  run(values.binary, cli('message', 'rule', 'remove', rule.id));
  assert(!skrzynka('message', 'rule', 'list').some(listed => listed.id === rule.id), 'The removed rule is still listed');
  rule = undefined;
  assert.equal(skrzynka('message', 'list', '--tag', tag, '--include-blocked').length, tagged.length,
    'Removing the rule took its tags');

  // A rule kept to one mailbox tags only that mailbox's mail, even where
  // another mailbox holds mail from the same sender.
  scopedTag = `rule-${randomUUID()}`;
  scoped = skrzynka('message', 'rule', 'add', '--tag', scopedTag, '--sender', sender, '--mailbox', values.mailbox);
  assert.equal(scoped.mailbox_id, values.mailbox, 'The rule does not name the mailbox it is kept to');
  const scopedExpected = every.filter(message => message.mailbox_id === values.mailbox
    && message.sender.toLowerCase().includes(sender)).map(message => message.id);
  // rule matches lists exactly the stored mail the rule would tag, and tags
  // none of it; an unknown rule is NOT_FOUND.
  const previewed = skrzynka('message', 'rule', 'matches', scoped.id).map(message => message.id);
  assert.deepEqual([...previewed].sort(), [...scopedExpected].sort(), 'rule matches listed other mail than the rule matches');
  assert.deepEqual(skrzynka('message', 'list', '--tag', scopedTag, '--include-blocked'), [], 'rule matches tagged mail');
  refused('NOT_FOUND', 'message', 'rule', 'matches', randomUUID());
  const scopedApplied = skrzynka('message', 'rule', 'apply').rules.find(entry => entry.rule_id === scoped.id);
  assert.equal(scopedApplied.messages, scopedExpected.length, 'A mailbox rule counted another number of messages');
  scopedTagged = skrzynka('message', 'list', '--tag', scopedTag, '--include-blocked').map(message => message.id);
  assert.deepEqual([...scopedTagged].sort(), [...scopedExpected].sort(), 'A mailbox rule tagged mail of another mailbox');
  // rule list --mailbox lists the rules tagging that mailbox's mail: the one
  // kept to it and those of every mailbox, not those kept to another; an
  // unknown mailbox is NOT_FOUND.
  const everyRule = skrzynka('message', 'rule', 'list');
  assert.deepEqual(listedIds('--mailbox', values.mailbox),
    everyRule.filter(listed => listed.mailbox_id === null || listed.mailbox_id === values.mailbox).map(listed => listed.id),
    'rule list --mailbox kept other rules than those tagging the mailbox');
  const [elsewhere] = skrzynka('mailbox', 'list').filter(mailbox => mailbox.id !== values.mailbox);
  if (elsewhere) {
    assert(!listedIds('--mailbox', elsewhere.id).includes(scoped.id), 'rule list --mailbox kept a rule kept to another mailbox');
  }
  refused('NOT_FOUND', 'message', 'rule', 'list', '--mailbox', randomUUID());
  // rule copy makes a new rule with the mailbox rule's conditions and mailbox
  // and the tag asked for, and leaves the rule copied as it was; copying an
  // unknown rule is NOT_FOUND and a blank tag MESSAGE_TAG_INVALID.
  copied = skrzynka('message', 'rule', 'copy', scoped.id, '--tag', `copy-${randomUUID()}`);
  const conditions = entry => [entry.sender, entry.list, entry.contains, entry.recipient, entry.attachment, entry.mailbox_id];
  assert.notEqual(copied.id, scoped.id, 'rule copy answered the rule copied');
  assert.deepEqual(conditions(copied), conditions(scoped), 'rule copy changed the conditions or the mailbox');
  assert.deepEqual(skrzynka('message', 'rule', 'show', scoped.id), scoped, 'rule copy changed the rule copied');
  refused('NOT_FOUND', 'message', 'rule', 'copy', randomUUID(), '--tag', scopedTag);
  refused('MESSAGE_TAG_INVALID', 'message', 'rule', 'copy', scoped.id, '--tag', ' ');
  run(values.binary, cli('message', 'rule', 'remove', copied.id));
  copied = undefined;
  // rule export writes the rules rule list keeps as JSON, the mailbox named by
  // address; an existing path is refused; rule import adds a rule with the
  // same tag, conditions and mailbox; a file holding none, or a rule with
  // no condition, is TAG_RULE_INVALID and adds nothing.
  const exported = join(output, 'tag-rules.json');
  const written = skrzynka('message', 'rule', 'export', '--output', exported, '--contains', scopedTag);
  const address = skrzynka('mailbox', 'list').find(mailbox => mailbox.id === values.mailbox).email;
  const document = JSON.parse(readFileSync(exported, 'utf8'));
  assert.deepEqual(document, [{ tag: scoped.tag, sender: scoped.sender, list: scoped.list, contains: scoped.contains,
    recipient: scoped.recipient, attachment: scoped.attachment, mailbox: address }], 'rule export wrote another document than the rule');
  assert.equal(written.rules, document.length, 'rule export counted other rules than it wrote');
  refused('TAG_RULE_OUTPUT_EXISTS', 'message', 'rule', 'export', '--output', exported);
  [copied] = skrzynka('message', 'rule', 'import', '--input', exported).rules;
  assert.notEqual(copied.id, scoped.id, 'rule import answered the rule exported');
  assert.deepEqual([copied.tag, ...conditions(copied)], [scoped.tag, ...conditions(scoped)], 'rule import changed the rule');
  run(values.binary, cli('message', 'rule', 'remove', copied.id));
  copied = undefined;
  const empty = join(output, 'no-rules.json');
  writeFileSync(empty, JSON.stringify([]));
  refused('TAG_RULE_INVALID', 'message', 'rule', 'import', '--input', empty);
  const bare = join(output, 'bare-rule.json');
  const bareTag = `bare-${randomUUID()}`;
  writeFileSync(bare, JSON.stringify([{ tag: bareTag }]));
  refused('TAG_RULE_INVALID', 'message', 'rule', 'import', '--input', bare);
  assert.deepEqual(skrzynka('message', 'rule', 'list', '--contains', bareTag), [], 'a refused rule import added a rule');
  run(values.binary, cli('message', 'rule', 'remove', scoped.id));
  scoped = undefined;

  // A recipient rule tags the mail whose To or Cc header contains the text:
  // here the mailbox's own address, as mail sent to it names it. A message
  // stating no To or Cc header is addressed to no one and never matches.
  const own = skrzynka('mailbox', 'show', values.mailbox).email.toLowerCase();
  addressedTag = `rule-${randomUUID()}`;
  addressed = skrzynka('message', 'rule', 'add', '--tag', addressedTag, '--recipient', own);
  assert.equal(addressed.recipient, own, 'The rule does not record its recipient condition');
  // apply --rule tags by that rule alone and answers it alone.
  const addressedRun = skrzynka('message', 'rule', 'apply', '--rule', addressed.id).rules;
  assert.deepEqual(addressedRun.map(entry => entry.rule_id), [addressed.id], 'apply --rule applied other rules');
  const [addressedApplied] = addressedRun;
  const names = header => typeof header === 'string' && header.toLowerCase().includes(own);
  const addressedExpected = every.filter(message => names(message.recipients) || names(message.cc))
    .map(message => message.id);
  assert.equal(addressedApplied.messages, addressedExpected.length, 'A recipient rule counted another number of messages');
  addressedTagged = skrzynka('message', 'list', '--tag', addressedTag, '--include-blocked').map(message => message.id);
  assert.deepEqual([...addressedTagged].sort(), [...addressedExpected].sort(), 'A recipient rule tagged other messages');
  run(values.binary, cli('message', 'rule', 'remove', addressed.id));
  addressed = undefined;
  refused('TAG_RULE_INVALID', 'message', 'rule', 'add', '--tag', tag, '--recipient', ' ');

  // An attachment rule tags the mail carrying a file whose recorded name
  // contains the text: here the name of a file the stored mail carries.
  const fileNames = message => (Array.isArray(message.attachments) ? message.attachments : [])
    .map(file => file.filename).filter(name => typeof name === 'string');
  const [carriedName] = every.flatMap(fileNames);
  if (carriedName) {
    const part = carriedName.toLowerCase();
    filedTag = `rule-${randomUUID()}`;
    filed = skrzynka('message', 'rule', 'add', '--tag', filedTag, '--attachment', carriedName.toUpperCase());
    assert.equal(filed.attachment, part, 'The rule does not record its attachment condition in lower case');
    const [filedApplied] = skrzynka('message', 'rule', 'apply', '--rule', filed.id).rules;
    const filedExpected = every.filter(message => fileNames(message).some(name => name.toLowerCase().includes(part)))
      .map(message => message.id);
    assert.equal(filedApplied.messages, filedExpected.length, 'An attachment rule counted another number of messages');
    filedTagged = skrzynka('message', 'list', '--tag', filedTag, '--include-blocked').map(message => message.id);
    assert.deepEqual([...filedTagged].sort(), [...filedExpected].sort(), 'An attachment rule tagged other messages');
    run(values.binary, cli('message', 'rule', 'remove', filed.id));
    filed = undefined;
  }
  refused('TAG_RULE_INVALID', 'message', 'rule', 'add', '--tag', tag, '--attachment', ' ');
  refused('NOT_FOUND', 'message', 'rule', 'add', '--tag', scopedTag, '--sender', sender, '--mailbox', randomUUID());
  refused('TAG_RULE_INVALID', 'message', 'rule', 'add', '--tag', tag);
  refused('TAG_RULE_INVALID', 'message', 'rule', 'add', '--tag', tag, '--sender', ' ');
  refused('NOT_FOUND', 'message', 'rule', 'remove', randomUUID());
  refused('NOT_FOUND', 'message', 'rule', 'apply', '--rule', randomUUID());

  report.observed = { tag, tagged: tagged.length };
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = error.message;
  failure = error;
} finally {
  if (rule) run(values.binary, cli('message', 'rule', 'remove', rule.id));
  if (scoped) run(values.binary, cli('message', 'rule', 'remove', scoped.id));
  if (copied) run(values.binary, cli('message', 'rule', 'remove', copied.id));
  if (addressed) run(values.binary, cli('message', 'rule', 'remove', addressed.id));
  if (filed) run(values.binary, cli('message', 'rule', 'remove', filed.id));
  report.cleanup = tagged.map(id => run(values.binary, cli('message', 'tag', id, '--remove', tag)).status)
    .concat(scopedTagged.map(id => run(values.binary, cli('message', 'tag', id, '--remove', scopedTag)).status))
    .concat(addressedTagged.map(id => run(values.binary, cli('message', 'tag', id, '--remove', addressedTag)).status))
    .concat(filedTagged.map(id => run(values.binary, cli('message', 'tag', id, '--remove', filedTag)).status));
  report.finished_at = new Date().toISOString();
  writeFileSync(join(output, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${report.status}: ${join(output, 'report.json')}`);
}
if (failure) throw failure;
