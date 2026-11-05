# LIG-W102 — Dynamic style object

Status: lint (M1)

## message
Dynamic style object

## explanation
Style objects with dynamic values re-create per update; static-valued keys are folded into the const style table (§8.2).

## suggestion
Prefer static classes; keep only genuinely dynamic properties in the object.
