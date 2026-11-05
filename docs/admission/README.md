# Admission tiers

The admission-tier schema (which constructs are admitted at which tier, and
their diagnostics) is **M1 scope** (`lig check/build`, NFR-5 diagnostic
fixture set). At M0 the frontend is fail-closed: anything outside the spike
subset produces `LIG-E001` — see `docs/errors/LIG-E001.md` and the fixture
rules in `fixtures/README.md`.

Both frontends feed the same ladder: since M2 the Vue SFC frontend lowers to
the same canonical IR, so `lig check/build` accept `.tsx` and `.vue` sources
alike and the rules apply identically (ADR-014). The NFR-5 fixture cases stay
on the Solid leg; the conformance fixtures pin both legs.
