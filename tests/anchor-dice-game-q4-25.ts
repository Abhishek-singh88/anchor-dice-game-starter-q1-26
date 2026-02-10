import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { AnchorDiceGameQ425 } from "../target/types/anchor_dice_game_q4_25";
import nacl from "tweetnacl";

describe("anchor-dice-game-q4-25", () => {
  anchor.setProvider(anchor.AnchorProvider.env());

  const program = anchor.workspace.anchorDiceGameQ425 as Program<AnchorDiceGameQ425>;

  it("happy path", async () => {
    const provider = anchor.getProvider() as anchor.AnchorProvider;
    const connection = provider.connection;
    const house = (provider.wallet as anchor.Wallet).payer;
    const player = anchor.web3.Keypair.generate();

    const airdropSig = await connection.requestAirdrop(
      player.publicKey,
      2 * anchor.web3.LAMPORTS_PER_SOL
    );
    await connection.confirmTransaction(airdropSig, "confirmed");

    const [vault] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), house.publicKey.toBuffer()],
      program.programId
    );

    await program.methods
      .initialize(new anchor.BN(1 * anchor.web3.LAMPORTS_PER_SOL))
      .accounts({
        house: house.publicKey,
        vault,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    const seed = new anchor.BN(12345);
    const roll = 96;
    const amount = new anchor.BN(10_000_000);

    const [bet] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("bet"),
        vault.toBuffer(),
        seed.toArrayLike(Buffer, "le", 16),
      ],
      program.programId
    );

    await program.methods
      .placeBet(seed, roll, amount)
      .accounts({
        player: player.publicKey,
        house: house.publicKey,
        vault,
        bet,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([player])
      .rpc();

    const betAccount = await program.account.bet.fetch(bet);
    const message = Buffer.concat([
      betAccount.player.toBuffer(),
      betAccount.seed.toArrayLike(Buffer, "le", 16),
      betAccount.slot.toArrayLike(Buffer, "le", 8),
      betAccount.amount.toArrayLike(Buffer, "le", 8),
      Buffer.from([betAccount.roll]),
      Buffer.from([betAccount.bump]),
    ]);

    const signature = nacl.sign.detached(message, house.secretKey);
    const edIx = anchor.web3.Ed25519Program.createInstructionWithPrivateKey({
      privateKey: house.secretKey,
      message,
    });

    await program.methods
      .resolveBet(Array.from(signature))
      .accounts({
        house: house.publicKey,
        player: player.publicKey,
        vault,
        bet,
        instructionSysvar: anchor.web3.SYSVAR_INSTRUCTIONS_PUBKEY,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .preInstructions([edIx])
      .rpc();
  });
});
