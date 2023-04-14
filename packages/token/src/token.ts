/* eslint-disable max-statements */
/* eslint-disable max-lines */
/* eslint-disable new-cap */
/* eslint-disable @typescript-eslint/prefer-readonly-parameter-types */

import {
  AccountUpdate,
  Bool,
  SmartContract,
  method,
  PublicKey,
  UInt64,
  Account,
  state,
  State,
  VerificationKey,
} from 'snarkyjs';

import type Approvable from './interfaces/token/approvable.js';
// eslint-disable-next-line putout/putout
import type Transferable from './interfaces/token/transferable.js';
// eslint-disable-next-line max-len
// eslint-disable-next-line no-duplicate-imports, @typescript-eslint/consistent-type-imports
import {
  FromToTransferReturn,
  FromTransferReturn,
  MayUseToken,
  ToTransferReturn,
  TransferFromToOptions,
  TransferOptions,
  TransferReturn,
} from './interfaces/token/transferable.js';
import errors from './errors.js';
import {
  AdminAction,
  type Pausable,
  type Burnable,
  type Mintable,
  type Upgradable,
} from './interfaces/token/adminable.js';
// eslint-disable-next-line putout/putout
import type Viewable from './interfaces/token/viewable.js';
// eslint-disable-next-line no-duplicate-imports
import type { ViewableOptions } from './interfaces/token/viewable.js';
import Hooks from './Hooks.js';
import type Hookable from './interfaces/token/hookable.js';

