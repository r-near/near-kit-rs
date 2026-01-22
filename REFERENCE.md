# near-kit TypeScript Reference

This document captures key implementation patterns from the TypeScript `near-kit` library
that should inform the Rust implementation.

## Source Location

The TypeScript implementation is at `/home/ricky/near-kit`. Key files:

```
/home/ricky/near-kit/
├── src/
│   ├── core/
│   │   ├── near.ts              # Main Near class
│   │   ├── rpc/
│   │   │   └── rpc.ts           # RPC client
│   │   ├── transaction.ts       # TransactionBuilder
│   │   ├── actions.ts           # Action factories
│   │   ├── schema.ts            # Borsh schemas
│   │   ├── types.ts             # Type definitions
│   │   ├── config-schemas.ts    # BlockReference, Finality, etc.
│   │   └── constants.ts         # Network presets, defaults
│   ├── utils/
│   │   ├── amount.ts            # NEAR amount parsing
│   │   ├── validation.ts        # Gas parsing
│   │   └── key.ts               # Key parsing
│   └── errors/
│       └── index.ts             # Error types
```

## Amount Handling (from `/home/ricky/near-kit/src/utils/amount.ts`)

```typescript
// Type definitions for amounts
type NearAmountString = `${number} NEAR`
type YoctoAmountString = `${bigint} yocto`
type AmountInput = NearAmountString | YoctoAmountString | bigint

// Helper namespace
const Amount = {
  NEAR(value: number | `${number}`): NearAmountString {
    return `${value} NEAR`
  },
  yocto(value: bigint | `${bigint}`): YoctoAmountString {
    return `${value} yocto`
  },
  ZERO: "0 yocto" as YoctoAmountString,
  ONE_NEAR: "1 NEAR" as NearAmountString,
  ONE_YOCTO: "1 yocto" as YoctoAmountString,
}

// Parsing function
function parseAmount(amount: AmountInput): string {
  // Returns yoctoNEAR as string
  
  // Handle bigint directly (treated as yoctoNEAR)
  if (typeof amount === "bigint") {
    return amount.toString()
  }
  
  // Parse "X NEAR" format
  const nearMatch = trimmed.match(/^([\d.]+)\s+NEAR$/i)
  if (nearMatch) {
    return parseNearToYocto(nearMatch[1]!)
  }
  
  // Parse "X yocto" format
  const yoctoMatch = trimmed.match(/^(\d+)\s+yocto$/i)
  if (yoctoMatch) {
    return yoctoMatch[1]!
  }
  
  // Error on bare numbers
  if (/^[\d.]+$/.test(trimmed)) {
    throw new Error(`Ambiguous amount: "${amount}". Did you mean "${amount} NEAR"?`)
  }
}
```

## Gas Handling (from `/home/ricky/near-kit/src/utils/validation.ts`)

```typescript
type GasString = `${number} ${"Tgas" | "tgas" | "Ggas" | "ggas" | "gas"}`
type Gas = GasString | bigint

function normalizeGas(gas: Gas): string {
  if (typeof gas === "bigint") {
    return gas.toString()
  }
  
  const tgasMatch = gas.match(/^(\d+)\s*[Tt]gas$/i)
  if (tgasMatch) {
    return (BigInt(tgasMatch[1]!) * 1_000_000_000_000n).toString()
  }
  
  const ggasMatch = gas.match(/^(\d+)\s*[Gg]gas$/i)
  if (ggasMatch) {
    return (BigInt(ggasMatch[1]!) * 1_000_000_000n).toString()
  }
  
  // ... etc
}
```

## Block Reference (from `/home/ricky/near-kit/src/core/config-schemas.ts`)

```typescript
type BlockReference = {
  finality?: "optimistic" | "near-final" | "final"
  blockId?: number | string  // block height or hash
}

// In RPC calls:
const result = await this.call("query", {
  request_type: "view_account",
  ...(options?.blockId
    ? { block_id: options.blockId }
    : { finality: options?.finality || "optimistic" }),
  account_id: accountId,
})
```

