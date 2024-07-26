#!/bin/bash

set -euo pipefail
INTRINSIC_FILES=('library/core/src/intrinsics.rs' 'library/core/src/intrinsics/simd.rs')

# 1. get files which define intrinsic functions
# 2. remove lines that begin with comments
# 3. extract line prefix from 'pub' to 'fn' keyword followed by fun name and then either [<] or [(]
# 4. extract the suffix from each line that does not contain spaces
# 5. remove any extraneous spaces, open parens, or open angle brackets
# 6. remove any duplicates
cat "${INTRINSIC_FILES[@]}"                      \
  | grep -Ev '^( |\t)*/'                         \
  | grep -Eo 'pub[a-z _]*fn [a-zA-Z0-9_]*(<|\()' \
  | grep -Eo '[^ ]*$'                            \
  | sed 's/ \|<\|(//g'                           \
  | sort | uniq
