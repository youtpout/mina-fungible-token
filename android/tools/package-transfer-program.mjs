import { gzipSync } from 'node:zlib';
import { readFile, writeFile } from 'node:fs/promises';

const inputPath = process.argv[2];
const outputPath = process.argv[3];
if (inputPath === undefined || outputPath === undefined) {
  throw Error('Usage: node package-transfer-program.mjs <recording.json> <program.json.gz>');
}

const fixture = JSON.parse(await readFile(inputPath, 'utf8'));
const transferCircuit = JSON.stringify(fixture.recordings[0].circuit);
const transferBranch = fixture.compileRecordings.findIndex(
  (recording) => JSON.stringify(recording.circuit) === transferCircuit
);
if (transferBranch < 0) throw Error('The transfer circuit is not a compiled program branch');

const packaged = {
  verificationKeyHash: fixture.verificationKey.hash,
  transferBranch,
  branches: fixture.compileRecordings,
  transferWitnessTemplate: fixture.recordings[0].witness,
};
await writeFile(outputPath, gzipSync(JSON.stringify(packaged), { level: 9 }));
process.stderr.write(
  `Packaged ${packaged.branches.length} branches; transfer is branch ${transferBranch}\n`
);
