#!/bin/bash

set -eu

USAGE="$0 <rustc_dir> <out_dir> <source_file> [args...]\nset env var RUST_BIN to custom rustc if desired"
function die() { printf "%s\n" "error: $*"; exit 1; }

[ $# -lt 3 ] && die "$USAGE"
RUST_DIR=$1
OUT_DIR=$2
SOURCE_FILE=$3
shift 3
[ ! -d "$RUST_DIR" ] && die "$USAGE\n<rustc_dir=$RUST_DIR> must be a valid directory"
mkdir -p "$OUT_DIR" 2>/dev/null || die "$USAGE\nunable to create <out_dir=$OUT_DIR>"
[ ! -f "$SOURCE_FILE" ] && die "$USAGE\nunable to find <source_file=$SOURCE_FILE>"

[ command -v rustc 2>/dev/null ] && die "$USAGE\nrustc binary must be available on PATH"
ARCH=$(rustc -vV | grep '^host' | grep -o '[^: ]*$')

[ -z "${RUST_BIN+x}" ] && RUST_BIN="$RUST_DIR/build/$ARCH/stage1/bin/rustc"
[ ! -x "$RUST_BIN" ] && die "$USAGE\nenv var RUST_BIN or default stage1 rustc does not point to executable"

LLVM_COMPONENTS="aarch64 aarch64asmparser aarch64codegen aarch64desc aarch64disassembler aarch64info aarch64utils aggressiveinstcombine all all-targets analysis arm armasmparser armcodegen armdesc armdisassembler arminfo armutils asmparser asmprinter avr avrasmparser avrcodegen avrdesc avrdisassembler avrinfo binaryformat bitreader bitstreamreader bitwriter bpf bpfasmparser bpfcodegen bpfdesc bpfdisassembler bpfinfo cfguard codegen codegentypes core coroutines coverage csky cskyasmparser cskycodegen cskydesc cskydisassembler cskyinfo debuginfobtf debuginfocodeview debuginfodwarf debuginfogsym debuginfologicalview debuginfomsf debuginfopdb demangle dlltooldriver dwarflinker dwarflinkerclassic dwarflinkerparallel dwp engine executionengine extensions filecheck frontenddriver frontendhlsl frontendoffloading frontendopenacc frontendopenmp fuzzercli fuzzmutate globalisel hexagon hexagonasmparser hexagoncodegen hexagondesc hexagondisassembler hexagoninfo hipstdpar instcombine instrumentation interfacestub interpreter ipo irprinter irreader jitlink libdriver lineeditor linker loongarch loongarchasmparser loongarchcodegen loongarchdesc loongarchdisassembler loongarchinfo lto m68k m68kasmparser m68kcodegen m68kdesc m68kdisassembler m68kinfo mc mca mcdisassembler mcjit mcparser mips mipsasmparser mipscodegen mipsdesc mipsdisassembler mipsinfo mirparser msp430 msp430asmparser msp430codegen msp430desc msp430disassembler msp430info native nativecodegen nvptx nvptxcodegen nvptxdesc nvptxinfo objcarcopts objcopy object objectyaml option orcdebugging orcjit orcshared orctargetprocess passes powerpc powerpcasmparser powerpccodegen powerpcdesc powerpcdisassembler powerpcinfo profiledata remarks riscv riscvasmparser riscvcodegen riscvdesc riscvdisassembler riscvinfo riscvtargetmca runtimedyld scalaropts selectiondag sparc sparcasmparser sparccodegen sparcdesc sparcdisassembler sparcinfo support symbolize systemz systemzasmparser systemzcodegen systemzdesc systemzdisassembler systemzinfo tablegen target targetparser textapi textapibinaryreader transformutils vectorize webassembly webassemblyasmparser webassemblycodegen webassemblydesc webassemblydisassembler webassemblyinfo webassemblyutils windowsdriver windowsmanifest x86 x86asmparser x86codegen x86desc x86disassembler x86info x86targetmca xray"
LLVM_FILECHECK="$RUST_DIR/build/$ARCH/ci-llvm/bin/FileCheck"
LLVM_VERSION=$("$LLVM_FILECHECK" --version | grep -o "LLVM version .*")
LLVM_VERSION=${LLVM_VERSION##LLVM version }
NODE_BIN="$(which node 2>/dev/null)"
NPM_BIN="$(which npm 2>/dev/null)"
PY_BIN="$(which python3 2>/dev/null)"
GDB_BIN="$(which gdb 2>/dev/null)"

env -u CARGO                                                                                                       \
BOOTSTRAP_CARGO="$RUST_DIR/build/$ARCH/stage0/bin/cargo"                                                           \
DOC_RUST_LANG_ORG_CHANNEL="https://doc.rust-lang.org/nightly"                                                      \
LD_LIBRARY_PATH="$RUST_DIR/build/$ARCH/stage0-bootstrap-tools/$ARCH/release/deps:$RUST_DIR/build/$ARCH/stage0/lib" \
RUSTC="$RUST_DIR/build/$ARCH/stage0/bin/rustc"                                                                     \
RUSTC_BOOTSTRAP="1"                                                                                                \
RUSTC_FORCE_RUSTC_VERSION="compiletest"                                                                            \
RUST_TEST_THREADS="$(nproc)"                                                                                       \
RUST_TEST_TMPDIR="$RUST_DIR/build/tmp"                                                                             \
"$RUST_DIR/build/$ARCH/stage0-tools-bin/compiletest"                                                               \
"--compile-lib-path"   "$RUST_DIR/build/$ARCH/stage1/lib"                                                          \
"--run-lib-path"       "$RUST_DIR/build/$ARCH/stage1/lib/rustlib/$ARCH/lib"                                        \
"--src-base"           "$RUST_DIR/tests/ui"                                                                        \
"--sysroot-base"       "$RUST_DIR/build/$ARCH/stage1"                                                              \
"--stage-id"           "stage1-$ARCH"                                                                              \
"--suite"              "ui"                                                                                        \
"--mode"               "ui"                                                                                        \
"--target"             "$ARCH"                                                                                     \
"--host"               "$ARCH"                                                                                     \
"--llvm-filecheck"     "$LLVM_FILECHECK"                                                                           \
"--nodejs"             "$NODE_BIN"                                                                                 \
"--npm"                "$NPM_BIN"                                                                                  \
"--optimize-tests"                                                                                                 \
"--host-rustcflags"    "-Crpath"                                                                                   \
"--host-rustcflags"    "-Cdebuginfo=0"                                                                             \
"--host-rustcflags"    "-Lnative=$RUST_DIR/build/$ARCH/native/rust-test-helpers"                                   \
"--target-rustcflags"  "-Crpath"                                                                                   \
"--target-rustcflags"  "-Cdebuginfo=0"                                                                             \
"--target-rustcflags"  "-Lnative=$RUST_DIR/build/$ARCH/native/rust-test-helpers"                                   \
"--python"             "$PY_BIN"                                                                                   \
"--gdb"                "$GDB_BIN"                                                                                  \
"--llvm-version"       "$LLVM_VERSION"                                                                             \
"--llvm-components"    "$LLVM_COMPONENTS"                                                                          \
"--cc"                 ""                                                                                          \
"--cxx"                ""                                                                                          \
"--cflags"             ""                                                                                          \
"--cxxflags"           ""                                                                                          \
"--adb-path"           "adb"                                                                                       \
"--adb-test-dir"       "/data/local/tmp/work"                                                                      \
"--android-cross-path" ""                                                                                          \
"--channel"            "dev"                                                                                       \
"--git-repository"     "rust-lang/rust"                                                                            \
"--nightly-branch"     "master"                                                                                    \
"--json"                                                                                                           \
"--verbose"                                                                                                        \
"--rustc-path"         "$RUST_BIN"                                                                                 \
"--build-base"         "$OUT_DIR"                                                                                  \
"$@"                                                                                                               \
$(cat "$SOURCE_FILE")
