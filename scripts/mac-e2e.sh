#!/bin/bash
set -u

# End-to-end tests for the mounted macOS File Provider domain.
# The Filestash app must be running and connected before this script starts.

FILTER="${FILTER:-*}"
KEEP_GOING=false
REMOTE="${REMOTE:-/mac-e2e-$$}"
ROOT="${FDRIVE_MAC_ROOT:-$HOME/Library/CloudStorage/Filestash-Filestash}"
SESSION="$HOME/Library/Group Containers/group.app.filestash.sync/session.json"
TESTKIT="$HOME/Library/Developer/Xcode/DerivedData/Filestash-fdrive/Build/Products/Debug/FilestashTestKit.app/Contents/MacOS/FilestashTestKit"
APP="$HOME/Library/Developer/Xcode/DerivedData/Filestash-fdrive/Build/Products/Debug/Filestash.app"
PROJECT="$(cd "$(dirname "$0")/../crates/fdrive-mac/macos" && pwd)/Filestash.xcodeproj"
RESULTS=()
FAILURES=0
TESTS=()

while (($#)); do
    case "$1" in
        --filter) FILTER="$2"; shift 2 ;;
        --keep-going) KEEP_GOING=true; shift ;;
        --remote) REMOTE="$2"; shift 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

REMOTE="/${REMOTE#/}"
REMOTE="${REMOTE%/}"
LOCAL="$ROOT/${REMOTE#/}"

######## helpers ########

run_test() { TESTS+=("$1"); }

execute_test() {
    local fn="$1" name="${1//_/-}"
    [[ "$name" == $FILTER ]] || return 0
    echo "== $name"
    if "$fn"; then
        echo "  PASS"
        RESULTS+=("PASS  $name")
        return 0
    fi
    echo "  FAIL"
    RESULTS+=("FAIL  $name")
    ((FAILURES++))
    $KEEP_GOING || exit 1
}

