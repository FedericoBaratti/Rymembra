package com.aether.player;

import android.graphics.Color;
import android.os.Bundle;
import android.system.Os;
import android.util.Log;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.WebView;

import androidx.core.graphics.Insets;
import androidx.core.view.ViewCompat;
import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import com.getcapacitor.Bridge;
import com.getcapacitor.BridgeActivity;
import com.getcapacitor.BridgeWebViewClient;

public class MainActivity extends BridgeActivity {
    // nodejs-mobile is provided by the nodejs-mobile-cordova plugin (auto-wired
    // by `cap sync`); the renderer bridge uses its window.nodejs channel.
    //
    // The custom NodeBackend plugin (NodeBackendPlugin/NodeRuntime, raw AAR
    // route) is an ALTERNATIVE and is intentionally NOT registered here — doing
    // so would load libnode a second time. To switch to that route, drop
    // nodejs-mobile-cordova, add the nodejs-mobile AAR + JNI glue (see MOBILE.md),
    // and register the plugin before super.onCreate().

    // Last system-bar insets in dp, fed to CSS as --sa-* (see pushInsets / global.css).
    private int saTop, saRight, saBottom, saLeft;
    // Last soft-keyboard (IME) inset in dp, fed to CSS as --kb-height. With
    // setDecorFitsSystemWindows(false) the framework no longer pads the window for
    // the keyboard (API 35+ edge-to-edge), so the renderer must lift dialogs itself.
    private int kbHeight;
    private boolean haveInsets = false;

    @Override
    public void onCreate(Bundle savedInstanceState) {
        // Expose the app's native library dir (where jniLibs .so executables such
        // as libfpcalc.so are extracted) to the Node backend. Same mechanism the
        // nodejs-mobile-cordova plugin uses for TMPDIR: node runs in this process,
        // so Os.setenv lands in its process.env before the engine ever starts
        // (node only boots when the renderer calls nodejs.start, long after
        // onCreate — and this is idempotent across Activity recreates).
        try {
            Os.setenv("AETHER_NATIVE_LIB_DIR", getApplicationInfo().nativeLibraryDir, true);
        } catch (Exception ignored) {
            // Best-effort: without it, binaries.ts reports fpcalc/binaries absent.
        }

        // App-embedded Capacitor plugins (reached from the backend via reverse-RPC
        // and from the renderer for media controls). None loads libnode, so unlike
        // NodeBackendPlugin they are safe to register here.
        registerPlugin(FileAccessPlugin.class);
        registerPlugin(ImageResizePlugin.class);
        registerPlugin(AudioDecodePlugin.class);
        registerPlugin(SecureStorePlugin.class);
        registerPlugin(MediaSessionPlugin.class);
        registerPlugin(NativeAudioPlugin.class);
        registerPlugin(YtDlpPlugin.class);
        registerPlugin(MigrationServicePlugin.class);
        registerPlugin(DownloadNotificationPlugin.class);
        registerPlugin(TransferServicePlugin.class);
        registerPlugin(LanDiscoveryPlugin.class);
        registerPlugin(ThermalPlugin.class);
        super.onCreate(savedInstanceState);

        final WebView webView = this.getBridge().getWebView();

        // Belt-and-suspenders with capacitor.config backgroundColor: a fresh
        // WebView (launch or recreate after renderer death) must never show its
        // default white before the page's CSS paints.
        webView.setBackgroundColor(Color.parseColor("#09090D"));

        // Survive a renderer death instead of letting Android tear down the whole
        // app. Wrapping Capacitor's own BridgeWebViewClient keeps the bridge intact
        // while adding onRenderProcessGone; detail.didCrash() logs OOM (false) vs an
        // internal renderer crash (true) under the AETHER-WEBVIEW tag for diagnosis.
        webView.setWebViewClient(new RecoveringWebViewClient(this.getBridge(), this));

        // Allow auto-advance (gapless onEnd→next) and lockscreen resume to start
        // playback without a fresh user gesture. getBridge() is only valid after
        // super.onCreate().
        webView.getSettings().setMediaPlaybackRequiresUserGesture(false);

        // ── Edge-to-edge (Android 16 / API 36 enforces it; opt-out is gone) ──
        // The WebView now draws behind the status & navigation bars. We hand the
        // system-bar insets to CSS as --sa-* custom properties so the renderer's
        // safe-area layout works independently of the WebView/Chromium version
        // (env(safe-area-inset-*) is unreliable below Chromium 140). The dark theme
        // needs light (non-dark) bar icons to stay legible over transparent bars.
        WindowCompat.setDecorFitsSystemWindows(getWindow(), false);
        getWindow().setStatusBarColor(Color.TRANSPARENT);
        getWindow().setNavigationBarColor(Color.TRANSPARENT);

        WindowInsetsControllerCompat controller =
                WindowCompat.getInsetsController(getWindow(), webView);
        if (controller != null) {
            controller.setAppearanceLightStatusBars(false);
            controller.setAppearanceLightNavigationBars(false);
        }

        final float density = getResources().getDisplayMetrics().density;
        ViewCompat.setOnApplyWindowInsetsListener(webView, (v, insets) -> {
            Insets bars = insets.getInsets(WindowInsetsCompat.Type.systemBars());
            saTop = Math.round(bars.top / density);
            saRight = Math.round(bars.right / density);
            saBottom = Math.round(bars.bottom / density);
            saLeft = Math.round(bars.left / density);
            // IME inset: 0 when the keyboard is hidden, keyboard height (incl. the nav
            // bar it covers) when shown. This listener fires on every IME show/hide.
            Insets ime = insets.getInsets(WindowInsetsCompat.Type.ime());
            kbHeight = Math.round(ime.bottom / density);
            haveInsets = true;
            pushInsets(webView);
            // Don't consume: let the WebView keep handling insets too.
            return insets;
        });

        // Safety net: the first inset dispatch can land before the HTML document
        // exists. Re-push once the page has had time to parse (cheap no-ops if the
        // listener already ran against a live document).
        webView.postDelayed(() -> pushInsets(webView), 500);
        webView.postDelayed(() -> pushInsets(webView), 2000);

        // Android Auto cold-start: the car asked to play while the app was dead, so
        // the MediaBrowserService launched us with the mediaId. Dispatch it once the
        // NativeAudio plugin has loaded (AetherAuto.playWhenReady polls, no relaunch).
        handleAutoPlayIntent(getIntent());
    }

