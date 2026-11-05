# LIG-E117 — Raw HTML denied on layout-only profiles

Status: deny (M1)

## message
Raw HTML denied on layout-only profiles

## explanation
The layout profile (taffy) has no DOM primitive to render raw HTML — fail-closed (§6.2/§8.2).

## suggestion
Render text via {expr}, or build against a web-capable host profile.
