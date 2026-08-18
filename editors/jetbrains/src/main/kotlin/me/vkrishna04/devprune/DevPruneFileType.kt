// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

package me.vkrishna04.devprune

import com.intellij.json.JsonLanguage
import com.intellij.openapi.fileTypes.LanguageFileType
import com.intellij.openapi.util.IconLoader
import javax.swing.Icon

/**
 * A named file type for `.devprune.json` whose only job is the icon.
 *
 * It deliberately extends [LanguageFileType] over [JsonLanguage]: the file stays a JSON
 * file to every other subsystem, so schema-driven completion (from the `$schema` key the
 * CLI writes, or from SchemaStore), folding and formatting all keep working. An
 * exact-filename match outranks the `*.json` extension match, which is how this type
 * wins the file without claiming any other JSON file.
 */
class DevPruneFileType : LanguageFileType(JsonLanguage.INSTANCE) {
    override fun getName(): String = "dev-prune configuration"

    override fun getDescription(): String =
        "Per-repository configuration for dev-prune, the lockfile-verified workspace cleaner"

    override fun getDefaultExtension(): String = "devprune.json"

    override fun getIcon(): Icon = ICON

    companion object {
        @JvmField
        val INSTANCE = DevPruneFileType()

        private val ICON = IconLoader.getIcon("/icons/devprune.png", DevPruneFileType::class.java)
    }
}
