package com.aether.player

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.IBinder
import android.support.v4.media.session.PlaybackStateCompat
import androidx.media.session.MediaButtonReceiver
import androidx.core.app.NotificationCompat
import androidx.media.app.NotificationCompat.MediaStyle

/**
 * Foreground service that publishes the media notification (with transport
 * controls) tied to the MediaSession owned by MediaSessionPlugin, so the
 * WebView audio keeps playing while the app is backgrounded.
 *
 * Requires FOREGROUND_SERVICE + FOREGROUND_SERVICE_MEDIA_PLAYBACK (already in
 * AndroidManifest.xml) and registration of this <service> in the manifest.
 */
class MediaPlaybackService : Service() {

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
    }

    override fun onDestroy() {
        if (instance === this) instance = null
        super.onDestroy()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startForeground(NOTIFICATION_ID, buildNotification())
        // NOT_STICKY: the service is explicitly (re)started on every play via
        // ensureForegroundService, so a sticky zombie restart after a process
        // kill would only produce a blank notification with a dead session.
        return START_NOT_STICKY
    }

    /**
     * Leave the foreground state after a long pause (see MediaSessionPlugin's
     * demote grace timer): the notification stays for a quick resume, but the
     * process becomes an ordinary cached app that Android may freeze — instead
     * of being pinned in memory forever with WebView + node + ExoPlayer just
     * because playback is paused. Resume re-promotes via publishState(playing).
     */
    private fun demoteNow() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            stopForeground(STOP_FOREGROUND_DETACH)
        } else {
            @Suppress("DEPRECATION")
            stopForeground(false)
        }
        stopSelf()
    }

    private fun buildNotification(): Notification {
        ensureChannel(this)
        val session = MediaSessionPlugin.Holder.session

        val contentIntent = packageManager.getLaunchIntentForPackage(packageName)?.let {
            PendingIntent.getActivity(
                this,
                0,
                it,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )
        }

        val builder = NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentIntent(contentIntent)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setOnlyAlertOnce(true)
            .addAction(
                NotificationCompat.Action(
                    android.R.drawable.ic_media_previous,
                    "Previous",
                    actionIntent(PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS)
                )
            )
            .addAction(
                NotificationCompat.Action(
                    android.R.drawable.ic_media_pause,
                    "Play/Pause",
                    actionIntent(PlaybackStateCompat.ACTION_PLAY_PAUSE)
                )
            )
            .addAction(
                NotificationCompat.Action(
                    android.R.drawable.ic_media_next,
                    "Next",
                    actionIntent(PlaybackStateCompat.ACTION_SKIP_TO_NEXT)
                )
            )

        if (session != null) {
            val meta = session.controller?.metadata
            if (meta != null) {
                builder
                    .setContentTitle(meta.getString(android.media.MediaMetadata.METADATA_KEY_TITLE))
                    .setContentText(meta.getString(android.media.MediaMetadata.METADATA_KEY_ARTIST))
                    .setLargeIcon(meta.getBitmap(android.media.MediaMetadata.METADATA_KEY_ALBUM_ART))
            }
            builder.setStyle(
                MediaStyle()
                    .setMediaSession(session.sessionToken)
                    .setShowActionsInCompactView(0, 1, 2)
            )
        }
        return builder.build()
    }

    private fun actionIntent(action: Long): PendingIntent =
        MediaButtonReceiver.buildMediaButtonPendingIntent(this, action)

    companion object {
        private const val CHANNEL_ID = "aether_playback"
        private const val NOTIFICATION_ID = 1001

        /** Live service instance, so demote() works without startService — which
         *  would be illegal from the background once the app is a cached process. */
        @Volatile
        private var instance: MediaPlaybackService? = null

        /** Demote the running service out of the foreground state (no-op when the
         *  service isn't running). Called by MediaSessionPlugin's pause grace timer. */
        fun demote() {
            instance?.demoteNow()
        }

        private fun ensureChannel(context: Context) {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
            val mgr = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            if (mgr.getNotificationChannel(CHANNEL_ID) == null) {
                mgr.createNotificationChannel(
                    NotificationChannel(
                        CHANNEL_ID,
                        "Playback",
                        NotificationManager.IMPORTANCE_LOW
                    ).apply { setShowBadge(false) }
                )
            }
        }

        /** Re-issue the notification (e.g. after async artwork load). Only while
         *  the service is running — never (re)start it just for artwork, which
         *  would re-promote a service the pause grace timer already demoted. */
        fun refresh(context: Context) {
            if (instance == null) return
            val intent = Intent(context, MediaPlaybackService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }
    }
}
