# LIG-W105 — Raw HTML on web-capable profiles

Status: lint (M1)

## message
Raw HTML on web-capable profiles

## explanation
The web profile can render raw HTML; the compiler cannot verify its safety.

## suggestion
Sandbox the source before it reaches user data.
