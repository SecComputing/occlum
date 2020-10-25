#!/bin/bash
#*
#*
#* Copyright 2014 gRPC authors.
#*
#* Licensed under the Apache License, Version 2.0 (the "License");
#* you may not use this file except in compliance with the License.
#* You may obtain a copy of the License at
#*
#*     http://www.apache.org/licenses/LICENSE-2.0
#*
#* Unless required by applicable law or agreed to in writing, software
#* distributed under the License is distributed on an "AS IS" BASIS,
#* WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#* See the License for the specific language governing permissions and
#* limitations under the License.
#*

set -e

rpcs=(1)
conns=(1)
warmup=10
dur=10
reqs=(1)
resps=(1)
rpc_types=(unary)

out_dir=$PWD/bin
# idx[0] = idx value for rpcs
# idx[1] = idx value for conns
# idx[2] = idx value for reqs
# idx[3] = idx value for resps
# idx[4] = idx value for rpc_types
idx=(0 0 0 0 0)
idx_max=(1 1 1 1 1)



# 1. Init Occlum Workspace
rm -rf occlum_server && mkdir occlum_server
rm -rf occlum_client && mkdir occlum_client
cd occlum_client
occlum init
new_json="$(jq '.resource_limits.user_space_size = "2048MB" |
	        .resource_limits.max_num_of_threads = 96 |
                .process.default_mmap_size = "300MB"' Occlum.json)" && \
echo "${new_json}" > Occlum.json

# 2. Copy program into Occlum Workspace and build
cp ${out_dir}/client image/bin
mkdir -p image/etc
cp /etc/hosts image/etc
occlum build

cd ../occlum_server
occlum init
new_json="$(jq '.resource_limits.user_space_size = "2048MB" |
	        .resource_limits.max_num_of_threads = 96 |
                .process.default_mmap_size = "300MB"' Occlum.json)" && \
echo "${new_json}" > Occlum.json

# 2. Copy program into Occlum Workspace and build
cp ${out_dir}/server image/bin
mkdir -p image/etc
cp /etc/hosts image/etc
occlum build
