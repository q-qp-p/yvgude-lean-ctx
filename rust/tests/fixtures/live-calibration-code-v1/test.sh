#!/bin/sh
set -eu

. ./solution.sh
[ "$(add 2 3)" = "5" ]
[ "$(add -4 7)" = "3" ]
