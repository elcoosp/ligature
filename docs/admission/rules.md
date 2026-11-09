# Admission rule table (§6.5, draft V.7)

The source of truth for the Deny/Lint/Allow ladder. Machine-parsed by
`lig-admission::rules`; the `rules-sync` gate enforces 1:1 with
`fixtures/admission/`. Each row links one allow fixture (the admitted shape)
and one fixture carrying the rule's diagnostic (`deny/` for hard errors,
`lint/` for flagged-but-lowered shapes).

| Code | Tier | Construct | Allow fixture | Deny fixture |
|---|---|---|---|---|
| LIG-E100 | deny | unknown identifier | e100-unknown-identifier/allow | e100-unknown-identifier/deny |
| LIG-E102 | deny | DOM/global access or unadmitted event field | e102-dom-access/allow | e102-dom-access/deny |
| LIG-E114 | deny | call with callback | e114-callback-call/allow | e114-callback-call/deny |
| LIG-E115 | deny | method not in the pure-builtin whitelist | e115-method-whitelist/allow | e115-method-whitelist/deny |
| LIG-E117 | deny | raw HTML on layout-only profiles | e117-html-layout/allow | e117-html-layout/deny |
| LIG-E118 | deny | event handler beyond the list item body (branch-in-item and nested lists — direct item-body handlers admit since M15, composed list bodies since M17) | e118-for-handler/allow | e118-for-handler/deny |
| LIG-W101 | lint | dynamic spread on an element | w101-dynamic-spread/allow | w101-dynamic-spread/lint |
| LIG-W102 | lint | dynamic style object | w102-style-object/allow | w102-style-object/lint |
| LIG-W103 | lint | deep prop chain | w103-prop-chain/allow | w103-prop-chain/lint |
| LIG-W104 | lint | inline Rust escape hatch | w104-native/allow | w104-native/lint |
| LIG-W105 | lint | raw HTML on web-capable profiles | w105-html-web/allow | w105-html-web/lint |

Governance: a new rule requires (1) this row, (2) an allow fixture, (3) a
deny/lint fixture with `expected.json`, (4) an entry in `docs/errors/` —
before the lowering that makes it reachable merges.
