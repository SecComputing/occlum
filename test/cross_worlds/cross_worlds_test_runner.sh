#! /bin/bash

set -e

if [[ ( "$#" != 1 ) ]] ; then
    echo "Usage: ./cross_worlds_test_runner.sh libos or host"
fi

run_env=$1

if [ "$run_env" = "libos" ]; then
    LD_LIBRARY_PATH=./image/lib ./image/bin/cross_worlds_server & 
    ../bin/occlum exec /bin/cross_worlds_client
    wait
    echo "  Libos client and host server - [OK]"

    ../bin/occlum exec /bin/cross_worlds_server &
    sleep 1
    LD_LIBRARY_PATH=./image/lib ./image/bin/cross_worlds_client 
    wait
    echo "  Host client and libos server - [OK]"
elif [ "$run_env" = "host" ]; then
    LD_LIBRARY_PATH=./image/lib 
    ./image/bin/cross_worlds_server &
    ./image/bin/cross_worlds_client
else
    echo "Error: only libos and host is supported"
fi
