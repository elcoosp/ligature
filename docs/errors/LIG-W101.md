# LIG-W101 — Dynamic spread on an element

Status: lint (M1)

## message
Dynamic spread on an element

## explanation
Object spreads with dynamic values lower to per-key update dynamics — correct, but each key becomes a separate reaction (§6.1).

## suggestion
Write the attributes out explicitly on hot elements.
