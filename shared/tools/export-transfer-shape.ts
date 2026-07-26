import {
  AccountUpdate,
  Bool,
  Mina,
  PrivateKey,
  UInt64,
  UInt8,
} from "o1js"
import { FungibleToken, FungibleTokenAdmin } from "../../index.js"

const localChain = await Mina.LocalBlockchain({
  proofsEnabled: false,
  enforceTransactionLimits: false,
})
Mina.setActiveInstance(localChain)

const [deployer, sender, receiver] = localChain.testAccounts
const tokenKeypair = PrivateKey.randomKeypair()
const adminKeypair = PrivateKey.randomKeypair()
const token = new FungibleToken(tokenKeypair.publicKey)
const admin = new FungibleTokenAdmin(adminKeypair.publicKey)

const deployTransaction = await Mina.transaction(deployer, async () => {
  AccountUpdate.fundNewAccount(deployer, 3)
  await admin.deploy({ adminPublicKey: adminKeypair.publicKey })
  await token.deploy({ symbol: "TEST", src: "https://example.test/token.ts" })
  await token.initialize(adminKeypair.publicKey, UInt8.from(9), Bool(false))
})
await deployTransaction.prove()
deployTransaction.sign([deployer.key, tokenKeypair.privateKey, adminKeypair.privateKey])
await deployTransaction.send()

const mintTransaction = await Mina.transaction(sender, async () => {
  AccountUpdate.fundNewAccount(sender, 1)
  await token.mint(sender, UInt64.from(2_000_000_000))
})
await mintTransaction.prove()
mintTransaction.sign([sender.key, adminKeypair.privateKey])
await mintTransaction.send()

const transferTransaction = await Mina.transaction(sender, async () => {
  AccountUpdate.fundNewAccount(sender, 1)
  await token.transfer(sender, receiver, UInt64.from(1_000_000_000))
})

const transactionJson = JSON.parse(transferTransaction.toJSON())
if (transactionJson.accountUpdates.length !== 4) {
  throw Error("Expected the transfer transaction to contain four account updates")
}
console.log(JSON.stringify(transactionJson, null, 2))
