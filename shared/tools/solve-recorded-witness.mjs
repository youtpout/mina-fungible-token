import { readFile } from 'node:fs/promises';
import { poseidonParamsKimchiFp } from '../../../o1js/dist/node/bindings/crypto/constants.js';

const MODULUS = 28948022309329048855892746252171976963363056481941560715954676764349967630337n;
const STRUCTURAL_SEED = 3392518251768960475377392625298437850623664973002200885669375116181514017494n;
const inputPath = process.argv[2] ?? '/tmp/ft-transfer-a.json';
const fixture = JSON.parse(await readFile(inputPath, 'utf8'));
const templateFixture = process.argv[3]
  ? JSON.parse(await readFile(process.argv[3], 'utf8'))
  : undefined;
const recording = fixture.recordings[0];
const expected = recording.witness.map(BigInt);
const witness = templateFixture
  ? templateFixture.recordings[0].witness.map(BigInt)
  : Array(recording.circuit.aux_count);
const protectedIndexes = new Set([0, 1, 2, 3, 5, 6, 7, 9, 10, 12, 25, 1109]);

function mod(value) {
  value %= MODULUS;
  return value < 0n ? value + MODULUS : value;
}

function inverse(value) {
  let a = mod(value);
  let b = MODULUS;
  let x = 1n;
  let y = 0n;
  while (b !== 0n) {
    const q = a / b;
    [a, b] = [b, a % b];
    [x, y] = [y, x - q * y];
  }
  if (a !== 1n) throw Error('field element is not invertible');
  return mod(x);
}

function terms(lc) {
  return (lc.terms ?? []).map(([coefficient, index]) => [BigInt(coefficient), index]);
}

function evaluate(lc, overrideIndex, overrideValue) {
  let value = BigInt(lc.constant ?? 0);
  for (const [coefficient, index] of terms(lc)) {
    const term = index === overrideIndex ? overrideValue : witness[index];
    if (term === undefined) return undefined;
    value += coefficient * term;
  }
  return mod(value);
}

function assign(lc, target) {
  const unknown = [...new Set(terms(lc).map(([, index]) => index).filter((index) => witness[index] === undefined))];
  if (unknown.length === 0) return evaluate(lc) === mod(target);
  if (unknown.length !== 1) return false;
  const index = unknown[0];
  let coefficient = 0n;
  let known = BigInt(lc.constant ?? 0);
  for (const [termCoefficient, termIndex] of terms(lc)) {
    if (termIndex === index) coefficient += termCoefficient;
    else known += termCoefficient * witness[termIndex];
  }
  witness[index] = mod((target - known) * inverse(coefficient));
  return true;
}

function forceAssign(lc, target, index) {
  let coefficient = 0n;
  let known = BigInt(lc.constant ?? 0);
  for (const [termCoefficient, termIndex] of terms(lc)) {
    if (termIndex === index) coefficient += termCoefficient;
    else known += termCoefficient * witness[termIndex];
  }
  if (mod(coefficient) === 0n) return false;
  witness[index] = mod((target - known) * inverse(coefficient));
  return true;
}

function activeGenericVariables(constraint) {
  const indexes = [];
  const add = (lc) => indexes.push(...terms(lc).map(([, index]) => index));
  if (mod(BigInt(constraint.cl)) !== 0n) add(constraint.l);
  if (mod(BigInt(constraint.cr)) !== 0n) add(constraint.r);
  if (mod(BigInt(constraint.co)) !== 0n) add(constraint.o);
  if (mod(BigInt(constraint.m)) !== 0n) {
    add(constraint.l);
    add(constraint.r);
  }
  return [...new Set(indexes)];
}

function genericValue(constraint, index, value) {
  const l = evaluate(constraint.l, index, value);
  const r = evaluate(constraint.r, index, value);
  const o = evaluate(constraint.o, index, value);
  if (l === undefined || r === undefined || o === undefined) return undefined;
  return mod(
    BigInt(constraint.cl) * l +
      BigInt(constraint.cr) * r +
      BigInt(constraint.co) * o +
      BigInt(constraint.m) * l * r +
      BigInt(constraint.c)
  );
}

