package dev.emperormodmanager.mobile

import android.app.Activity
import android.webkit.WebView
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.lifecycleScope
import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowInfoTracker
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch

/**
 * Tauri v2 Android plugin exposing Jetpack WindowManager FoldingFeature state
 * and CSS-pixel hinge geometry to the WebView (`fold-state-changed` event).
 */
@TauriPlugin
class FoldablePlugin(private val activity: Activity) : Plugin(activity) {
    @Volatile
    private var lastState: JSObject = emptyFoldState()

    override fun load(webView: WebView) {
        super.load(webView)
        val owner = activity as? LifecycleOwner ?: return
        val tracker = WindowInfoTracker.getOrCreate(activity)
        owner.lifecycleScope.launch(Dispatchers.Main) {
            tracker.windowLayoutInfo(activity).collectLatest { info ->
                val feature = info.displayFeatures
                    .filterIsInstance<FoldingFeature>()
                    .firstOrNull()
                val payload = buildFoldPayload(feature)
                lastState = payload
                trigger("fold-state-changed", payload)
            }
        }
    }

    @Command
    fun getFoldState(invoke: Invoke) {
        invoke.resolve(lastState)
    }

    private fun buildFoldPayload(feature: FoldingFeature?): JSObject {
        if (feature == null) return emptyFoldState()
        val density = activity.resources.displayMetrics.density.coerceAtLeast(1f)
        val bounds = feature.bounds
        val obj = JSObject()
        obj.put("has_hinge", true)
        obj.put(
            "state",
            if (feature.state == FoldingFeature.State.HALF_OPENED) "half_opened" else "flat"
        )
        obj.put(
            "orientation",
            if (feature.orientation == FoldingFeature.Orientation.HORIZONTAL) "horizontal" else "vertical"
        )
        obj.put("is_separating", feature.isSeparating)
        obj.put("hinge_x", (bounds.left / density).toInt())
        obj.put("hinge_y", (bounds.top / density).toInt())
        obj.put("hinge_w", (bounds.width() / density).toInt())
        obj.put("hinge_h", (bounds.height() / density).toInt())
        return obj
    }

    private fun emptyFoldState(): JSObject {
        val obj = JSObject()
        obj.put("has_hinge", false)
        obj.put("state", "flat")
        obj.put("orientation", "none")
        obj.put("is_separating", false)
        obj.put("hinge_x", 0)
        obj.put("hinge_y", 0)
        obj.put("hinge_w", 0)
        obj.put("hinge_h", 0)
        return obj
    }
}
