# Apple `container` fixtures

`container-list.json` is the output shape of `container list -a --format json`
for Apple `container` 1.4.1. It is copied from the MaibornWolff sandbox
repository (`src/platform/container-runtime/apple/__fixtures__/1.4.1/`,
BSD-3-Clause) so our parser stays aligned with theirs. Replace with a real
capture from an Apple silicon Mac on macOS 26 when available.

`container-stats.json` is a real capture of
`container stats --format json --no-stream` from an Apple silicon Mac, taken
on 2026-11-16 and cut to two containers with the ids renamed. `cpuUtilization`
is a fraction of one core (1.0 is one core fully used), per
`Sources/ContainerCommands/Container/ContainerStats.swift` in apple/container.
