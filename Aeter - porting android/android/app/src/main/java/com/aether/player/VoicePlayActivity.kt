package com.aether.player

import android.app.Activity
import android.os.Bundle
import android.provider.MediaStore

/**
 * Phone-side voice entry point (App Actions PLAY_MUSIC, see res/xml/shortcuts.xml):
 * a no-display activity that turns the assistant's entity parameters into the same
 * MediaStore focus bundle the in-car path receives, resolves it with [AutoVoice]
 * and dispatches playback through [AetherAuto] — then finishes immediately.
 *
 * Resolution runs off the main thread (the first catalog load parses the snapshot
 * file); the process outlives this activity, and a cold "play" launches
 * MainActivity through AetherAuto exactly like a cold car request.
 */
class VoicePlayActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val track = intent.getStringExtra("track").orEmpty()
        val artist = intent.getStringExtra("artist").orEmpty()
        val album = intent.getStringExtra("album").orEmpty()
        val playlist = intent.getStringExtra("playlist").orEmpty()

        val extras = Bundle()
        val query: String
        when {
            playlist.isNotEmpty() -> {
                extras.putString(MediaStore.EXTRA_MEDIA_FOCUS, MediaStore.Audio.Playlists.ENTRY_CONTENT_TYPE)
                extras.putString(MediaStore.EXTRA_MEDIA_PLAYLIST, playlist)
                query = playlist
            }
            album.isNotEmpty() -> {
                extras.putString(MediaStore.EXTRA_MEDIA_FOCUS, MediaStore.Audio.Albums.ENTRY_CONTENT_TYPE)
                extras.putString(MediaStore.EXTRA_MEDIA_ALBUM, album)
                if (artist.isNotEmpty()) extras.putString(MediaStore.EXTRA_MEDIA_ARTIST, artist)
                query = album
            }
            track.isNotEmpty() -> {
                extras.putString(MediaStore.EXTRA_MEDIA_FOCUS, MediaStore.Audio.Media.ENTRY_CONTENT_TYPE)
                extras.putString(MediaStore.EXTRA_MEDIA_TITLE, track)
                if (artist.isNotEmpty()) extras.putString(MediaStore.EXTRA_MEDIA_ARTIST, artist)
                query = listOf(track, artist).filter { it.isNotEmpty() }.joinToString(" ")
            }
            artist.isNotEmpty() -> {
                extras.putString(MediaStore.EXTRA_MEDIA_FOCUS, MediaStore.Audio.Artists.ENTRY_CONTENT_TYPE)
                extras.putString(MediaStore.EXTRA_MEDIA_ARTIST, artist)
                query = artist
            }
            else -> query = "" // "metti musica" → AutoVoice resumes/falls back
        }

        val app = applicationContext
        Thread {
            try {
                AutoVoice.resolve(app, query, extras)?.let { AetherAuto.play(app, it) }
            } catch (_: Exception) {
                // best-effort: a phone voice miss has no session to speak through
            }
        }.start()
        finish()
    }
}
