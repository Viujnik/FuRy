# FuRy engineering checks

- Work in the canonical checkout. Use the Russian mirror as a reference.
- Run `bash scripts/check-rust.sh` and preserve intentional dependency pins in Cargo.lock.
- Run the clean-install interop tests through `bash scripts/check-bindings.sh` for wire, CLI or binding changes. Use `bash scripts/test-postgres-local.sh bindings` to exercise PostgreSQL end to end.
- Preserve public signatures and wire-v2 data. Document compatibility changes and add tests that fail before the fix.
- Compare identical workloads in release builds. Investigate statistically supported slowdowns before merging.
- Require **FuRy required** on `develop`. Record actual checks and tested environments before claiming support.
