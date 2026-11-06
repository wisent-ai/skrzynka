#!/usr/bin/env node
// Two Skrzynka commands open the fleet database at the same moment, as the
// service and a command do when a host starts: both lay out the schema, and
// neither fails on the other's tables. Both run without the identity
// authority's address (SUPABASE_URL and SUPABASE_ANON_KEY removed), which
// only a process serving the loopback API needs. Each answers `status` with
// the schema version the build declares. Reads only; changes nothing but
// the run's own output directory.
//
// Run: node tests/database/concurrent_open.mjs --organization <id>
//        --source-revision <commit> [--binary <path>]
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { values } = parseArgs({ options: {
  organization: { type: 'string' },
  binary: { type: 'string', default: 'skrzynka' }, 'source-revision': { type: 'string' },
} });
const outputRoot = join(root, '.build/real-tests/concurrent-open');
mkdirSync(outputRoot, { recursive: true });
const output = mkdtempSync(join(outputRoot, 'run-'));
const report = { started_at: new Date().toISOString(), commands: [], status: 'running' };
const hash = data => createHash('sha256').update(data).digest('hex');
const environment = { ...process.env };
delete environment.SUPABASE_URL;
delete environment.SUPABASE_ANON_KEY;

function successful(binary, args) {
  const result = spawnSync(binary, args, { cwd: root, encoding: 'utf8', env: environment });
  report.commands.push({ argv: [binary, ...args], exit_status: result.status, signal: result.signal,
    error: result.error?.message, stdout: result.stdout, stderr: result.stderr });
  if (result.error) throw result.error;
  assert(!result.status && !result.signal, `${binary} ${args.join(' ')} failed: ${result.stderr}`);
  return result.stdout;
}

// One command started without waiting for it; settles with what it printed.
function started(binary, args) {
  return new Promise((settle, fail) => {
    const child = spawn(binary, args, { cwd: root, env: environment });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    child.on('error', fail);
    child.on('close', (status, signal) => {
      report.commands.push({ argv: [binary, ...args], exit_status: status, signal, stdout, stderr });
      settle({ status, signal, stdout, stderr });
    });
  });
}

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
    'Candidate source identity differs from --source-revision; qualify a build of that commit');
  assert(values.organization, '--organization is required; name an organization of the fleet database');

  const args = ['--organization', values.organization, 'status'];
  const answers = await Promise.all([started(values.binary, args), started(values.binary, args)]);
  report.observed = answers.map(answer => ({ status: answer.status, signal: answer.signal }));
  for (const [index, answer] of answers.entries()) {
    assert(!answer.status && !answer.signal,
      `opener ${index} failed beside the other: ${answer.stderr}`);
    const status = JSON.parse(answer.stdout);
    assert.equal(status.product, 'skrzynka', `opener ${index} answered for another product`);
    assert(Number.isInteger(status.schema_version) && status.schema_version > 0,
      `opener ${index} answered no schema version: ${answer.stdout}`);
    assert(!answer.stderr.includes('SUPABASE_URL'),
      `opener ${index} asked for the identity authority: ${answer.stderr}`);
  }
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
