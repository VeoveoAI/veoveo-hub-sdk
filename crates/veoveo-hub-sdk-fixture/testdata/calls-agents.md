# Calls Fixture

## Purpose

Ledger answers calls and serves entries that close a while after they open. Relay calls
Ledger for its own caller through the Hub, and reads and follows Ledger's entries.

## Invariants

Ledger and Relay report the identity they received, so tests can check what a delegated
call carries.

## Build And Test

Run the SDK's and the Hub's tests of calls between servers.