function solveGeneric(constraint) {
  const unknown = activeGenericVariables(constraint).filter((index) => witness[index] === undefined);
  if (unknown.length === 0) return genericValue(constraint) === 0n;
  if (unknown.length !== 1) return false;
  const index = unknown[0];
  const f0 = genericValue(constraint, index, 0n);
  const f1 = genericValue(constraint, index, 1n);
  const f2 = genericValue(constraint, index, 2n);
  if (f0 === undefined || f1 === undefined || f2 === undefined) return false;
  const quadratic = mod((f2 - 2n * f1 + f0) * inverse(2n));
  if (quadratic !== 0n) return false;
  const linear = mod(f1 - f0);
  witness[index] = mod(-f0 * inverse(linear));
  return true;
}

function solveEqual(constraint) {
  const indexes = [...new Set([...terms(constraint.l), ...terms(constraint.r)].map(([, index]) => index))];
  const unknown = indexes.filter((index) => witness[index] === undefined);
  if (unknown.length === 0) return evaluate(constraint.l) === evaluate(constraint.r);
  if (unknown.length !== 1) return false;
  const index = unknown[0];
  const l0 = evaluate(constraint.l, index, 0n);
  const l1 = evaluate(constraint.l, index, 1n);
  const r0 = evaluate(constraint.r, index, 0n);
  const r1 = evaluate(constraint.r, index, 1n);
  if ([l0, l1, r0, r1].includes(undefined)) return false;
  const constant = mod(l0 - r0);
  const linear = mod(l1 - l0 - (r1 - r0));
  witness[index] = mod(-constant * inverse(linear));
  return true;
}

const roundConstants = poseidonParamsKimchiFp.roundConstants.map((row) => row.map(BigInt));
const mds = poseidonParamsKimchiFp.mds.map((row) => row.map(BigInt));

function power7(value) {
  const square = mod(value * value);
  const fourth = mod(square * square);
  return mod(fourth * square * value);
}

function solvePoseidon(constraint) {
  const initial = constraint.states[0].map((lc) => evaluate(lc));
  if (initial.includes(undefined)) return false;
  const rows = [initial];
  let state = initial;
  for (let round = 0; round < roundConstants.length; round++) {
    const sboxed = state.map(power7);
    state = mds.map((row, i) =>
      mod(row.reduce((sum, coefficient, j) => sum + coefficient * sboxed[j], 0n) + roundConstants[round][i])
    );
    if (round < roundConstants.length - 1) rows.push(state);
  }
  const ordered = [];
  for (let i = 0; i < rows.length; i += 5) {
    const [r0, r1, r2, r3, r4] = rows.slice(i, i + 5);
    ordered.push(r0, r4, r1, r2, r3);
  }
  for (let row = 0; row < ordered.length; row++) {
    for (let column = 0; column < 3; column++) {
      if (!assign(constraint.states[row][column], ordered[row][column])) return false;
    }
  }
  for (let column = 0; column < 3; column++) {
    if (!assign(constraint.last[column], state[column])) return false;
  }
  return true;
}

function repairPoseidon(constraint) {
  const initial = constraint.states[0].map((lc) => evaluate(lc));
  const rows = [initial];
  let state = initial;
  for (let round = 0; round < roundConstants.length; round++) {
    const sboxed = state.map(power7);
    state = mds.map((row, i) =>
      mod(row.reduce((sum, coefficient, j) => sum + coefficient * sboxed[j], 0n) + roundConstants[round][i])
    );
    if (round < roundConstants.length - 1) rows.push(state);
  }
  const ordered = [];
  for (let i = 0; i < rows.length; i += 5) {
    const [r0, r1, r2, r3, r4] = rows.slice(i, i + 5);
    ordered.push(r0, r4, r1, r2, r3);
  }
  for (let row = 1; row < ordered.length; row++) {
    for (let column = 0; column < 3; column++) {
      const variables = terms(constraint.states[row][column]).map(([, index]) => index);
      const index = variables.find((index) => !protectedIndexes.has(index));
      if (index !== undefined) forceAssign(constraint.states[row][column], ordered[row][column], index);
    }
  }
  for (let column = 0; column < 3; column++) {
    const variables = terms(constraint.last[column]).map(([, index]) => index);
    const index = variables.find((index) => !protectedIndexes.has(index));
    if (index !== undefined) forceAssign(constraint.last[column], state[column], index);
  }
}

