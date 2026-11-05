# LIG-E115 — Method not in the pure-builtin whitelist

Status: deny (M1)

## message
Method not in the pure-builtin whitelist

## explanation
Only whitelisted pure methods (join/slice/trim/toUpperCase/toLowerCase/includes/startsWith/endsWith, toFixed/toString) are admitted in view expressions.

## suggestion
Compute the value in a memo, or use one of the whitelisted methods.
