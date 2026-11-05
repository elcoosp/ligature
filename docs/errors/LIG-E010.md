# LIG-E010 — Non-vapor SFC

Status: reserved (M2 — Vue frontend)

## message
`<script setup vapor>` is required; found `{found}`

## explanation
Ligature's Vue frontend admits only Vapor-mode SFCs (§6.2): the virtual-DOM
runtime is not on the AOT path.

## suggestion
Add `vapor` to the `<script setup>` block. A migration note ships with the
M2 frontend.
