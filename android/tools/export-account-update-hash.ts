// Computes the o1js account-update digest and calls hash for a zkApp
// command JSON (the ZkappCommand.toJSON() shape produced by the native
// graphql_json serializer). Used to cross-check the Rust ledger digests:
//
//   npm run task -- android/tools/export-account-update-hash.ts <command.json> [updateIndex]
import { AccountUpdate, Mina } from "o1js"
import * as fs from "node:fs"

const path = process.argv[2] as string
const updateIndex = Number(process.argv[3] ?? "1")
const local = await Mina.LocalBlockchain({ proofsEnabled: false })
Mina.setActiveInstance(local)
console.log("networkId:", Mina.getNetworkId())

const json = JSON.parse(fs.readFileSync(path, "utf8")) as any
const updates: AccountUpdate[] = []
for (const raw of json.accountUpdates as any[]) {
  const parsed = AccountUpdate.fromJSON(raw)
  parsed.body.callDepth = raw.body.callDepth
  updates.push(parsed)
}
const target = updates[updateIndex] as AccountUpdate
const { accountUpdate, calls } = target.toPublicInput({ accountUpdates: updates })
console.log("o1js account_update_hash =", accountUpdate.toString())
console.log("o1js calls_hash =", calls.toString())
