#!/bin/bash
export CARGO_TARGET_DIR="target/smokes"

run () {
	RUN_COMMAND=$@
	echo -ne "\033[1;33mTEST\033[0m $RUN_COMMAND"
	ERRORS=$($RUN_COMMAND 2>&1>/tmp/uwu_smoketest.out)
	RESULT=$?
	echo
	if test $RESULT -ne 0; then
		cat /tmp/uwu_smoketest.out
		echo -e "$ERRORS"
		echo -e "\033[1;5;41;37mFAIL\033[0m exit ($RESULT): $RUN_COMMAND"
		exit $RESULT
	else
		echo -ne "\033[1F"
		echo -e "\033[1;32mPASS\033[0m $RUN_COMMAND"
	fi
}

conduwuit () {
	UWU_OPTS=$@
	rm -rf /tmp/uwu_smoketest.db
	echo -e "[global]\nserver_name = \"localhost\"\ndatabase_path = \"/tmp/uwu_smoketest.db\"" > /tmp/uwu_smoketest.toml
	cargo $UWU_OPTS -- -c /tmp/uwu_smoketest.toml &
	sleep 15s
	kill -QUIT %1
	wait %1
	return $?
}

element () {
	TOOLCHAIN=$1; shift
	ELEMENT_OPTS=$@
	run cargo "$TOOLCHAIN" check $ELEMENT_OPTS --all-targets
	run cargo "$TOOLCHAIN" clippy $ELEMENT_OPTS --all-targets -- -D warnings
	if [ "$BUILD" != "check" ]; then
		run cargo "$TOOLCHAIN" build $ELEMENT_OPTS --all-targets
		run cargo "$TOOLCHAIN" test $ELEMENT_OPTS --all-targets
		run cargo "$TOOLCHAIN" bench $ELEMENT_OPTS --all-targets
		run cargo "$TOOLCHAIN" run $ELEMENT_OPTS --bin conduwuit -- -V
		run conduwuit "$TOOLCHAIN" run $ELEMENT_OPTS --bin conduwuit
	fi
}

vector () {
	TOOLCHAIN=$1; shift
	VECTOR_OPTS=$@
	element "$TOOLCHAIN" $VECTOR_OPTS --no-default-features
	element "$TOOLCHAIN" $VECTOR_OPTS --features=default
	element "$TOOLCHAIN" $VECTOR_OPTS --features full
}

matrix () {
	run cargo +nightly fmt --all --check
	vector "" --profile=dev
	vector "" --profile=release
	vector +nightly --profile=dev
	vector +nightly --profile=release
}

BUILD=${1:-build}
matrix && exit 0
