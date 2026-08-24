// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// The `devp` entry point. Identical to `src/main.rs` and deliberately a separate file:
// pointing two `[[bin]]` targets at one source makes cargo warn on every single build.

fn main() {
    dev_prune::run_cli();
}