## RPC Client Pattern (from `/home/ricky/near-kit/src/core/rpc/rpc.ts`)

```typescript
class RpcClient {
  private readonly url: string
  private readonly retryConfig: RpcRetryConfig
  private requestId: number = 0

  async call<T>(method: string, params: unknown): Promise<T> {
    const request = {
      jsonrpc: "2.0",
      id: ++this.requestId,
      method,
      params,
    }

    // Retry loop with exponential backoff
    for (let attempt = 0; attempt < totalAttempts; attempt++) {
      try {
        const response = await fetch(this.url, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(request),
        })

        if (!response.ok) {
          throw new NetworkError(`HTTP ${response.status}`)
        }

        const data = await response.json()
        
        if (data.error) {
          parseRpcError(data.error)  // Throws appropriate error
        }

        return data.result
      } catch (error) {
        if (isRetryable(error) && attempt < totalAttempts - 1) {
          await sleep(initialDelayMs * 2 ** attempt)
          continue
        }
        throw error
      }
    }
  }

  // High-level methods
  async viewFunction(contractId, methodName, args, options?: BlockReference) {
    const argsBase64 = base64.encode(JSON.stringify(args))
    
    return this.call("query", {
      request_type: "call_function",
      ...(options?.blockId
        ? { block_id: options.blockId }
        : { finality: options?.finality || "final" }),
      account_id: contractId,
      method_name: methodName,
      args_base64: argsBase64,
    })
  }

  async sendTransaction(signedTx: Uint8Array, waitUntil: TxExecutionStatus) {
    return this.call("send_tx", {
      signed_tx_base64: base64.encode(signedTx),
      wait_until: waitUntil,
    })
  }
}
```

## Transaction Builder Pattern (from `/home/ricky/near-kit/src/core/transaction.ts`)

```typescript
class TransactionBuilder {
  private signerId: string
  private actions: Action[] = []
  private receiverId?: string
  
  transfer(receiverId: string, amount: Amount): this {
    this.actions.push(actions.transfer(BigInt(normalizeAmount(amount))))
    if (!this.receiverId) {
      this.receiverId = receiverId
    }
    return this
  }
  
  functionCall(
    contractId: string,
    methodName: string,
    args: object | Uint8Array = {},
    options: { gas?: Gas; attachedDeposit?: Amount } = {},
  ): this {
    const argsBytes = args instanceof Uint8Array
      ? args
      : new TextEncoder().encode(JSON.stringify(args))
    
    this.actions.push(actions.functionCall(
      methodName,
      argsBytes,
      BigInt(normalizeGas(options.gas ?? DEFAULT_GAS)),
      BigInt(normalizeAmount(options.attachedDeposit ?? "0 yocto")),
    ))
    
    if (!this.receiverId) {
      this.receiverId = contractId
    }
    return this
  }
  
  async send(): Promise<FinalExecutionOutcome> {
    // 1. Resolve key pair
    // 2. Get nonce from access key
    // 3. Get block hash
    // 4. Build transaction
    // 5. Sign
    // 6. Send via RPC
  }
}
```

## Action Factories (from `/home/ricky/near-kit/src/core/actions.ts`)

```typescript
// Each action factory returns the Borsh-serializable shape

function transfer(deposit: bigint): TransferAction {
  return { transfer: { deposit } }
}

function functionCall(
  methodName: string,
  args: Uint8Array,
  gas: bigint,
  deposit: bigint,
): FunctionCallAction {
  return {
    functionCall: { methodName, args, gas, deposit },
  }
}

function createAccount(): CreateAccountAction {
  return { createAccount: {} }
}

function deployContract(code: Uint8Array): DeployContractAction {
  return { deployContract: { code } }
}

function addKey(publicKey: PublicKey, permission: AccessKeyPermission): AddKeyAction {
  return {
    addKey: {
      publicKey: publicKeyToZorsh(publicKey),
      accessKey: { nonce: 0n, permission },
    },
  }
}

function deleteKey(publicKey: PublicKey): DeleteKeyAction {
  return { deleteKey: { publicKey: publicKeyToZorsh(publicKey) } }
}

function deleteAccount(beneficiaryId: string): DeleteAccountAction {
  return { deleteAccount: { beneficiaryId } }
}
```

