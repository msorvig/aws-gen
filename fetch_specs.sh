#!/usr/bin/env bash
# Downloads the three botocore service specs needed by the codegen.
# Run once before the first build; specs are checked into git or kept in specs/.
# Requires curl.
set -euo pipefail

BOTOCORE="https://raw.githubusercontent.com/boto/botocore/develop/botocore/data"
SPECS_DIR="codegen/specs"
mkdir -p "$SPECS_DIR"

fetch() {
    local service=$1 date=$2
    local out="$SPECS_DIR/${service}.json"
    if [[ -f "$out" ]]; then
        echo "  exists: $out (delete to re-fetch)"
    else
        echo "  fetching: $service $date ..."
        curl -sSfL "${BOTOCORE}/${service}/${date}/service-2.json" -o "$out"
        echo "  wrote: $out ($(wc -c < "$out") bytes)"
    fi
}

echo "Fetching botocore service specs..."
fetch ec2  "2016-11-15"
fetch s3   "2006-03-01"
fetch ssm  "2014-11-06"
fetch iam  "2010-05-08"
echo "Done. Run 'cargo build -p aws-api' to generate Rust types."
