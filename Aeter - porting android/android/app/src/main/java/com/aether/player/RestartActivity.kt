package com.aether.player

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Process

/**
 * Process-restart trampoline (ProcessPhoenix pattern).
 *
 * The nodejs-mobile engine keeps a process-wide "started" flag and CANNOT be
 * restarted in the same process (a second start aborts with
 * `Check failed: !platform_`). So when the node backend dies while the WebView
 * survives, the only real recovery is a full process restart — which the main
 * process cannot do by itself (killing yourself right after startActivity kills
 * the new activity too, since it lives in the same process).
 *
 * This activity runs in its own `:restart` process (see AndroidManifest.xml):
 * it kills the main process by pid, relaunches the launcher activity in a fresh
 * task, then exits its own process. Invoked from the renderer via
 * FileAccessPlugin.restartApp() when the user taps "Riavvia app" on the
 * backend-down banner (src/components/layout/BackendDownBanner.tsx).
 */
class RestartActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val mainPid = intent.getIntExtra(EXTRA_MAIN_PID, -1)
        if (mainPid > 0) Process.killProcess(mainPid)

        val launch = packageManager.getLaunchIntentForPackage(packageName)
        if (launch != null) {
            launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            startActivity(launch)
        }
        finish()
        // Exit the trampoline process too: the relaunched app must start from a
        // clean slate, not inherit this short-lived helper process.
        Runtime.getRuntime().exit(0)
    }

    companion object {
        const val EXTRA_MAIN_PID = "main_pid"
    }
}
