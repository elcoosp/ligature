# LIG-E001 — Invalid syntax

Status: active

## message
Invalid syntax in {file}

## explanation
The source could not be tokenized or parsed under the admitted frontend grammar.
Ligature is fail-closed (BR-1): a parse error produces no output.

## suggestion
Fix the syntax error at the reported span. The spike frontends admit the
subset documented in the architecture spec §6; anything outside it is a
hard error, never a guess.
