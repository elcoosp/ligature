# LIG-W103 — Deep prop chain

Status: lint (M1)

## message
Deep prop chain

## explanation
Props read through two or more field hops re-run the whole chain per update (§6.5).

## suggestion
Flatten the chain in a memo and read the memo in the view.