## Error Handling (from `/home/ricky/near-kit/src/errors/`)

```typescript
// Base error class
class NearError extends Error {
  code: string
  retryable: boolean
  
  constructor(message: string, code: string, retryable = false) {
    super(message)
    this.code = code
    this.retryable = retryable
  }
}

// Specific errors
class NetworkError extends NearError {
  statusCode?: number
  
  constructor(message: string, statusCode?: number, retryable = false) {
    super(message, "NETWORK_ERROR", retryable)
    this.statusCode = statusCode
  }
}

class InvalidTransactionError extends NearError {
  failure: unknown
  
  constructor(message: string, failure: unknown) {
    super(message, "INVALID_TRANSACTION")
    this.failure = failure
  }
}

class InvalidKeyError extends NearError {
  constructor(message: string) {
    super(message, "INVALID_KEY")
  }
}

// RPC error parsing
function parseRpcError(error: RpcError, statusCode?: number): never {
  const cause = error.cause?.name
  
  switch (cause) {
    case "UNKNOWN_ACCOUNT":
      throw new AccountNotFoundError(...)
    case "UNKNOWN_ACCESS_KEY":
      throw new AccessKeyNotFoundError(...)
    case "INVALID_NONCE":
      throw new InvalidNonceError(...)
    // ... etc
  }
}
```

## Constants (from `/home/ricky/near-kit/src/core/constants.ts`)

```typescript
// Network presets
const NETWORK_PRESETS = {
  mainnet: {
    rpcUrl: "https://rpc.mainnet.near.org",
    networkId: "mainnet",
  },
  testnet: {
    rpcUrl: "https://rpc.testnet.near.org",
    networkId: "testnet",
  },
  localnet: {
    rpcUrl: "http://localhost:3030",
    networkId: "localnet",
  },
}

// Default gas for function calls: 30 Tgas
const DEFAULT_FUNCTION_CALL_GAS = 30_000_000_000_000n

// yoctoNEAR per NEAR
const YOCTO_PER_NEAR = 1_000_000_000_000_000_000_000_000n
```

## Key Observations for Rust Implementation

1. **Amount/Gas as strings with parsing**: Accept strings like "5 NEAR" and parse at runtime.
   In Rust, implement `FromStr` and `TryFrom<&str>` for `NearToken` and `Gas`.

2. **BlockReference is pervasive**: Almost every query method takes an optional `BlockReference`.
   Make it a builder method on all query types.

3. **Actions are Borsh-serializable structs**: The action factories return shapes that can be
   directly Borsh-serialized. Use Rust enums with `#[derive(BorshSerialize)]`.

4. **RPC errors need careful parsing**: The RPC returns structured errors that need to be
   mapped to specific error types. Look at `rpc-error-handler.ts` for patterns.

5. **Nonce management**: The `NonceManager` class handles concurrent transactions by tracking
   used nonces. Consider implementing similar logic in Rust.

6. **Transaction signing flow**:
   - Get signer's public key
   - Fetch access key (for nonce)
   - Fetch recent block (for block hash)
   - Build transaction struct
   - Serialize with Borsh
   - SHA-256 hash
   - Sign the hash
   - Build SignedTransaction
   - Serialize and send

7. **Wait until semantics**: The `send_tx` RPC method takes a `wait_until` parameter that
   controls when the RPC returns. This is different from `finality` which is for queries.
