# Contributing to near-kit

## Mission

near-kit gives Rust programs NEAR Protocol as small, typed pieces that compose:
reading chain state (`Near`), signing (`Signer`, `SigningNear`), transactions
(`Tx`, `sign`, `send_signed`), contract clients (`#[near_kit::contract]`) and the
protocol's wire types. When something is hard to do, we go one level deeper and
fix the piece underneath instead of adding a shortcut on top of it.

## What belongs in near-kit

Start every feature request with one question: can it already be built from the
pieces near-kit has?

- **Yes, it composes.** Then the answer is a recipe in the guide or in the item's
  docs, not a new method. A shortcut that saves a line is a second way to do the
  same thing, and each one is more surface to learn, document and keep working.
- **No, a piece is sealed.** If you can't reach something you need (a value a type
  holds but won't hand out, a step you can't run on its own, a type you can't
  build), opening that piece belongs here.
- **The protocol layer is wrong or missing.** A wire format that doesn't match
  nearcore or near-sdk, an RPC method near-kit doesn't cover, a retry that can do
  the wrong thing: fix it here.
- **It would break callers later.** A public name, a return type or a generated
  item that could collide with user code gets settled before a release, because
  changing it afterward is a breaking change.

When we turn down a shortcut, the recipe should be easy to find: in the guide, in
the docs of the item people reach for, and in compile errors that point at it.

## Proposing a change

Open an issue before a pull request. Describe the problem you hit and the code you
have to write today; that's usually enough to tell which of the cases above it
falls into.

## Developing

Build, test and lint commands, code style, where code goes and the dependency
policy are in [AGENTS.md](AGENTS.md). Every change to the public API must show up
in `public-api/`; run `scripts/public-api.sh --bless` and commit the diff with the
change. Commits use conventional prefixes (`feat:`, `fix:`, `docs:`, `refactor:`,
`test:`; `!` for breaking).
