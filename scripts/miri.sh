export LOG_FILE=/tmp/miri.log LOG_LEVEL=DEBUG
#MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-backtrace=full -Zmiri-deterministic-concurrency -Zmiri-tree-borrows -Zmiri-strict-provenance " cargo +nightly miri test -- --no-capture --test-threads=1
MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-disable-weak-memory-emulation  -Zmiri-backtrace=full" cargo +nightly miri test $@ -- --no-capture --test-threads=1
