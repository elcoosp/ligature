# LIG-E102 — DOM and global access

Status: deny (M1)

## message
DOM and global access

## explanation
window/document/console access and unadmitted event fields leave the compiled subset — static dataflow cannot see them (§7.3).

## suggestion
Read event data through the admitted fields (currentTarget.value, target.value, key); move other DOM access behind a model fn or the native() escape.
