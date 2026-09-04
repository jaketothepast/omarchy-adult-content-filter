#!/bin/bash

adult_filter_cmdline_matches() {
  local cmdline=$1 expected=$2 match=$3
  local argument first_argument=""
  local argument_count=0
  local -a rewritten_arguments=()

  [[ -r $cmdline && -n $expected && $expected != *[[:space:]]* ]] || return 1

  while IFS= read -r -d '' argument; do
    ((argument_count += 1))
    if ((argument_count == 1)); then
      first_argument=$argument
    fi
    if [[ $match == exact && $argument == "$expected" ]] ||
      [[ $match == prefix && $argument == "$expected"* ]]; then
      return 0
    fi
  done <"$cmdline"

  # Chromium rewrites the browser process's argv into one space-delimited
  # process title after startup. Only apply that fallback to a genuine
  # single-argument cmdline; ordinary argv still requires exact tokens.
  ((argument_count == 1)) || return 1
  read -r -a rewritten_arguments <<<"$first_argument"
  for argument in "${rewritten_arguments[@]}"; do
    if [[ $match == exact && $argument == "$expected" ]] ||
      [[ $match == prefix && $argument == "$expected"* ]]; then
      return 0
    fi
  done
  return 1
}

adult_filter_cmdline_has_exact_argument() {
  adult_filter_cmdline_matches "$1" "$2" exact
}

adult_filter_cmdline_has_argument_prefix() {
  adult_filter_cmdline_matches "$1" "$2" prefix
}

adult_filter_new_chromium_client() {
  local baseline=$1 current=$2

  jq -r --argjson baseline "$baseline" '
    [
      .[]
      | select((.class // "") | test("(?i)chromium"))
      | . as $candidate
      | select($baseline | all(.[];
          .address != $candidate.address or .pid != $candidate.pid))
    ]
    | first // empty
    | if type == "object" then [.address, (.pid | tostring)] | @tsv else empty end
  ' <<<"$current"
}

adult_filter_close_window() {
  local address=${1:-}

  [[ $address =~ ^0x[[:xdigit:]]+$ ]] || return 2
  hyprctl dispatch "hl.dsp.window.close({ window = \"address:$address\" })" ||
    hyprctl dispatch closewindow "address:$address"
}
