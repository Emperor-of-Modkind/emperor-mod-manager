package dev.emperormodmanager.mobile

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class InstallApkArgs {
    lateinit var path: String
}

/**
 * Tauri v2 Android plugin that checks REQUEST_INSTALL_PACKAGES permission and
 * launches the Android system Package Installer for a downloaded APK via FileProvider.
 */
@TauriPlugin
class AppUpdatePlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun installApk(invoke: Invoke) {
        val args = invoke.parseArgs(InstallApkArgs::class.java)
        val file = File(args.path)
        if (!file.exists() || !file.isFile) {
            invoke.reject("Downloaded APK not found at ${args.path}")
            return
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
            !activity.packageManager.canRequestPackageInstalls()
        ) {
            val settingsIntent = Intent(
                Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                Uri.parse("package:${activity.packageName}")
            ).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            activity.startActivity(settingsIntent)

            val res = JSObject()
            res.put("installed", false)
            res.put("needsPermission", true)
            res.put(
                "message",
                "Allow 'Install unknown apps' for Emperor Mod Manager in Android Settings, then tap Update again."
            )
            invoke.resolve(res)
            return
        }

        val authority = "${activity.packageName}.fileprovider"
        val apkUri = FileProvider.getUriForFile(activity, authority, file)
        val installIntent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(apkUri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        activity.startActivity(installIntent)

        val res = JSObject()
        res.put("installed", false)
        res.put("needsPermission", false)
        res.put("message", "Opening Android installer…")
        invoke.resolve(res)
    }
}
