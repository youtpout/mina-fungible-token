import { Encoding, Field, Poseidon, PrivateKey, PublicKey, UInt64 } from "o1js"

const sender = PrivateKey.fromBase58(
  "EKFPQBAbjYkjM6p6fEaZAzufQgQs3spvUw1Uyq2Ghta81cpKrfGg",
).toPublicKey()
const receiver = PublicKey.fromBase58(
  "B62qjVQLxt9nYMWGn45mkgwYfcz8e8jvjNCBo11VKJb7vxDNwv5QLPS",
)
const amount = UInt64.from(1_000_000_000)
const blinding = Field(42)

const packedArguments = sender.isOdd
  .toField()
  .mul(2)
  .add(receiver.isOdd.toField())
  .mul(1n << 64n)
  .add(amount.value)
const methodName = Encoding.stringToFields("transfer")
const fields = [
  Field(5),
  sender.x,
  receiver.x,
  packedArguments,
  Field(0),
  ...methodName,
  blinding,
]

console.log(
  JSON.stringify(
    {
      sender: sender.toBase58(),
      receiver: receiver.toBase58(),
      amount: amount.toString(),
      blinding: blinding.toString(),
      fields: fields.map((field) => field.toString()),
      callData: Poseidon.hash(fields).toString(),
    },
    null,
    2,
  ),
)
