import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { EXPR, Q, evaluate } from '../src/index.ts';

test('independent expression golden vectors are shared with the C evaluator', async () => {
  const fixture = JSON.parse(await readFile(new URL('./fixtures/expressions.json', import.meta.url), 'utf8'));
  assert.equal(fixture.formatVersion, 1);
  assert.equal(fixture.q, Q);
  const names = new Set(), covered = new Set();
  for (const vector of fixture.vectors) {
    assert.equal(names.has(vector.name), false, vector.name);
    names.add(vector.name);
    assert.equal(evaluate(vector.instructions, vector.variables, vector.lookup), vector.expected, vector.name);
    for (const instruction of vector.instructions) covered.add(instruction.op);
  }
  assert.ok(fixture.vectors.length >= 60);
  for (const op of Object.values(EXPR)) assert.ok(covered.has(op), `opcode ${op}`);
});