class Token
  extends SmartContract
  implements
    Hookable,
    Mintable,
    Burnable,
    Approvable,
    Transferable,
    Viewable,
    Pausable,
    Upgradable
{
  public static defaultViewableOptions: ViewableOptions = {
    preconditions: { shouldAssertEquals: true },
  };

  // eslint-disable-next-line no-warning-comments
  // TODO: check how many decimals mina has by default
  public static defaultDecimals = 9;

  @state(PublicKey) public hooks = State<PublicKey>();

  @state(UInt64) public totalSupply = State<UInt64>();

  @state(UInt64) public circulatingSupply = State<UInt64>();

  @state(Bool) public paused = State<Bool>();

  public decimals: UInt64 = UInt64.from(Token.defaultDecimals);

  public getHooksContract(): Hooks {
    const admin = this.getHooks();
    return new Hooks(admin);
  }

  @method
  public initialize(hooks: PublicKey, totalSupply: UInt64) {
    this.account.provedState.assertEquals(Bool(false));

    this.hooks.set(hooks);
    this.totalSupply.set(totalSupply);
    this.circulatingSupply.set(UInt64.from(0));
    this.paused.set(Bool(false));
  }

  /**
   * Mintable
   */

  @method
  public mint(to: PublicKey, amount: UInt64): AccountUpdate {
    const hooksContract = this.getHooksContract();
    hooksContract.canAdmin(AdminAction.fromType(AdminAction.types.mint));

    const totalSupply = this.getTotalSupply();
    const circulatingSupply = this.getCirculatingSupply();

    const newCirculatingSupply = circulatingSupply.add(amount);
    newCirculatingSupply.assertLessThanOrEqual(
      totalSupply,
      errors.mintAmountExceedsTotalSupply
    );

    // eslint-disable-next-line no-warning-comments
    // TODO: find out why amount can't be Int64, also for burn
    // eslint-disable-next-line putout/putout
    return this.token.mint({ address: to, amount });
  }

  @method
  public setTotalSupply(amount: UInt64) {
    const hooksContract = this.getHooksContract();
    hooksContract.canAdmin(
      AdminAction.fromType(AdminAction.types.setTotalSupply)
    );

    this.totalSupply.set(amount);
  }

  /**
   * Burnable
   */

  @method
  public burn(from: PublicKey, amount: UInt64): AccountUpdate {
    const hooksContract = this.getHooksContract();
    hooksContract.canAdmin(AdminAction.fromType(AdminAction.types.burn));

    // eslint-disable-next-line putout/putout
    return this.token.mint({ address: from, amount });
  }

  /**
   * Upgradable
   */

  @method
  public setVerificationKey(verificationKey: VerificationKey) {
    const hooksContract = this.getHooksContract();
    hooksContract.canAdmin(
      AdminAction.fromType(AdminAction.types.setVerificationKey)
    );

    this.account.verificationKey.set(verificationKey);
  }

  /**
   * Pausable
   */

  @method
  public setPaused(paused: Bool) {
    const hooksContract = this.getHooksContract();
    hooksContract.canAdmin(AdminAction.fromType(AdminAction.types.setPaused));

    this.paused.set(paused);
  }

  /**
   * Approvable
   */

  // TODO
  public hasNoBalanceChange(accountUpdates: AccountUpdate[]): Bool {
    return Bool(true);
  }

  public assertHasNoBalanceChange(accountUpdates: AccountUpdate[]) {
    this.hasNoBalanceChange(accountUpdates).assertTrue(
      errors.nonZeroBalanceChange
    );
  }

  @method
  public approveTransfer(from: AccountUpdate, to: AccountUpdate): void {
    this.assertHasNoBalanceChange([from, to]);
    this.approve(from, AccountUpdate.Layout.NoChildren);
    this.approve(to, AccountUpdate.Layout.NoChildren);
  }

  @method
  public approveDeploy(deploy: AccountUpdate): void {
    this.assertHasNoBalanceChange([deploy]);
    this.approve(deploy, AccountUpdate.Layout.NoChildren);
  }

  /**
   * Transferable
   */

  @method
  public transferFromTo({
    from,
    to,
    amount,
  }: TransferFromToOptions): FromToTransferReturn {
    const [fromAccountUpdate] = this.transferFrom(
      from,
      amount,
      AccountUpdate.MayUseToken.ParentsOwnToken
    );
    const [, toAccountUpdate] = this.transferTo(
      to,
      amount,
      AccountUpdate.MayUseToken.ParentsOwnToken
    );

    fromAccountUpdate.requireSignature();

    return [fromAccountUpdate, toAccountUpdate];
  }

  public transferFrom(
    from: PublicKey,
    amount: UInt64,
    mayUseToken: MayUseToken
  ): FromTransferReturn {
    const fromAccountUpdate = AccountUpdate.create(from, this.token.id);
    fromAccountUpdate.balance.subInPlace(amount);

    fromAccountUpdate.body.mayUseToken = mayUseToken;

    return [fromAccountUpdate, undefined];
  }

  public transferTo(
    to: PublicKey,
    amount: UInt64,
    mayUseToken: MayUseToken
  ): ToTransferReturn {
    const toAccountUpdate = AccountUpdate.create(to, this.token.id);

    toAccountUpdate.body.mayUseToken = mayUseToken;

    toAccountUpdate.balance.addInPlace(amount);
    return [undefined, toAccountUpdate];
  }

  public transfer({
    from,
    to,
    amount,
    mayUseToken,
  }: TransferOptions): TransferReturn {
    if (!from && !to) {
      throw new Error(errors.fromOrToNotProvided);
    }

    if (from && to) {
      return this.transferFromTo({
        from,
        to,
        amount,
      });
    }

    if (!mayUseToken) {
      throw new Error(errors.mayUseTokenNotProvided);
    }

    if (from && !to) {
      return this.transferFrom(from, amount, mayUseToken);
    }

    if (!to) {
      throw new Error(errors.fromOrToNotProvided);
    }

    return this.transferTo(to, amount, mayUseToken);
  }

  /**
   * Viewable
   */

  public getAccountOf(address: PublicKey): ReturnType<typeof Account> {
    return Account(address, this.token.id);
  }

  public getBalanceOf(
    address: PublicKey,
    { preconditions }: ViewableOptions = Token.defaultViewableOptions
  ): UInt64 {
    const account = this.getAccountOf(address);
    const balance = account.balance.get();

    if (preconditions.shouldAssertEquals) {
      account.balance.assertEquals(balance);
    }

    return balance;
  }

  public getTotalSupply(
    { preconditions }: ViewableOptions = Token.defaultViewableOptions
  ): UInt64 {
    const totalSupply = this.totalSupply.get();

    if (preconditions.shouldAssertEquals) {
      this.totalSupply.assertEquals(totalSupply);
    }

    return totalSupply;
  }

  public getCirculatingSupply(
    { preconditions }: ViewableOptions = Token.defaultViewableOptions
  ): UInt64 {
    const circulatingSupply = this.circulatingSupply.get();

    if (preconditions.shouldAssertEquals) {
      this.circulatingSupply.assertEquals(circulatingSupply);
    }

    return circulatingSupply;
  }

  public getHooks(
    { preconditions }: ViewableOptions = Token.defaultViewableOptions
  ): PublicKey {
    const hooks = this.hooks.get();

    if (preconditions.shouldAssertEquals) {
      this.hooks.assertEquals(hooks);
    }

    return hooks;
  }

  public getPaused(
    { preconditions }: ViewableOptions = Token.defaultViewableOptions
  ): Bool {
    const paused = this.paused.get();

    if (preconditions.shouldAssertEquals) {
      this.paused.assertEquals(paused);
    }

    return paused;
  }

  public getDecimals(): UInt64 {
    return this.decimals;
  }
}

export default Token;
