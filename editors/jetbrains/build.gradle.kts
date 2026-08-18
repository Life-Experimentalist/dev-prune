// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Minimal file-type plugin: gives `.devprune.json` the dev-prune icon in the project
// tree while leaving the file's language as JSON, so every JSON feature the IDE has —
// completion from the `$schema` link, folding, formatting — keeps working untouched.
//
// Build: `./gradlew buildPlugin` (needs JDK 17+). The ZIP lands in
// build/distributions/ and uploads as-is to the JetBrains Marketplace.
plugins {
    id("org.jetbrains.kotlin.jvm") version "2.0.21"
    id("org.jetbrains.intellij.platform") version "2.2.1"
}

group = "me.vkrishna04"
version = "0.1.0"

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        // 2024.2 is the oldest line where the 2.x Gradle plugin is comfortable and the
        // JSON PSI is still part of the platform (2024.3 moved it into a bundled
        // plugin; `com.intellij.modules.json` below covers both arrangements).
        intellijIdeaCommunity("2024.2.4")
    }
}

intellijPlatform {
    pluginConfiguration {
        ideaVersion {
            sinceBuild = "242"
            // No untilBuild: the plugin touches nothing version-sensitive.
        }
    }
}

kotlin {
    jvmToolchain(17)
}