for (const index of [0, 1, 2, 3, 5, 6, 7, 9, 10, 12, 25]) witness[index] = expected[index];
witness[1109] = STRUCTURAL_SEED;

function seedAmountAssertions() {
  const amount = witness[12];
  witness[404] = 0n;
  witness[405] = inverse(amount);
  witness[407] = 1n;
  witness[409] = amount;
  witness[410] = mod(-1n);
  witness[411] = amount;
  witness[412] = 1n;
  witness[413] = mod(amount - 1n);
  witness[414] = mod(amount - 1n);
  witness[415] = inverse(amount - 1n);
  witness[417] = mod(-amount);
  witness[435] = 0n;
  witness[436] = inverse(amount);
  witness[438] = 1n;
  witness[440] = amount;
  witness[441] = 1n;
  witness[444] = mod(amount + 1n);
  witness[445] = mod(amount + 1n);
  witness[446] = inverse(amount + 1n);
  witness[448] = amount;
}

seedAmountAssertions();

function singletonIndex(lc) {
  const valueTerms = terms(lc);
  return valueTerms.length === 1 && valueTerms[0][0] === 1n && BigInt(lc.constant ?? 0) === 0n
    ? valueTerms[0][1]
    : undefined;
}

function seedIsZeroAssertions() {
  const constraints = recording.circuit.constraints;
  for (let index = 0; index + 2 < constraints.length; index++) {
    const zeroProduct = constraints[index];
    const complement = constraints[index + 1];
    const inverseCheck = constraints[index + 2];
    if (zeroProduct.kind !== 'generic' || complement.kind !== 'generic' || inverseCheck.kind !== 'generic') continue;
    if (
      mod(BigInt(zeroProduct.m)) !== 1n ||
      mod(BigInt(zeroProduct.cl)) !== 0n ||
      mod(BigInt(zeroProduct.cr)) !== 0n ||
      mod(BigInt(zeroProduct.co)) !== 0n ||
      mod(BigInt(zeroProduct.c)) !== 0n
    ) continue;
    const selector = singletonIndex(zeroProduct.l);
    const value = singletonIndex(zeroProduct.r);
    const inverseIndex = singletonIndex(inverseCheck.l);
    const result = singletonIndex(complement.o);
    if ([selector, value, inverseIndex, result].includes(undefined)) continue;
    const fieldValue = witness[value];
    if (fieldValue === undefined) continue;
    witness[selector] = fieldValue === 0n ? 1n : 0n;
    witness[result] = fieldValue === 0n ? 0n : 1n;
    witness[inverseIndex] = fieldValue === 0n ? 0n : inverse(fieldValue);
    protectedIndexes.add(selector);
    protectedIndexes.add(result);
    protectedIndexes.add(inverseIndex);
  }
}

seedIsZeroAssertions();
if (process.env.DEBUG_WITNESS === '1') {
  console.error('protected', [...protectedIndexes].sort((a, b) => a - b).join(','));
}

let progress = true;
while (!templateFixture && progress) {
  progress = false;
  for (const constraint of recording.circuit.constraints) {
    const before = witness.filter((value) => value !== undefined).length;
    if (constraint.kind === 'generic') solveGeneric(constraint);
    else if (constraint.kind === 'equal') solveEqual(constraint);
    else if (constraint.kind === 'endoscalar') {
      const input = evaluate(constraint.input);
      if (input !== undefined) assign(constraint.output, input % (1n << BigInt(constraint.num_bits)));
    } else if (constraint.kind === 'poseidon') solvePoseidon(constraint);
    const after = witness.filter((value) => value !== undefined).length;
    progress ||= after > before;
  }
}


