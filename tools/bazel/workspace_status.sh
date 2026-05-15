#!/usr/bin/env sh
set -eu

version="${RELEASE_VERSION:-}"
tag="${GITHUB_REF_NAME:-}"

if [ -z "${version}" ] && [ -z "${tag}" ]; then
    tag="$(git describe --tags --exact-match 2>/dev/null || true)"
fi

if [ -z "${version}" ]; then
    case "${tag}" in
        v[0-9]*.[0-9]*.[0-9]*)
            version="${tag#v}"
            ;;
        *)
            version="0.0.0-dev"
            ;;
    esac
fi

printf 'STABLE_BUILD_VERSION %s\n' "${version}"
printf 'BUILD_SCM_VERSION %s\n' "${tag:-dev}"