cleanup() {
    local status=$?
    ((FAILURES == 0)) || status=1
    osascript -e "tell application \"Finder\" to close (every Finder window whose name is \"$(basename "$REMOTE")\")" >/dev/null 2>&1 || true
    [[ -e "$LOCAL" ]] && rm -rf "$LOCAL"
    srv_rm "$REMOTE/"
    local register="/System/Library/Frameworks/CoreServices.framework/Versions/Current/Frameworks/LaunchServices.framework/Versions/Current/Support/lsregister"
    "$register" -u "${TESTKIT%/Contents/MacOS/FilestashTestKit}" 2>/dev/null || true
    "$register" -f -R -trusted "$APP" 2>/dev/null || true
    if ((${#RESULTS[@]})); then
        echo
        echo "======== summary ========"
        printf '%s\n' "${RESULTS[@]}"
        echo "${#RESULTS[@]} tests, $FAILURES failed"
    fi
    exit $status
}

encode() { jq -nr --arg value "$1" '$value|@uri'; }

srv_call() {
    local method="$1" endpoint="$2" path="$3" body="${4:-}"
    local url="$BASE$endpoint?path=$(encode "$path")"
    local args=(-fsS -X "$method" -H "X-Requested-With: SDKHttpRequest" -H "Authorization: Bearer $TOKEN")
    [[ -n "$body" ]] && args+=(--data-binary "@$body")
    curl "${args[@]}" "$url"
}

srv_save() {
    local path="$1" content="$2" tmp
    tmp="$(mktemp)"
    printf '%s' "$content" >"$tmp"
    srv_save_file "$path" "$tmp"
    rm -f "$tmp"
}

srv_save_file() {
    local response
    response="$(srv_call POST /api/files/cat "$1" "$2")"
    jq -e '.status == "ok"' >/dev/null <<<"$response"
}

srv_cat() { srv_call GET /api/files/cat "$1"; }

srv_mkdir() {
    local response
    response="$(srv_call POST /api/files/mkdir "$1")"
    jq -e '.status == "ok"' >/dev/null <<<"$response"
}

srv_rm() { srv_call POST /api/files/rm "$1" >/dev/null 2>&1 || true; }

srv_has() {
    local directory="$1" name="$2" response
    response="$(srv_call GET /api/files/ls "$directory" 2>/dev/null)" || return 1
    jq -e --arg name "$name" '.results // [] | any(.name == $name)' >/dev/null <<<"$response"
}

not_srv_has() { ! srv_has "$1" "$2"; }

srv_name_normalized() {
    local directory="$1" wanted="$2" response name normalized
    response="$(srv_call GET /api/files/ls "$directory" 2>/dev/null)" || return 1
    wanted="$(printf '%s' "$wanted" | iconv -f UTF-8-MAC -t UTF-8)"
    while IFS= read -r name; do
        normalized="$(printf '%s' "$name" | iconv -f UTF-8-MAC -t UTF-8)"
        [[ "$normalized" == "$wanted" ]] && { printf '%s' "$name"; return 0; }
    done < <(jq -r '.results[]?.name' <<<"$response")
    return 1
}
srv_has_normalized() { srv_name_normalized "$1" "$2" >/dev/null; }

wait_until() {
    local what="$1" timeout="$2"
    shift 2
    local end=$((SECONDS + timeout))
    while ((SECONDS < end)); do
        "$@" && return 0
        sleep 0.5
    done
    echo "    timed out waiting for: $what" >&2
    return 1
}

exists() { [[ -e "$1" ]]; }
missing() { [[ ! -e "$1" ]]; }
file_size_is() { [[ "$(stat -f %z "$1" 2>/dev/null)" == "$2" ]]; }
file_content_is() { [[ "$(cat "$1" 2>/dev/null)" == "$2" ]]; }
server_content_is() { [[ "$(srv_cat "$1" 2>/dev/null)" == "$2" ]]; }
files_match() { cmp -s "$1" "$2"; }
managed_item() { "$TESTKIT" is-managed "$1"; }

activity_has() {
    "$TESTKIT" activity | jq -e \
        --arg path "$REMOTE/$1" --arg direction "$2" --arg state "$3" \
        'any(.transfers[]; .path == $path and .direction == $direction and .state == $state)' >/dev/null
}

activity_field() {
    "$TESTKIT" activity | jq -r --arg path "$REMOTE/$1" --arg field "$2" \
        'first(.transfers[] | select(.path == $path)) | .[$field]'
}

activity_meter_total() { "$TESTKIT" activity | jq '[.meter[] | .up + .down] | add'; }

activity_meter_grew_by() { (($(activity_meter_total) >= $1 + $2)); }
remove_empty_dir() { rmdir "$1" 2>/dev/null || [[ ! -e "$1" ]]; }

file_count_is() {
    [[ "$(find "$1" -mindepth 1 -maxdepth 1 -type f 2>/dev/null | wc -l | tr -d ' ')" == "$2" ]]
}

server_file_matches() {
    local remote="$1" expected="$2" actual
    actual="$(mktemp)"
    srv_cat "$remote" >"$actual" 2>/dev/null || { find "$actual" -delete; return 1; }
    cmp -s "$expected" "$actual"
    local result=$?
    find "$actual" -delete
    return $result
}

trash_item() {
    osascript - "$1" >/dev/null <<'APPLESCRIPT' &
on run argv
    set theItem to POSIX file (item 1 of argv) as alias
    tell application "Finder" to delete theItem
end run
APPLESCRIPT
    local pid=$! i
    for i in $(seq 1 60); do
        kill -0 "$pid" 2>/dev/null || { wait "$pid"; return $?; }
        sleep 0.5
    done
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    return 1
}

######## tests ########

remote_create_file() {
    srv_save "$REMOTE/r1.txt" "remote one" &&
        wait_until "r1.txt locally" 25 exists "$LOCAL/r1.txt"
}
run_test remote_create_file

hydrate_on_read() { file_content_is "$LOCAL/r1.txt" "remote one"; }
run_test hydrate_on_read

remote_modify_updates_placeholder() {
    srv_save "$REMOTE/r1.txt" "remote one but longer now" &&
        wait_until "new remote version" 25 "$TESTKIT" is-outdated "$LOCAL/r1.txt" &&
        "$TESTKIT" download "$LOCAL/r1.txt" &&
        wait_until "size becomes 25" 25 file_size_is "$LOCAL/r1.txt" 25 &&
        file_content_is "$LOCAL/r1.txt" "remote one but longer now"
}
run_test remote_modify_updates_placeholder

remote_modify_dehydrated_placeholder() {
    srv_save "$REMOTE/rd.txt" v1 &&
        wait_until "rd.txt locally" 25 exists "$LOCAL/rd.txt" &&
        srv_save "$REMOTE/rd.txt" "v2 much longer content" &&
        wait_until "rd.txt size updates" 25 file_size_is "$LOCAL/rd.txt" 22 &&
        file_content_is "$LOCAL/rd.txt" "v2 much longer content"
}
run_test remote_modify_dehydrated_placeholder

remote_delete_file() {
    srv_rm "$REMOTE/r1.txt"
    wait_until "r1.txt vanishes" 25 missing "$LOCAL/r1.txt"
}
run_test remote_delete_file

remote_tree_populates_on_first_enumeration() {
    srv_mkdir "$REMOTE/rtree/" && srv_mkdir "$REMOTE/rtree/deep/" &&
        srv_save "$REMOTE/rtree/a.txt" aaa && srv_save "$REMOTE/rtree/deep/b.txt" bbb &&
        wait_until "remote tree locally" 25 exists "$LOCAL/rtree/deep/b.txt" &&
        file_content_is "$LOCAL/rtree/deep/b.txt" bbb
}
run_test remote_tree_populates_on_first_enumeration

remote_delete_clean_dir() {
    mkdir "$LOCAL/rtree/empty-local-dir" &&
        wait_until "empty directory on server" 30 srv_has "$REMOTE/rtree/" empty-local-dir &&
        "$TESTKIT" stabilize &&
        srv_rm "$REMOTE/rtree/"
    wait_until "remote tree vanishes" 30 missing "$LOCAL/rtree"
}
run_test remote_delete_clean_dir

local_create_uploads() {
    printf 'local one' >"$LOCAL/l1.txt"
    wait_until "l1.txt on server" 30 srv_has "$REMOTE/" l1.txt
}
run_test local_create_uploads

local_create_becomes_file_provider_item() {
    wait_until "l1 becomes a File Provider item" 15 managed_item "$LOCAL/l1.txt"
}
run_test local_create_becomes_file_provider_item

local_edit_uploads() {
    printf 'local one edited' >"$LOCAL/l1.txt"
    wait_until "edited content on server" 30 server_content_is "$REMOTE/l1.txt" "local one edited"
}
run_test local_edit_uploads

local_rename_propagates() {
    mv "$LOCAL/l1.txt" "$LOCAL/l1-renamed.txt" &&
        wait_until "renamed file on server" 30 srv_has "$REMOTE/" l1-renamed.txt &&
        ! srv_has "$REMOTE/" l1.txt
}
run_test local_rename_propagates

local_mkdir_and_move_in() {
    mkdir "$LOCAL/lsub" && wait_until "lsub on server" 30 srv_has "$REMOTE/" lsub &&
        mv "$LOCAL/l1-renamed.txt" "$LOCAL/lsub/l1-renamed.txt" &&
        wait_until "file moved on server" 30 srv_has "$REMOTE/lsub/" l1-renamed.txt
}
run_test local_mkdir_and_move_in

local_delete_file_propagates() {
    rm "$LOCAL/lsub/l1-renamed.txt"
    wait_until "file deleted on server" 30 not_srv_has "$REMOTE/lsub/" l1-renamed.txt
}
run_test local_delete_file_propagates

local_delete_dir_propagates() {
    wait_until "local directory removal" 15 remove_empty_dir "$LOCAL/lsub" || return 1
    wait_until "directory deleted on server" 30 not_srv_has "$REMOTE/" lsub
}
run_test local_delete_dir_propagates

recycle_hydrated_placeholder() {
    srv_save "$REMOTE/rec1.txt" "to be recycled" &&
        wait_until "rec1 locally" 25 exists "$LOCAL/rec1.txt" &&
        cat "$LOCAL/rec1.txt" >/dev/null && trash_item "$LOCAL/rec1.txt" &&
        wait_until "rec1 deleted on server" 30 not_srv_has "$REMOTE/" rec1.txt
}
run_test recycle_hydrated_placeholder

recycle_dehydrated_placeholder() {
    srv_save "$REMOTE/rec2.txt" "never opened" &&
        wait_until "rec2 locally" 25 exists "$LOCAL/rec2.txt" &&
        trash_item "$LOCAL/rec2.txt" &&
        wait_until "rec2 deleted on server" 30 not_srv_has "$REMOTE/" rec2.txt
}
run_test recycle_dehydrated_placeholder

recycle_directory() {
    srv_mkdir "$REMOTE/recdir/" && srv_save "$REMOTE/recdir/x.txt" x &&
        wait_until "recdir locally" 25 exists "$LOCAL/recdir" &&
        wait_until "recdir child locally" 25 exists "$LOCAL/recdir/x.txt" &&
        trash_item "$LOCAL/recdir/x.txt" && trash_item "$LOCAL/recdir" &&
        wait_until "recdir deleted on server" 30 not_srv_has "$REMOTE/" recdir
}
run_test recycle_directory

download_now_hydrates() {
    srv_save "$REMOTE/download1.txt" "download me" &&
        wait_until "download1 locally" 25 exists "$LOCAL/download1.txt" &&
        "$TESTKIT" download "$LOCAL/download1.txt" &&
        wait_until "download1 downloaded" 30 "$TESTKIT" is-downloaded "$LOCAL/download1.txt" &&
        file_content_is "$LOCAL/download1.txt" "download me"
}
run_test download_now_hydrates

remove_download_evicts() {
    "$TESTKIT" evict "$LOCAL/download1.txt" &&
        wait_until "download1 evicted" 45 "$TESTKIT" is-evicted "$LOCAL/download1.txt" &&
        sleep 5 && "$TESTKIT" is-evicted "$LOCAL/download1.txt" &&
        srv_rm "$REMOTE/download1.txt"
}
run_test remove_download_evicts

downloaded_file_remains_materialized() {
    srv_save "$REMOTE/download-stay.txt" "downloaded v1" &&
        wait_until "download-stay locally" 25 exists "$LOCAL/download-stay.txt" &&
        "$TESTKIT" download "$LOCAL/download-stay.txt" &&
        wait_until "download-stay downloaded" 30 "$TESTKIT" is-downloaded "$LOCAL/download-stay.txt" &&
        sleep 25 && "$TESTKIT" is-downloaded "$LOCAL/download-stay.txt"
}
run_test downloaded_file_remains_materialized

downloaded_file_refreshes_remote_change() {
    srv_save "$REMOTE/download-stay.txt" "downloaded v2 now much longer" &&
        wait_until "download-stay remote version" 30 "$TESTKIT" is-outdated "$LOCAL/download-stay.txt" &&
        "$TESTKIT" download "$LOCAL/download-stay.txt" &&
        wait_until "new download-stay version" 45 file_content_is "$LOCAL/download-stay.txt" "downloaded v2 now much longer" &&
        "$TESTKIT" is-downloaded "$LOCAL/download-stay.txt"
}
run_test downloaded_file_refreshes_remote_change

downloaded_file_remote_delta() {
    # macOS owns the transfer strategy. This verifies that a small remote edit
    # inside a large downloaded file produces the correct final bytes.
    local source updated
    source="$(mktemp)"
    updated="$(mktemp)"
    dd if=/dev/urandom of="$source" bs=1m count=5 status=none
    cp "$source" "$updated"
    printf 'changed remote block' | dd of="$updated" bs=1 seek=$((2 * 1024 * 1024)) conv=notrunc status=none
    srv_save_file "$REMOTE/downloaded-delta.bin" "$source" &&
        wait_until "downloaded delta locally" 30 exists "$LOCAL/downloaded-delta.bin" &&
        "$TESTKIT" download "$LOCAL/downloaded-delta.bin" &&
        wait_until "downloaded delta downloaded" 45 "$TESTKIT" is-downloaded "$LOCAL/downloaded-delta.bin" &&
        srv_save_file "$REMOTE/downloaded-delta.bin" "$updated" &&
        wait_until "downloaded delta remote version" 30 "$TESTKIT" is-outdated "$LOCAL/downloaded-delta.bin" &&
        "$TESTKIT" download "$LOCAL/downloaded-delta.bin" &&
        wait_until "downloaded delta refreshed" 60 files_match "$updated" "$LOCAL/downloaded-delta.bin"
    local result=$?
    find "$source" "$updated" -delete
    return $result
}
run_test downloaded_file_remote_delta

conflict_keeps_both() {
    printf base >"$LOCAL/c1.txt" || return 1
    wait_until "c1 on server" 30 srv_has "$REMOTE/" c1.txt || return 1
    sleep 2
    srv_save "$REMOTE/c1.txt" "server version" || return 1
    printf 'local version' >"$LOCAL/c1.txt"
    local end=$((SECONDS + 35)) listing copy
    while ((SECONDS < end)); do
        listing="$(srv_call GET /api/files/ls "$REMOTE/" 2>/dev/null)" || true
        copy="$(jq -r '.results[]?.name | select(test("^c1 \\(conflicted copy from .+\\)\\.txt$"))' <<<"$listing" | head -1)"
        if [[ -n "$copy" ]]; then
            [[ "$(srv_cat "$REMOTE/$copy")" == "local version" && "$(srv_cat "$REMOTE/c1.txt")" == "server version" ]]
            return
        fi
        sleep 0.5
    done
    return 1
}
run_test conflict_keeps_both

dirty_file_survives_remote_dir_delete() {
    srv_mkdir "$REMOTE/ddir/" && srv_save "$REMOTE/ddir/keep.txt" server &&
        wait_until "keep.txt locally" 25 exists "$LOCAL/ddir/keep.txt" || return 1
    printf 'my precious edits' >"$LOCAL/ddir/keep.txt"
    srv_rm "$REMOTE/ddir/"
    wait_until "dirty edit restored remotely" 40 server_content_is "$REMOTE/ddir/keep.txt" "my precious edits"
}
run_test dirty_file_survives_remote_dir_delete

big_file_roundtrip() {
    local source
    source="$(mktemp)"
    dd if=/dev/urandom of="$source" bs=1m count=5 status=none
    srv_save_file "$REMOTE/big.bin" "$source" &&
        wait_until "big.bin locally" 30 exists "$LOCAL/big.bin" &&
        cmp "$source" "$LOCAL/big.bin"
    local result=$?
    rm -f "$source"
    return $result
}
run_test big_file_roundtrip

explorer_enumeration_is_fast() {
    srv_mkdir "$REMOTE/many/" || return 1
    local i
    for i in $(seq 0 39); do srv_save "$REMOTE/many/f$i.txt" "file $i" || return 1; done
    wait_until "all 40 files locally" 45 file_count_is "$LOCAL/many" 40
}
run_test explorer_enumeration_is_fast

remote_zero_byte_file() {
    srv_save "$REMOTE/empty-remote.txt" "" &&
        wait_until "empty remote file locally" 30 file_size_is "$LOCAL/empty-remote.txt" 0
}
run_test remote_zero_byte_file

remote_rapid_updates_converge() {
    srv_save "$REMOTE/rapid.txt" "one" &&
        wait_until "rapid file locally" 30 exists "$LOCAL/rapid.txt" &&
        file_content_is "$LOCAL/rapid.txt" one &&
        srv_save "$REMOTE/rapid.txt" "two" &&
        srv_save "$REMOTE/rapid.txt" "three is final" &&
        wait_until "rapid final version noticed" 30 "$TESTKIT" is-outdated "$LOCAL/rapid.txt" &&
        "$TESTKIT" download "$LOCAL/rapid.txt" &&
        wait_until "rapid final content" 30 file_content_is "$LOCAL/rapid.txt" "three is final"
}
run_test remote_rapid_updates_converge

remote_special_filename() {
    local name="résumé space #1.txt"
    srv_save "$REMOTE/$name" "special remote" &&
        wait_until "special remote filename locally" 30 exists "$LOCAL/$name" &&
        file_content_is "$LOCAL/$name" "special remote"
}
run_test remote_special_filename

remote_replaces_file_with_directory() {
    srv_save "$REMOTE/swap" file && wait_until "swap file locally" 30 test -f "$LOCAL/swap" &&
        srv_rm "$REMOTE/swap" && srv_mkdir "$REMOTE/swap/" &&
        wait_until "swap becomes directory" 40 test -d "$LOCAL/swap"
}
run_test remote_replaces_file_with_directory

remote_replaces_directory_with_file() {
    srv_rm "$REMOTE/swap/" && srv_save "$REMOTE/swap" file-again &&
        wait_until "swap becomes file" 40 test -f "$LOCAL/swap" &&
        file_content_is "$LOCAL/swap" file-again
}
run_test remote_replaces_directory_with_file

local_zero_byte_file() {
    : >"$LOCAL/empty-local.txt"
    wait_until "empty local file on server" 30 srv_has "$REMOTE/" empty-local.txt &&
        [[ -z "$(srv_cat "$REMOTE/empty-local.txt")" ]]
}
run_test local_zero_byte_file

local_special_filename() {
    local name="local ünicode #2.txt" remote_name
    printf 'special local' >"$LOCAL/$name"
    wait_until "special local filename on server" 30 srv_has_normalized "$REMOTE/" "$name" || return 1
    remote_name="$(srv_name_normalized "$REMOTE/" "$name")" &&
        server_content_is "$REMOTE/$remote_name" "special local"
}
run_test local_special_filename

local_nested_tree() {
    mkdir "$LOCAL/local-tree" &&
        wait_until "local tree root on server" 30 srv_has "$REMOTE/" local-tree &&
        mkdir "$LOCAL/local-tree/deep" &&
        wait_until "local tree child on server" 30 srv_has "$REMOTE/local-tree/" deep &&
        printf nested >"$LOCAL/local-tree/deep/value.txt" &&
        wait_until "nested file on server" 30 server_content_is "$REMOTE/local-tree/deep/value.txt" nested
}
run_test local_nested_tree

repeated_download_evict() {
    srv_save "$REMOTE/cycle.txt" cycle &&
        wait_until "cycle file locally" 30 exists "$LOCAL/cycle.txt" || return 1
    local i
    for i in 1 2 3; do
        "$TESTKIT" download "$LOCAL/cycle.txt" || return 1
        wait_until "cycle download $i" 30 "$TESTKIT" is-downloaded "$LOCAL/cycle.txt" || return 1
        "$TESTKIT" evict "$LOCAL/cycle.txt" || return 1
        wait_until "cycle eviction $i" 30 "$TESTKIT" is-evicted "$LOCAL/cycle.txt" || return 1
    done
}
run_test repeated_download_evict

remote_empty_directory() {
    srv_mkdir "$REMOTE/empty-remote-dir/" &&
        wait_until "empty remote directory locally" 30 test -d "$LOCAL/empty-remote-dir"
}
run_test remote_empty_directory

evicted_file_stays_evicted_on_remote_change() {
    srv_save "$REMOTE/evicted-update.txt" one &&
        wait_until "evicted update file locally" 30 exists "$LOCAL/evicted-update.txt" &&
        "$TESTKIT" download "$LOCAL/evicted-update.txt" &&
        wait_until "evicted update downloaded" 30 "$TESTKIT" is-downloaded "$LOCAL/evicted-update.txt" &&
        "$TESTKIT" evict "$LOCAL/evicted-update.txt" &&
        wait_until "evicted update evicted" 30 "$TESTKIT" is-evicted "$LOCAL/evicted-update.txt" &&
        srv_save "$REMOTE/evicted-update.txt" "two is newer" &&
        sleep 10 && "$TESTKIT" is-evicted "$LOCAL/evicted-update.txt" &&
        "$TESTKIT" download "$LOCAL/evicted-update.txt" &&
        wait_until "evicted update latest content" 30 file_content_is "$LOCAL/evicted-update.txt" "two is newer"
}
run_test evicted_file_stays_evicted_on_remote_change

local_truncate_and_grow() {
    printf 'a much longer initial value' >"$LOCAL/resize.txt" &&
        wait_until "initial resize upload" 30 server_content_is "$REMOTE/resize.txt" "a much longer initial value" &&
        printf x >"$LOCAL/resize.txt" &&
        wait_until "truncated resize upload" 30 server_content_is "$REMOTE/resize.txt" x &&
        printf 'grown again after truncate' >"$LOCAL/resize.txt" &&
        wait_until "grown resize upload" 30 server_content_is "$REMOTE/resize.txt" "grown again after truncate"
}
run_test local_truncate_and_grow

local_binary_roundtrip() {
    local source="$LOCAL/local-binary.bin"
    dd if=/dev/urandom of="$source" bs=1m count=1 status=none
    wait_until "binary file on server" 45 server_file_matches "$REMOTE/local-binary.bin" "$source"
}
run_test local_binary_roundtrip

local_batch_create() {
    local i
    for i in $(seq 1 20); do printf 'batch %s' "$i" >"$LOCAL/batch-$i.txt"; done
    for i in $(seq 1 20); do
        wait_until "batch file $i on server" 45 server_content_is "$REMOTE/batch-$i.txt" "batch $i" || return 1
    done
}
run_test local_batch_create

activity_service_answers() {
    "$TESTKIT" activity | jq -e '(.meter | length) == 120 and (.transfers | type) == "array"' >/dev/null
}
run_test activity_service_answers

activity_records_download() {
    srv_save "$REMOTE/act-down.txt" "activity download payload" &&
        wait_until "act-down.txt locally" 25 exists "$LOCAL/act-down.txt" &&
        "$TESTKIT" download "$LOCAL/act-down.txt" &&
        wait_until "download in activity" 25 activity_has act-down.txt down done &&
        [[ "$(activity_field act-down.txt size)" == 25 ]] &&
        [[ "$(activity_field act-down.txt wire)" == 25 ]]
}
run_test activity_records_download

activity_records_upload() {
    printf 'activity upload payload' >"$LOCAL/act-up.txt" &&
        wait_until "act-up.txt on server" 30 srv_has "$REMOTE/" act-up.txt &&
        wait_until "upload in activity" 25 activity_has act-up.txt up done &&
        [[ "$(activity_field act-up.txt size)" == 23 ]]
}
run_test activity_records_upload

activity_meter_counts_traffic() {
    local before
    before="$(activity_meter_total)"
    dd if=/dev/urandom of="$LOCAL/act-meter.bin" bs=1m count=2 status=none
    wait_until "act-meter.bin on server" 45 srv_has "$REMOTE/" act-meter.bin &&
        wait_until "meter counts the upload" 25 activity_meter_grew_by "$before" 2097152
}
run_test activity_meter_counts_traffic

######## run ########

[[ -f "$SESSION" ]] || { echo "missing connected session: $SESSION" >&2; exit 2; }
[[ -d "$ROOT" ]] || { echo "missing File Provider root: $ROOT" >&2; exit 2; }
xcodebuild -project "$PROJECT" -scheme FilestashTestKit -destination 'platform=macOS' \
    -derivedDataPath "$HOME/Library/Developer/Xcode/DerivedData/Filestash-fdrive" build >/dev/null || exit 2
"/System/Library/Frameworks/CoreServices.framework/Versions/Current/Frameworks/LaunchServices.framework/Versions/Current/Support/lsregister" \
    -f -R -trusted "${TESTKIT%/Contents/MacOS/FilestashTestKit}" 2>/dev/null || true
BASE="$(plutil -extract serverURL raw "$SESSION")"
BASE="${BASE%/}"
TOKEN="$(plutil -extract token raw "$SESSION")"

trap cleanup EXIT

srv_rm "$REMOTE/"
mkdir "$LOCAL"
wait_until "test directory on server" 30 srv_has "$(dirname "$REMOTE")/" "$(basename "$REMOTE")" || {
    echo "local test directory did not upload" >&2
    exit 1
}
open "$LOCAL"
wait_until "test window watched" 15 test -e "$LOCAL"

for fn in "${TESTS[@]}"; do execute_test "$fn"; done
