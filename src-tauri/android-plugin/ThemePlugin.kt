package dev.emperormodmanager.mobile

import android.app.Activity
import android.content.res.Configuration
import android.os.Build
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/**
 * Tauri v2 Android plugin exposing Android 12+ (API 31+) Material You dynamic
 * color tokens mapped to Emperor Mod Manager's SystemPalette schema.
 */
@TauriPlugin
class ThemePlugin(private val activity: Activity) : Plugin(activity) {
    private fun colorHex(resId: Int): String {
        val color = ContextCompat.getColor(activity, resId) and 0x00FFFFFF
        return String.format("#%06x", color)
    }

    @Command
    fun getSystemPalette(invoke: Invoke) {
        val obj = JSObject()
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) {
            obj.put("available", false)
            invoke.resolve(obj)
            return
        }

        val nightMode =
            activity.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK
        val isDark = nightMode == Configuration.UI_MODE_NIGHT_YES

        obj.put("available", true)
        obj.put("dark", isDark)
        if (isDark) {
            obj.put("bg", colorHex(android.R.color.system_neutral1_900))
            obj.put("bg_2", colorHex(android.R.color.system_neutral1_800))
            obj.put("panel", colorHex(android.R.color.system_neutral2_800))
            obj.put("ink", colorHex(android.R.color.system_neutral1_50))
            obj.put("muted", colorHex(android.R.color.system_neutral2_300))
            obj.put("line", colorHex(android.R.color.system_neutral2_700))
            obj.put("accent", colorHex(android.R.color.system_accent1_200))
            obj.put("on_accent", colorHex(android.R.color.system_accent1_900))
            obj.put("input_bg", colorHex(android.R.color.system_neutral1_800))
        } else {
            obj.put("bg", colorHex(android.R.color.system_neutral1_50))
            obj.put("bg_2", colorHex(android.R.color.system_neutral1_100))
            obj.put("panel", colorHex(android.R.color.system_neutral1_0))
            obj.put("ink", colorHex(android.R.color.system_neutral1_900))
            obj.put("muted", colorHex(android.R.color.system_neutral2_600))
            obj.put("line", colorHex(android.R.color.system_neutral2_200))
            obj.put("accent", colorHex(android.R.color.system_accent1_600))
            obj.put("on_accent", colorHex(android.R.color.system_neutral1_0))
            obj.put("input_bg", colorHex(android.R.color.system_neutral1_0))
        }
        invoke.resolve(obj)
    }
}
