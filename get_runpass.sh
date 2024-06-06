#!/bin/bash

USAGE="usage: $0 <test_dir>\nget run-pass tests"
function die() { printf "%s\n" "$*"; exit 1; }
[ $# -lt 1 ] && die "$USAGE"
TEST_DIR=$1
[ ! -d "$TEST_DIR" ] && die "$USAGE\nthe directory <test_dir=$TEST_DIR> is not readable"
grep -lRs '//@ *run-pass' "$TEST_DIR" | grep '\.rs$'
