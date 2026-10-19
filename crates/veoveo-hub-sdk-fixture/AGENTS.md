# Observatory Fixture Instructions

Follow the repository and parent fixture instructions.

## Purpose

Qualify an independent server contract without adding domain definitions to the SDK or core.

## Invariants

- Keep scope, reading ID and route vocabulary in the library's contract feature.
- Use synthetic grants and an isolated loopback listener. No installation credentials.
- The fixture declares only implemented resource behavior; no compliance marker grants authority.

## Build And Test

Run the fixture contract and hosted collection tests.
Resolve an independent contract consumer to check dependency isolation.
