package com.aether.player

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

/**
 * Foreground service that keeps the process alive during downloads (YouTube and
 * single Spotify tracks) AND shows an aggregate progress notification with a live
 * percentage bar.
 *
 * Two update paths:
 *   - start/stop come from the node backend via DownloadNotificationPlugin
 *     (reverse-RPC) using startForegroundService Intents — only valid while the
 *     app is foreground (where downloads are started).
 *   - per-track counts and the LIVE intra-track percent update the already-running
 *     notification directly via NotificationManagerCompat.notify(), which is
 *     allowed in the background. The live percent is pushed straight from
 *     YtDlpPlugin's youtubedl-android progress callback (no WebView round-trip),
 *     so the bar moves even while the app is backgrounded / the WebView frozen.
 *
 * Uses the dataSync foreground type (FOREGROUND_SERVICE_DATA_SYNC on Android 14+;
 * note the 6h runtime cap on Android 15+, acceptable for downloads).
 */
class DownloadForegroundService : Service() {

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.getStringExtra(EXTRA_ACTION)
        if (action == ACTION_STOP) {
            running = false
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return START_NOT_STICKY
        }
        // START (or an Intent-borne update): adopt the provided fields.
        intent?.getStringExtra(EXTRA_TITLE)?.let { title = it }
        if (intent?.hasExtra(EXTRA_ACTIVE) == true) active = intent.getIntExtra(EXTRA_ACTIVE, active)
        if (intent?.hasExtra(EXTRA_TOTAL) == true) total = intent.getIntExtra(EXTRA_TOTAL, total)
        if (intent?.hasExtra(EXTRA_PERCENT) == true) percent = intent.getIntExtra(EXTRA_PERCENT, percent)
        running = true
        val notification = build(this)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
        return START_NOT_STICKY
    }

    companion object {
        private const val CHANNEL_ID = "aether_downloads"
        private const val NOTIFICATION_ID = 1003
        const val EXTRA_ACTION = "action"
        const val EXTRA_TITLE = "title"
        const val EXTRA_ACTIVE = "active"
        const val EXTRA_TOTAL = "total"
        const val EXTRA_PERCENT = "percent"
        const val ACTION_STOP = "stop"

        @Volatile
        private var running = false
        @Volatile
        private var title = ""
        @Volatile
        private var active = 0
        @Volatile
        private var total = 0
        @Volatile
        private var percent = 0

        /** Backend-driven per-track update (title + N/M + percent). Updates the
         *  already-running notification directly so it works in the background. */
        fun applyProgress(context: Context, title: String, active: Int, total: Int, percent: Int) {
            this.title = title
            this.active = active
            this.total = total
            this.percent = percent.coerceIn(0, 100)
            renotify(context)
        }

        /** Live intra-track percent straight from yt-dlp's progress callback. */
        fun applyLivePercent(context: Context, percent: Int) {
            this.percent = percent.coerceIn(0, 100)
            renotify(context)
        }

        private fun renotify(context: Context) {
            if (!running) return
            try {
                NotificationManagerCompat.from(context).notify(NOTIFICATION_ID, build(context))
            } catch (_: Exception) {
                // Missing POST_NOTIFICATIONS or transient manager issue: never crash
                // a download over a missed notification update.
            }
        }

        private fun build(context: Context): Notification {
            ensureChannel(context)
            val contentIntent = context.packageManager
                .getLaunchIntentForPackage(context.packageName)?.let {
                    PendingIntent.getActivity(
                        context,
                        0,
                        it,
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
                    )
                }
            val builder = NotificationCompat.Builder(context, CHANNEL_ID)
                .setSmallIcon(R.mipmap.ic_launcher)
                .setContentTitle(if (active > 1) "Download ($active)" else "Download")
                .setContentIntent(contentIntent)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            builder.setContentText(if (title.isEmpty()) "$percent%" else "$title — $percent%")
            builder.setProgress(100, percent.coerceIn(0, 100), false)
            return builder.build()
        }

        private fun ensureChannel(context: Context) {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
            val mgr = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            if (mgr.getNotificationChannel(CHANNEL_ID) == null) {
                mgr.createNotificationChannel(
                    NotificationChannel(
                        CHANNEL_ID,
                        "Download",
                        NotificationManager.IMPORTANCE_LOW
                    ).apply { setShowBadge(false) }
                )
            }
        }
    }
}
