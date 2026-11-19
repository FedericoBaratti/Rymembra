package io.github.federicobaratti.rymembra

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.annotation.OptIn
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.util.UnstableApi

/**
 * Il nucleo Rust: se è partito, e come svegliarlo quando non c'è.
 *
 * # Chi lo avvia
 *
 * Il nucleo — la libreria, il lettore, la sessione, il collegamento con gli
 * altri dispositivi — non dipende da una finestra, e parte in uno di due modi,
 * sempre **una volta per processo**:
 *
 * - dall'attività: `TauriActivity` carica la libreria nativa e fa girare il
 *   ciclo di Tauri, che nel `setup` avvia il nucleo;
 * - da un servizio, senza attività ([Ponte.avviaIlNucleo]): `Presenza` quando il
 *   sottofondo è acceso — anche dopo un riavvio del telefono, dal ricevitore
 *   [AvvioAccensione] — e [sveglia], quando a chiamare è l'auto o il sistema
 *   con l'app chiusa da ore.
 *
 * Tauri invece senza un'attività non sa partire (registra i suoi plugin Android
 * sull'attività, e senza va in panico): quando l'attività nasce dopo, il suo
 * `setup` si aggancia al nucleo che c'è già, e non ne fa un secondo.
 *
 * # Quando lo si sveglia
 *
 * Mai alla sola nascita del servizio della musica: il sistema lo sveglia anche
 * dopo un riavvio, per sapere l'ultimo brano, e avviare il nucleo per quello non
 * lo ha chiesto nessuno. Si sveglia quando serve davvero: una domanda sulla
 * libreria, un gesto. È un gesto dell'utente nell'auto, e vale anche con
 * «Resta attiva in sottofondo» spento: il nucleo parte, l'auto trova Rymembra, e
 * al resto pensa Android — senza il servizio di sottofondo il processo può
 * essere chiuso quando nessuno suona.
 */
object Nucleo {
  private val principale = Handler(Looper.getMainLooper())

  /** Il nucleo ha detto di esserci (`sessione.pronta`). */
  @Volatile var pronto = false
    private set

  @Volatile private var svegliatoAlle = 0L

  /**
   * Rust ha finito di partire: d'ora in poi le domande hanno risposta. Porta le
   * cartelle delle copertine: lo store, e la cache degli altri dispositivi
   * (`lontane`, `null` se non c'è).
   */
  fun pronta(contesto: Context, copertine: String, lontane: String?) {
    Copertine.ricorda(contesto, copertine, lontane)
    principale.post {
      pronto = true
      // Se un tentativo di prima era fallito, adesso il nucleo c'è: via «a riposo».
      Ponte.segnaNucleoParte()
      Sessione.nucleoPronto()
      // La notifica del sottofondo passa da «si sta avviando» a «è attiva».
      Presenza.rinfresca(contesto)
    }
  }

  /**
   * Sveglia il nucleo se non c'è, una volta ogni tanto: lo avvia da qui, senza
   * attività. Se un'attività è già nata, il nucleo lo avvia Tauri (o l'ha già
   * fatto), e non c'è niente da fare.
   */
  fun sveglia(contesto: Context) {
    if (pronto) return
    val adesso = SystemClock.elapsedRealtime()
    if (svegliatoAlle != 0L && adesso - svegliatoAlle < RIPROVA_MS) return
    svegliatoAlle = adesso
    val applicazione = contesto.applicationContext
    principale.post { Ponte.avviaIlNucleo(applicazione) }
  }

  /** Fra un tentativo e l'altro. */
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
  private var copertinaScritta: String? = null

  fun scrivi(contesto: Context, scheda: Scheda) {
    if (!scheda.brano) return
    val preferenze = contesto.getSharedPreferences(FILE, Context.MODE_PRIVATE)
    val editor = preferenze.edit().putLong("posizioneMs", scheda.posizioneMs)
    // Anche quando cambia solo la copertina: quella di un brano di un altro
    // dispositivo arriva dopo il brano, e un ricordo con l'id giusto e la
    // copertina vuota mostrerebbe, dopo un riavvio, una scheda senza immagine.
    if (scheda.id != scritto || scheda.copertina != copertinaScritta) {
      scritto = scheda.id
      copertinaScritta = scheda.copertina
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
