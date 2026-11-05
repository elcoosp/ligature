# Admission tiers

The admission-tier schema (which constructs are admitted at which tier, and
their diagnostics) is **M1 scope** (`lig check/build`, NFR-5 diagnostic
fixture set). At M0 the frontend is fail-closed: anything outside the spike
subset produces `LIG-E001` — see `docs/errors/LIG-E001.md` and the fixture
rules in `fixtures/README.md`.