if (templateFixture) {
  for (let pass = 0; pass < 20; pass++) {
    seedAmountAssertions();
    seedIsZeroAssertions();
    let repaired = 0;
    for (let constraintIndex = 0; constraintIndex < recording.circuit.constraints.length; constraintIndex++) {
      seedIsZeroAssertions();
      const constraint = recording.circuit.constraints[constraintIndex];
      if (constraint.kind === 'poseidon') {
        repairPoseidon(constraint);
        continue;
      }
      if (constraint.kind === 'endoscalar') {
        const input = evaluate(constraint.input);
        const candidates = terms(constraint.output).map(([, index]) => index).filter((index) => !protectedIndexes.has(index));
        if (candidates.length > 0) forceAssign(constraint.output, input % (1n << BigInt(constraint.num_bits)), candidates[0]);
        continue;
      }
      const valid = constraint.kind === 'generic'
        ? genericValue(constraint) === 0n
        : evaluate(constraint.l) === evaluate(constraint.r);
      if (valid) continue;
      let candidates;
      if (constraint.kind === 'generic') {
        const output = mod(BigInt(constraint.co)) === 0n ? [] : terms(constraint.o).map(([, index]) => index);
        candidates = [...output, ...activeGenericVariables(constraint)];
      } else {
        candidates = [...terms(constraint.r), ...terms(constraint.l)].map(([, index]) => index);
      }
      candidates = [...new Set(candidates)].filter((index) => !protectedIndexes.has(index));
      for (const index of candidates) {
        if (constraint.kind === 'equal') {
          const l0 = evaluate(constraint.l, index, 0n);
          const l1 = evaluate(constraint.l, index, 1n);
          const r0 = evaluate(constraint.r, index, 0n);
          const r1 = evaluate(constraint.r, index, 1n);
          const linear = mod(l1 - l0 - (r1 - r0));
          if (linear === 0n) continue;
          witness[index] = mod(-(l0 - r0) * inverse(linear));
          repaired++;
          break;
        }
        const f0 = genericValue(constraint, index, 0n);
        const f1 = genericValue(constraint, index, 1n);
        const f2 = genericValue(constraint, index, 2n);
        const quadratic = mod((f2 - 2n * f1 + f0) * inverse(2n));
        const linear = mod(f1 - f0 - quadratic);
        if (quadratic !== 0n || linear === 0n) continue;
        witness[index] = mod(-f0 * inverse(linear));
        repaired++;
        break;
      }
    }
    if (repaired === 0) break;
  }
}

const unresolved = witness.flatMap((value, index) => value === undefined ? [index] : []);
for (let index = 0; index < witness.length; index++) witness[index] ??= 0n;
const differences = [];
for (let index = 0; index < witness.length; index++) {
  if (witness[index] !== expected[index]) differences.push(index);
}
const invalidConstraints = [];
for (let index = 0; index < recording.circuit.constraints.length; index++) {
  const constraint = recording.circuit.constraints[index];
  let valid = true;
  if (constraint.kind === 'generic') valid = genericValue(constraint) === 0n;
  else if (constraint.kind === 'equal') valid = evaluate(constraint.l) === evaluate(constraint.r);
  else if (constraint.kind === 'endoscalar') {
    valid = evaluate(constraint.output) === evaluate(constraint.input) % (1n << BigInt(constraint.num_bits));
  } else if (constraint.kind === 'poseidon') {
    const before = witness.slice();
    valid = solvePoseidon(constraint) && before.every((value, i) => value === witness[i]);
  }
  if (!valid) invalidConstraints.push(index);
}
console.log(
  JSON.stringify({
    witnessSize: witness.length,
    unresolved: unresolved.length,
    firstUnresolved: unresolved.slice(0, 20),
    differences: differences.length,
    firstDifferences: differences.slice(0, 20),
    firstDifferenceValues: differences.slice(0, 10).map((index) => ({
      index,
      generated: witness[index].toString(),
      expected: expected[index].toString(),
    })),
    invalidConstraints,
  })
);
if (invalidConstraints.length > 0) process.exitCode = 1;
