// Copyright 2026 VKrishna04
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

/// Command handler modules.
///
/// Each subcommand has its own module with a `run()` function.
pub mod config;
pub mod daemon;
pub mod doctor;
pub mod hook;
pub mod icon;
pub mod init;
pub mod link;
pub mod restore;
pub mod run;
pub mod setup;
pub mod skill;
pub mod status;
pub mod undo;
pub mod uninstall;
pub mod update;
