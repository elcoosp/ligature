# LIG-E114 — Function call with callback not admitted in view expressions

Status: reserved (M1 — admission tiers)

## message
Function call with callback not admitted in view expressions

## explanation
Calls with function-valued arguments (`.filter`, `.reduce`, `.sort`, …) are
outside the compiled view subset (§6.5 Deny tier, draft scenario V.2.1).
Allowing them would hide mutable reads from the static dataflow (§7.3).

## suggestion
Move filtering into a memo in the model section, then read it here:

    const visible = createMemo(() => items.filter(i => i.active));
