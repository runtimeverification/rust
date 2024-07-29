#!/bin/bash

USAGE="usage: $0 <test_dir>\nget run-pass tests"
function die() { printf "%s\n" "$*"; exit 1; }
[ $# -lt 1 ] && die "$USAGE"
TEST_DIR=$1
[ ! -d "$TEST_DIR" ] && die "$USAGE\nthe directory <test_dir=$TEST_DIR> is not readable"
grep -lRs '//@ *run-pass' "$TEST_DIR" | grep '\.rs$'

SKIPPED_TESTS=(
  # serde_json doesn't support 128-bit numbers
  enum-discriminant/issue-70509-partial_eq.rs
  # inconsistent error messages
  inference/inference_unstable.rs
  trivial-bounds/trivial-bounds-inconsistent-sized.rs
  trivial-bounds/trivial-bounds-inconsistent-well-formed.rs
  trivial-bounds/trivial-bounds-inconsistent.rs
  # auxiliary library failed to link/build
  abi/cross-crate/duplicated-external-mods.rs
  abi/cross-crate/anon-extern-mod-cross-crate-2.rs
  abi/extern/extern-crosscrate.rs
  abi/foreign/foreign-dupe.rs
  abi/foreign/invoke-external-foreign.rs
  issues/issue-25185.rs
  # missing support for async closures
  async-await/async-closure.rs
  async-await/async-closures/async-fn-mut-for-async-fn.rs
  async-await/async-closures/async-fn-once-for-async-fn.rs
  async-await/async-closures/await-inference-guidance.rs
  async-await/async-closures/captures.rs
  async-await/async-closures/drop.rs
  async-await/async-closures/mutate.rs
  async-await/async-closures/mut-ref-reborrow.rs
  async-await/async-closures/overlapping-projs.rs
  async-await/async-closures/precise-captures.rs
  async-await/async-drop.rs
  sanitizer/cfi-async-closures.rs
  # missing support for global asm
  asm/simple_global_asm.rs
  asm/empty_global_asm.rs
  asm/x86_64/const.rs
  # needs further instantiation
  async-await/async-await.rs
  async-await/async-fn-size.rs
  async-await/drop-order/drop-order-for-async-fn-parameters-by-ref-binding.rs
  async-await/drop-order/drop-order-for-async-fn-parameters.rs
  async-await/drop-order/drop-order-for-locals-when-cancelled.rs
  async-await/drop-order/drop-order-for-temporary-in-tail-return-expr.rs
  async-await/drop-order/drop-order-when-cancelled.rs
  async-await/futures-api.rs
  async-await/issue-73137.rs
  consts/const-eval/issue-64908.rs
  coroutine/issue-93161.rs
  cross-crate/static-init.rs
  extern/extern-types-thin-pointer.rs
  issues/issue-20427.rs
  issues/issue-27997.rs
  polymorphization/promoted-function.rs
  simd/repr_packed.rs
  traits/multidispatch-infer-convert-target.rs
  # missing support for async closures AND needs further instantiation
  async-await/track-caller/panic-track-caller.rs
)

# Inconsistent entries in link map --- means we're not serializing enough information
#
# cast/codegen-object-shim.rs
# const_prop/dont-propagate-generic-instance.rs
# dyn-star/box.rs
# dyn-star/dyn-star-to-dyn.rs
# rfcs/rfc-2091-track-caller/caller-location-fnptr-rt-ctfe-equiv.rs
# rfcs/rfc-2091-track-caller/tracked-fn-ptr.rs
# rfcs/rfc-2091-track-caller/tracked-fn-ptr-with-arg.rs
# sanitizer/cfi-fn-ptr.rs
#
# Multiple candidates for rmeta dependency libc found - somehow the test build is confused by our non-standard compiler
#
# allocator/no_std-alloc-error-handler-default.rs
# allocator/no_std-alloc-error-handler-custom.rs
