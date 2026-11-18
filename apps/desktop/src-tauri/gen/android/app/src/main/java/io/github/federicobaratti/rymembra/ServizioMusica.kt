package io.github.federicobaratti.rymembra

import android.content.Intent
import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession

/**
 * Il servizio che tiene viva la musica a schermo spento, e da cui passa l'auto.
 *
 * Media3 lo porta in primo piano da sé quando il giocatore suona — con la
 * notifica del lettore, l'unica forma in cui Android permette a un'app lasciata
 * sullo sfondo di continuare a lavorare — e lo rimette sullo sfondo in pausa.
 *
 * È un `MediaLibraryService` e non un semplice servizio di sessione perché è
 * così che Android Auto riconosce un lettore: lo trova da quest'intento nel
 * manifest, gli chiede l'albero della libreria e da lì sceglie che cosa
 * suonare. L'albero lo dà [Biblioteca]; la sessione è quella di [Sessione], e
 * il servizio la ospita, non la decide.
 */
@OptIn(UnstableApi::class)
class ServizioMusica : MediaLibraryService() {
  private var sessione: MediaLibrarySession? = null

  override fun onCreate() {
    super.onCreate()
    val nuova = MediaLibrarySession.Builder(this, Sessione.giocatore(this), Biblioteca(this))
      .setSessionActivity(Sessione.ritorno(this))
      // L'errore «apri Rymembra sul telefono» deve arrivare all'auto come
      // errore della sessione, col suo pulsante: vedi `Biblioteca.senzaNucleo`.
      .setLibraryErrorReplicationMode(MediaLibrarySession.LIBRARY_ERROR_REPLICATION_MODE_FATAL)
      .build()
    sessione = nuova
    Sessione.libreria = nuova
  }

  override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession? = sessione

  /**
   * L'app tolta dalle recenti: se suona, continua — è il punto di un lettore —
   * altrimenti il servizio se ne va, e con lui la notifica.
   *
   * Con «Resta attiva in sottofondo» acceso (vedi [Presenza]) non si ferma
   * niente: tolta dalle recenti l'app deve restare com'era, e la sessione
   * resta dov'è, pronta. Il processo lo tiene `Presenza`; qui basta non
   * spegnere l'unico pezzo che il gesto di togliere l'app spegnerebbe.
   */
  override fun onTaskRemoved(rootIntent: Intent?) {
    if (Presenza.voluto(this)) return
    val giocatore = sessione?.player
    if (giocatore == null || !giocatore.playWhenReady) stopSelf()
  }

  override fun onDestroy() {
    // La sessione sì, il giocatore no: è di `Sessione`, e serve ancora se il
    // servizio rinasce.
    Sessione.libreria = null
    sessione?.release()
    sessione = null
    super.onDestroy()
  }
}
