package io.github.federicobaratti.rymembra

import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import androidx.annotation.OptIn
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.util.UnstableApi

/**
 * Il nucleo Rust: se è partito, e come svegliarlo quando non c'è.
 *
 * # Perché può non esserci
 *
 * Il nucleo parte con l'attività: è `TauriActivity` che carica la libreria
 * nativa e fa girare il ciclo di Tauri, e Tauri senza un'attività non sa
 * partire (registra i suoi plugin Android sull'attività, e senza va in panico).
 * Il servizio della musica invece può nascere da solo: lo sveglia l'auto, o il
 * sistema per la scheda di ripresa, con l'app chiusa da ore.
 *
 * # Come lo si sveglia
 *
 * Aprendo l'attività sullo sfondo: si apre, il nucleo parte, e lei si
 * rimette subito dietro (`MainActivity`, con [SFONDO]). Da Android 10 un'app
 * che non si vede non può aprire la sua schermata, salvo eccezioni che non
 * dipendono da lei; quando Android dice di no, la domanda dell'auto scade e
 * l'auto mostra «Apri Rymembra» — vedi `Biblioteca.senzaNucleo`.
 *
 * Mai alla sola nascita del servizio: il sistema lo sveglia anche dopo un
 * riavvio, per sapere l'ultimo brano, e aprire l'app per quello non lo ha
 * chiesto nessuno. Si sveglia quando serve davvero: una domanda sulla libreria,
 * un gesto.
 */
object Nucleo {
  private val principale = Handler(Looper.getMainLooper())

  /** Il nucleo ha detto di esserci (`sessione.pronta`). */
  @Volatile var pronto = false
    private set

  @Volatile private var svegliatoAlle = 0L

  /** Rust ha finito di partire: d'ora in poi le domande hanno risposta. */
  fun pronta(contesto: Context, copertine: String) {
    Copertine.ricorda(contesto, copertine)
    principale.post {
      pronto = true
      Sessione.nucleoPronto()
    }
  }

  /** Sveglia il nucleo se non c'è, una volta ogni tanto. */
  fun sveglia(contesto: Context) {
    if (pronto || Ponte.agganciato) return
    val adesso = SystemClock.elapsedRealtime()
    if (svegliatoAlle != 0L && adesso - svegliatoAlle < RIPROVA_MS) return
    svegliatoAlle = adesso
    try {
      contesto.startActivity(
        Intent(contesto, MainActivity::class.java)
          .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_ANIMATION)
          .putExtra(SFONDO, true)
      )
    } catch (e: Exception) {
      Log.w("Rymembra", "il nucleo non si sveglia: ${e.message}")
    }
  }

  /** L'attività aperta per svegliare il nucleo, non per farsi vedere. */
  const val SFONDO = "io.github.federicobaratti.rymembra.SFONDO"

  /** Fra un tentativo e l'altro: Android, se dice di no, dice di no a tutti. */
  private const val RIPROVA_MS = 20_000L
}

/**
 * L'ultimo brano, ricordato fuori dal nucleo.
 *
 * Per la scheda di ripresa che il sistema mostra dopo un riavvio, e per l'auto
 * che chiede «riprendi» con l'app chiusa: in quei momenti il nucleo non c'è, e
 * il titolo e la copertina di ieri si leggono da qui. Solo quel che si vede —
 * titolo, artista, album, copertina — e dove si era arrivati.
 */
@OptIn(UnstableApi::class)
object Ricordo {
  private const val FILE = "ricordo"
  private var scritto: Long = -1

  fun scrivi(contesto: Context, scheda: Scheda) {
    if (!scheda.brano) return
    val preferenze = contesto.getSharedPreferences(FILE, Context.MODE_PRIVATE)
    val editor = preferenze.edit().putLong("posizioneMs", scheda.posizioneMs)
    if (scheda.id != scritto) {
      scritto = scheda.id
      editor
        .putLong("id", scheda.id)
        .putString("titolo", scheda.titolo)
        .putString("artista", scheda.artista)
        .putString("album", scheda.album)
        .putString("copertina", scheda.copertina)
        .putLong("durataMs", scheda.durataMs)
    }
    editor.apply()
  }

  fun posizioneMs(contesto: Context): Long =
    contesto.getSharedPreferences(FILE, Context.MODE_PRIVATE).getLong("posizioneMs", 0)

  fun voce(contesto: Context): MediaItem? {
    val p = contesto.getSharedPreferences(FILE, Context.MODE_PRIVATE)
    val titolo = p.getString("titolo", null) ?: return null
    return MediaItem.Builder()
      .setMediaId(Giocatore.ATTUALE)
      .setMediaMetadata(
        MediaMetadata.Builder()
          .setTitle(titolo)
          .setArtist(p.getString("artista", null))
          .setAlbumTitle(p.getString("album", null))
          .setIsPlayable(true)
          .setIsBrowsable(false)
          .setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC)
          .apply { p.getString("copertina", null)?.let { setArtworkUri(Copertine.indirizzo(it)) } }
          .apply { p.getLong("durataMs", 0).takeIf { it > 0 }?.let { setDurationMs(it) } }
          .build()
      )
      .build()
  }
}
