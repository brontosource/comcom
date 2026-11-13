#!/bin/sh
# A fake `make` for tests, run as `fake_make.sh -C <build-dir> ...`. Records
# its arguments, one per line, in <build-dir>/args.
printf '%s\n' "$@" > "$2/args"
