// Reads a CSV table as RFC 4180 writes it, as a spreadsheet would: quoted
// fields may hold commas, doubled quotes and line breaks; every line ends in
// CRLF. The tests of `--csv` read what Skrzynka wrote through it, so a field
// the writer quoted wrongly fails the test instead of splitting a row.
import assert from 'node:assert/strict';

export function readTable(text) {
  const rows = [];
  let row = [];
  let field = '';
  let quoted = false;
  let quoteSeen = false;
  let lineEnding = false;
  for (const char of text) {
    if (quoteSeen) {
      quoteSeen = false;
      if (char === '"') {
        field += char;
        continue;
      }
      quoted = false;
    }
    if (lineEnding) {
      lineEnding = false;
      assert.equal(char, '\n', 'A CR outside quotes is not followed by LF');
      row.push(field);
      rows.push(row);
      row = [];
      field = '';
      continue;
    }
    if (quoted) {
      if (char === '"') quoteSeen = true;
      else field += char;
    } else if (char === '"') {
      quoted = true;
    } else if (char === ',') {
      row.push(field);
      field = '';
    } else if (char === '\r') {
      lineEnding = true;
    } else {
      field += char;
    }
  }
  assert(!quoted || quoteSeen, 'A quoted field is never closed');
  assert(!lineEnding, 'The table ends in a CR without LF');
  assert.deepEqual([row, field], [[], ''], 'The last line of the table does not end in CRLF');
  const [header, ...records] = rows;
  assert(header, 'The table has no header');
  for (const record of records) {
    assert.equal(record.length, header.length, `A row holds other fields than the header names: ${record.join(',')}`);
  }
  return { header, records: records.map(record => Object.fromEntries(header.map((name, at) => [name, record[at]]))) };
}