    /**
     * singleTask: a second Auto play request while the app is already running
     * arrives here instead of a fresh onCreate.
     */
    @Override
    protected void onNewIntent(android.content.Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleAutoPlayIntent(intent);
    }

    /** Play the catalog mediaId stashed by AetherAuto on a cold-start launch. */
    private void handleAutoPlayIntent(android.content.Intent intent) {
        if (intent == null) return;
        String mediaId = intent.getStringExtra(AetherAuto.EXTRA_PLAY_MEDIA_ID);
        if (mediaId != null && !mediaId.isEmpty()) {
            AetherAuto.playWhenReady(mediaId);
        }
    }

    /**
     * Persist backend state when the app leaves the foreground. The nodejs-mobile
     * process is killed without warning when Android reclaims a backgrounded app,
     * so we ask the backend (via the existing renderer bridge) to flush its
     * debounced DB/settings/queue writes now. Best-effort and idempotent: a
     * no-op if nothing is dirty. Reuses the WebView→renderer→node path rather than
     * adding a Java→node channel. onPause (not onStop) gives the flush more lead
     * time before a potential kill.
     */
    @Override
    public void onPause() {
        super.onPause();
        try {
            final WebView webView = this.getBridge().getWebView();
            if (webView != null) {
                webView.evaluateJavascript(
                        "window.aether&&window.aether.flushNow&&window.aether.flushNow()", null);
            }
        } catch (Exception ignored) {
            // Bridge/WebView not available yet — nothing to flush.
        }
    }

    /** Writes the cached system-bar insets onto <html> as --sa-* CSS variables. */
    private void pushInsets(WebView webView) {
        if (!haveInsets) return;
        String js = "(function(){var s=document.documentElement&&document.documentElement.style;"
                + "if(!s)return;"
                + "s.setProperty('--sa-top','" + saTop + "px');"
                + "s.setProperty('--sa-right','" + saRight + "px');"
                + "s.setProperty('--sa-bottom','" + saBottom + "px');"
                + "s.setProperty('--sa-left','" + saLeft + "px');"
                + "s.setProperty('--kb-height','" + kbHeight + "px');})();";
        webView.evaluateJavascript(js, null);
    }

    /**
     * Capacitor's BridgeWebViewClient plus a renderer-death handler. By default
     * Android kills the entire app when a WebView's render process dies (OOM or
     * internal crash); returning true here keeps the app alive. A dead WebView is
     * unusable, so we recreate the Activity (rebuilding a fresh WebView/bridge)
     * rather than trying to revive the corpse.
     */
    private static class RecoveringWebViewClient extends BridgeWebViewClient {
        private final MainActivity activity;

        RecoveringWebViewClient(Bridge bridge, MainActivity activity) {
            super(bridge);
            this.activity = activity;
        }

        @Override
        public boolean onRenderProcessGone(WebView view, RenderProcessGoneDetail detail) {
            boolean didCrash = detail != null && detail.didCrash();
            Log.e("AETHER-WEBVIEW",
                    "Render process gone — didCrash=" + didCrash
                            + (didCrash ? " (internal renderer crash)" : " (OOM kill by system)"));
            // The dead WebView is unusable; rebuild the activity (fresh WebView +
            // bridge) on the UI thread instead of letting the app be killed.
            if (activity != null) activity.runOnUiThread(activity::recreate);
            return true; // handled → app is NOT killed
        }
    }
}
