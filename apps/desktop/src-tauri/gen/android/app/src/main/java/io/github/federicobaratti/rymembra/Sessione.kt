package io.github.federicobaratti.rymembra

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.annotation.OptIn
import androidx.core.content.ContextCompat
import androidx.media3.common.C
import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.CommandButton
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.SessionCommand
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/** Quel che la notifica deve mostrare: la `Scheda` di `rymembra-android`. */
data class Scheda(
  val id: Long,
  val titolo: String,
  val artista: String,
  val album: String,
  val copertina: String?,
  val durataMs: Long,
  val posizioneMs: Long,
  val suona: Boolean,
  val brano: Boolean,
  val dopo: Boolean,
  val piace: Boolean,
  val mescola: Boolean,
  val ripeti: String,
  val indiceCoda: Int?,
) {
  companion object {
    val VUOTA = Scheda(0, "", "", "", null, 0, 0, false, false, false, false, false, "off", null)

    fun da(a: JSONObject) = Scheda(
      id = a.optLong("id"),
      titolo = a.optString("titolo"),
      artista = a.optString("artista"),
      album = a.optString("album"),
      copertina = if (a.isNull("copertina")) null else a.optString("copertina"),
      durataMs = a.optLong("durataMs"),
      posizioneMs = a.optLong("posizioneMs"),
      suona = a.optBoolean("suona"),
      brano = a.optBoolean("brano"),
      dopo = a.optBoolean("dopo"),
      piace = a.optBoolean("piace"),
      mescola = a.optBoolean("mescola"),
      ripeti = a.optString("ripeti", "off"),
      indiceCoda = if (a.isNull("indiceCoda") || !a.has("indiceCoda")) null else a.optInt("indiceCoda"),
    )
  }
}

/** Una voce della coda che l'auto mostra: la `VoceCoda` di `rymembra-android`. */
data class VoceCoda(
  val posto: Int,
  val id: Long,
  val titolo: String,
  val artista: String,
  val album: String,
  val copertina: String?,
  val durataMs: Long,
) {
  companion object {
    fun elenco(a: JSONObject): List<VoceCoda> {
      val voci = a.optJSONArray("voci") ?: JSONArray()
      return (0 until voci.length()).map {
        val v = voci.getJSONObject(it)
        VoceCoda(
          posto = v.optInt("posto"),
          id = v.optLong("id"),
          titolo = v.optString("titolo"),
          artista = v.optString("artista"),
          album = v.optString("album"),
          copertina = if (v.isNull("copertina")) null else v.optString("copertina"),
          durataMs = v.optLong("durataMs"),
        )
      }
    }
  }
}

/**
 * Un giocatore che non suona niente.
 *
 * La musica la fa Rust (`rymembra-play`, via AAudio). Questo giocatore esiste
 * per Media3: dice alla sessione che cosa suona, a che punto, che cosa viene
 * dopo, e trasforma i comandi che arrivano dalla notifica, dalla schermata di
 * blocco, dalle cuffie e dall'auto in gesti per Rust. Non decide niente da sé:
 * anche una pausa chiesta dalle cuffie diventa vera solo quando Rust la rimanda
 * indietro con la scheda dopo.
 */
@OptIn(UnstableApi::class)
class Giocatore(looper: Looper) : SimpleBasePlayer(looper) {
  private var scheda = Scheda.VUOTA
  private var coda: List<VoceCoda> = emptyList()
  private var posizione = PositionSupplier.ZERO
  private var errore: PlaybackException? = null

  /** Sul filo principale, come ogni chiamata a un giocatore di Media3. */
  fun imposta(nuova: Scheda) {
    val prima = scheda
    scheda = nuova
    // Un avviso vale finché la musica non riparte o non cambia brano: vedi
    // `Sessione.errore`.
    if (errore != null && (nuova.id != prima.id || (nuova.suona && !prima.suona))) errore = null
    // Il punto da cui estrapolare si fissa qui, una volta per scheda: Media3
    // chiede lo stato più volte, e un punto rifatto a ogni richiesta farebbe
    // tornare indietro il cursore della notifica a ogni domanda.
    posizione = PositionSupplier.getExtrapolating(nuova.posizioneMs, if (nuova.suona) 1f else 0f)
    invalidateState()
  }

  fun impostaCoda(nuova: List<VoceCoda>) {
    coda = nuova
    invalidateState()
  }

  fun avvisa(eccezione: PlaybackException?) {
    errore = eccezione
    invalidateState()
  }

  val haDopo get() = scheda.dopo
  val attuale get() = scheda

  /** Il brano come lo vedono gli altri: la scheda, se c'è, vince sulla coda. */
  private fun metadati(titolo: String, artista: String, album: String, copertina: String?) =
    MediaMetadata.Builder()
      .setTitle(titolo)
      .setArtist(artista)
      .setAlbumTitle(album)
      .setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC)
      .setIsPlayable(true)
      .setIsBrowsable(false)
      .apply { copertina?.let { setArtworkUri(Copertine.indirizzo(it)) } }
      .build()

  private fun dato(uid: String, mediaId: String, metadati: MediaMetadata, durataMs: Long) =
    MediaItemData.Builder(uid)
      .setMediaItem(MediaItem.Builder().setMediaId(mediaId).setMediaMetadata(metadati).build())
      .setDurationUs(if (durataMs > 0) durataMs * 1_000 else C.TIME_UNSET)
      .setIsSeekable(true)
      .build()

  override fun getState(): State {
    val s = scheda
    val comandi = Player.Commands.Builder()
      .addAll(
        Player.COMMAND_PLAY_PAUSE,
        Player.COMMAND_PREPARE,
        Player.COMMAND_STOP,
        Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
        Player.COMMAND_GET_METADATA,
        Player.COMMAND_GET_TIMELINE,
        Player.COMMAND_SET_MEDIA_ITEM,
        Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_DEFAULT_POSITION,
        Player.COMMAND_SEEK_TO_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_PREVIOUS,
        Player.COMMAND_SEEK_BACK,
        Player.COMMAND_SEEK_FORWARD,
        Player.COMMAND_SET_SHUFFLE_MODE,
        Player.COMMAND_SET_REPEAT_MODE,
      )
      .apply { if (s.dopo) addAll(Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM) }
      .build()
    val stato = State.Builder()
      .setAvailableCommands(comandi)
      .setPlayWhenReady(s.brano && s.suona, Player.PLAY_WHEN_READY_CHANGE_REASON_REMOTE)
      .setShuffleModeEnabled(s.mescola)
      .setRepeatMode(
        when (s.ripeti) {
          "one" -> Player.REPEAT_MODE_ONE
          "all" -> Player.REPEAT_MODE_ALL
          else -> Player.REPEAT_MODE_OFF
        }
      )
      .setSeekBackIncrementMs(PASSO_MS)
      .setSeekForwardIncrementMs(PASSO_MS)
    if (!s.brano) {
      return stato
        .setPlaylist(emptyList())
        .setPlaybackState(Player.STATE_IDLE)
        .setPlayerError(errore)
        .build()
    }
    val corrente = metadati(s.titolo, s.artista, s.album, s.copertina)
    // La coda vera, se quella arrivata da Rust contiene il brano che suona: è
    // quel che l'auto mostra sotto «In coda». Se no — la coda deve ancora
    // arrivare, o è di un momento fa — il brano da solo.
    val qui = s.indiceCoda?.let { indice -> coda.indexOfFirst { it.posto == indice } } ?: -1
    val (elenco, indice) = if (qui >= 0) {
      coda.mapIndexed { n, voce ->
        if (n == qui) {
          dato("q${voce.posto}", "coda:${voce.posto}", corrente, s.durataMs)
        } else {
          dato(
            "q${voce.posto}",
            "coda:${voce.posto}",
            metadati(voce.titolo, voce.artista, voce.album, voce.copertina),
            voce.durataMs,
          )
        }
      } to qui
    } else {
      listOf(dato("q${s.indiceCoda ?: 0}", ATTUALE, corrente, s.durataMs)) to 0
    }
    return stato
      .setPlaylist(elenco)
      .setCurrentMediaItemIndex(indice)
      // Con un avviso aperto il giocatore è fermo per chi guarda: Media3 vuole
      // `STATE_IDLE` accanto a un errore.
      .setPlaybackState(if (errore != null) Player.STATE_IDLE else Player.STATE_READY)
      .setPlayerError(errore)
      .setContentPositionMs(posizione)
      .build()
  }

  override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
    if (!playWhenReady) Log.i(Sessione.PAUSA, "pausa da ${Sessione.chiChiede()}")
    Sessione.gesto(if (playWhenReady) "riprendi" else "pausa")
    // In attesa della scheda vera, quella che si è chiesta: senza, il tasto
    // tornerebbe indietro per un istante e poi avanti.
    imposta(scheda.copy(suona = playWhenReady))
    return Futures.immediateVoidFuture()
  }

  override fun handlePrepare(): ListenableFuture<*> = Futures.immediateVoidFuture()

  /**
   * L'auto, o l'assistente, chiede di suonare qualcosa.
   *
   * Una voce dell'albero arriva col suo identificativo; una ricerca a voce
   * arriva senza, con la frase in `requestMetadata`. La voce [ATTUALE] è quel
   * che c'è già — l'auto la chiede per riprendere — e non fa partire niente:
   * il «riprendi» arriva subito dopo, da solo.
   */
  override fun handleSetMediaItems(
    mediaItems: MutableList<MediaItem>,
    startIndex: Int,
    startPositionMs: Long,
  ): ListenableFuture<*> {
    val voce = mediaItems.getOrNull(if (startIndex in mediaItems.indices) startIndex else 0)
      ?: return Futures.immediateVoidFuture()
    val ricerca = voce.requestMetadata.searchQuery
    when {
      voce.mediaId == ATTUALE -> {}
      voce.mediaId.startsWith("coda:") ->
        voce.mediaId.removePrefix("coda:").toLongOrNull()?.let { Sessione.gesto("coda", it) }
      voce.mediaId.isEmpty() && ricerca != null -> Sessione.cerca(ricerca, voce.requestMetadata.extras)
      voce.mediaId.isEmpty() -> Sessione.cerca("", null)
      else -> Sessione.suona(voce.mediaId)
    }
    return Futures.immediateVoidFuture()
  }

  override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
    when (seekCommand) {
      Player.COMMAND_SEEK_BACK -> Sessione.gesto("indietro", PASSO_MS)
      Player.COMMAND_SEEK_FORWARD -> Sessione.gesto("avanti", PASSO_MS)
      Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> Sessione.gesto("precedente")
      Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> Sessione.gesto("prossimo")
      Player.COMMAND_SEEK_TO_MEDIA_ITEM, Player.COMMAND_SEEK_TO_DEFAULT_POSITION -> {
        // Una voce della coda: il suo posto, che Rust conosce. La voce che
        // suona già è un salto dentro al brano.
        val voce = state.playlist.getOrNull(mediaItemIndex)
        val posto = voce?.mediaItem?.mediaId?.removePrefix("coda:")?.toLongOrNull()
        if (posto != null && mediaItemIndex != state.currentMediaItemIndex) {
          Sessione.gesto("coda", posto)
        } else {
          Sessione.gesto("vai", positionMs.coerceAtLeast(0).takeIf { it != C.TIME_UNSET } ?: 0)
        }
      }
      else -> Sessione.gesto("vai", positionMs.coerceAtLeast(0))
    }
    return Futures.immediateVoidFuture()
  }

  override fun handleSetShuffleModeEnabled(shuffleModeEnabled: Boolean): ListenableFuture<*> {
    if (shuffleModeEnabled != scheda.mescola) {
      Sessione.gesto("mescola")
      imposta(scheda.copy(mescola = shuffleModeEnabled))
    }
    return Futures.immediateVoidFuture()
  }

  /**
   * Rymembra cicla — spenta, tutta la coda, questo brano — e Media3 chiede un
   * modo preciso: si manda un «ripeti» per ogni passo fra i due.
   */
  override fun handleSetRepeatMode(repeatMode: Int): ListenableFuture<*> {
    val giro = listOf("off", "all", "one")
    val voluto = when (repeatMode) {
      Player.REPEAT_MODE_ONE -> "one"
      Player.REPEAT_MODE_ALL -> "all"
      else -> "off"
    }
    val passi = (giro.indexOf(voluto) - giro.indexOf(scheda.ripeti).coerceAtLeast(0) + giro.size) % giro.size
    repeat(passi) { Sessione.gesto("ripeti") }
    if (passi > 0) imposta(scheda.copy(ripeti = voluto))
    return Futures.immediateVoidFuture()
  }

  override fun handleStop(): ListenableFuture<*> {
    Log.i(Sessione.PAUSA, "pausa da uno «stop» di ${Sessione.chiChiede()}")
    Sessione.gesto("pausa")
    return Futures.immediateVoidFuture()
  }

  companion object {
    /** Il passo di «avanti» e «indietro» dalla notifica e dalle cuffie. */
    const val PASSO_MS = 10_000L

    /** La voce che vuol dire «quel che suona già». Vedi `auto::ATTUALE` in Rust. */
    const val ATTUALE = "attuale"
  }
}

/**
 * La sessione: il giocatore, la notifica, l'audio focus, le cuffie.
 *
 * Tutto quel che tocca il giocatore succede sul filo principale. Rust arriva
 * da un filo suo, e [aggiorna] passa la scheda al filo principale con un `post`
 * e torna subito: chi chiama è il filo del seguito del lettore, che non deve
 * aspettare la notifica.
 */
@OptIn(UnstableApi::class)
object Sessione {
  private val principale = Handler(Looper.getMainLooper())
  private val gesti = LinkedBlockingQueue<JSONObject>()
  private var giocatore: Giocatore? = null
  private var avvolto: Player? = null
  private var richiesta: AudioFocusRequest? = null
  private var riprendiDopo = false
  private var ascoltaCuffie = false
  private var contesto: Context? = null
  private var pulsanti: List<Any>? = null

  /** La sessione del servizio, quando c'è: per i pulsanti e per l'albero. */
  var libreria: MediaLibrarySession? = null
    set(valore) {
      field = valore
      pulsanti = null
      giocatore?.let { aggiornaPulsanti(it.attuale) }
    }

  /** I comandi dei pulsanti che non sono di Media3: vedi [aggiornaPulsanti]. */
  const val COMANDO_PIACE = "rymembra.piace"
  const val COMANDO_MESCOLA = "rymembra.mescola"
  const val COMANDO_RIPETI = "rymembra.ripeti"
  val COMANDI = listOf(COMANDO_PIACE, COMANDO_MESCOLA, COMANDO_RIPETI)

  /**
   * Il giocatore che la sessione vede. Avvolto, perché «brano dopo» e «brano
   * prima» li decide Rust: per Media3 la coda è un tratto, e senza questo
   * involucro un «avanti» sull'ultima voce del tratto non farebbe niente.
   */
  fun giocatore(contesto: Context): Player {
    avvolto?.let { return it }
    this.contesto = contesto.applicationContext
    val nuovo = Giocatore(Looper.getMainLooper())
    giocatore = nuovo
    val involucro = object : ForwardingPlayer(nuovo) {
      override fun seekToNext() = gesto("prossimo")
      override fun seekToNextMediaItem() = gesto("prossimo")
      override fun seekToPrevious() = gesto("precedente")
      override fun seekToPreviousMediaItem() = gesto("precedente")
      override fun hasNextMediaItem() = nuovo.haDopo
      override fun hasPreviousMediaItem() = true
    }
    avvolto = involucro
    return involucro
  }

  /** Dove si torna toccando la notifica: l'attività, così com'era. */
  fun ritorno(contesto: Context): PendingIntent =
    PendingIntent.getActivity(
      contesto,
      0,
      Intent(contesto, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    )

  fun gesto(nome: String, ms: Long = 0) {
    mandaGesto(JSONObject().put("gesto", nome).put("ms", ms))
  }

  /** Suona una voce dell'albero dell'auto. */
  /**
   * Un link `rymembra://collega/…` aperto da fuori — il QR letto con la
   * fotocamera del sistema. Va a Rust dalla strada dei gesti, la sola che non
   * aspetta che Rust chieda; da lì la finestra chiede conferma.
   */
  fun collega(link: String) {
    mandaGesto(JSONObject().put("gesto", "collega").put("voce", link))
  }

  /**
   * La rete di sistema è cambiata (vedi `Rete.osserva`): il collegamento via
   * internet cerca strade nuove. Senza svegliare il nucleo: se non c'è, non
   * c'è niente da ricollegare.
   */
  fun cambioDiRete() {
    gesti.offer(JSONObject().put("gesto", "rete"))
  }

  fun suona(voce: String) {
    mandaGesto(JSONObject().put("gesto", "suona").put("voce", voce))
  }

  /**
   * Una ricerca a voce. Gli `extras` sono quelli dell'assistente: artista,
   * album, titolo, quando li ha separati dalla frase.
   */
  fun cerca(testo: String, extras: Bundle?) {
    val ricerca = JSONObject().put("testo", testo)
    extras?.getString(android.provider.MediaStore.EXTRA_MEDIA_ARTIST)?.let { ricerca.put("artista", it) }
    extras?.getString(android.provider.MediaStore.EXTRA_MEDIA_ALBUM)?.let { ricerca.put("album", it) }
    extras?.getString(android.provider.MediaStore.EXTRA_MEDIA_TITLE)?.let { ricerca.put("titolo", it) }
    mandaGesto(JSONObject().put("gesto", "cerca").put("ricerca", ricerca))
  }

  /**
   * In coda per Rust. Se il nucleo non è ancora partito — l'ha svegliato
   * l'auto, con l'app chiusa — lo si sveglia: il gesto aspetta in coda.
   */
  private fun mandaGesto(gesto: JSONObject) {
    gesti.offer(gesto)
    contesto?.let { Nucleo.sveglia(it) }
  }

  /**
   * Le schede che l'auto ha aperte si rileggono: quando Rust dice di esserci
   * (vedi [Nucleo.pronta]), e quando la libreria è cambiata. Da qualunque filo.
   */
  fun nucleoPronto() {
    principale.post { libreria?.let { Biblioteca.nucleoPronto(it) } }
  }

  /** Il prossimo tasto, aspettandolo al più `entroMs`. Da un filo Rust. */
  fun prossimoGesto(entroMs: Long): JSONObject? = gesti.poll(entroMs, TimeUnit.MILLISECONDS)

  fun aggiorna(contesto: Context, scheda: Scheda) {
    principale.post { applica(contesto.applicationContext, scheda) }
  }

  fun coda(contesto: Context, voci: List<VoceCoda>) {
    principale.post {
      giocatore(contesto.applicationContext)
      giocatore?.impostaCoda(voci)
    }
  }

  /**
   * Rust dice che un gesto non ha portato a niente. La frase la si sceglie qui,
   * nella lingua del telefono: è quel che l'auto mostra.
   */
  fun errore(contesto: Context, codice: String) {
    val testo = contesto.getString(
      when (codice) {
        "library.trackNotFound" -> R.string.auto_niente
        else -> R.string.auto_non_riesco
      }
    )
    principale.post {
      giocatore(contesto.applicationContext)
      val g = giocatore ?: return@post
      g.avvisa(PlaybackException(testo, null, PlaybackException.ERROR_CODE_FAILED_RUNTIME_CHECK))
      // E se ne va da sé: un avviso che resta su una musica che suona sembra un
      // guasto che non c'è.
      principale.postDelayed({ g.avvisa(null) }, AVVISO_MS)
    }
  }

  fun stacca(contesto: Context) {
    principale.post {
      applica(contesto.applicationContext, Scheda.VUOTA)
      giocatore?.impostaCoda(emptyList())
      contesto.stopService(Intent(contesto, ServizioMusica::class.java))
    }
  }

  /**
   * La musica suona, secondo l'ultima scheda arrivata da Rust. La legge il
   * collegamento via internet (`sistema.inUso`) da un filo Rust, quindi
   * `@Volatile`.
   */
  @Volatile var suonaOra = false
    private set

  private fun applica(contesto: Context, scheda: Scheda) {
    giocatore(contesto)
    giocatore?.imposta(scheda)
    aggiornaPulsanti(scheda)
    Ricordo.scrivi(contesto, scheda)
    val suona = scheda.brano && scheda.suona
    suonaOra = suona
    fuoco(contesto, suona)
    cuffie(contesto, suona)
  }

  /**
   * I pulsanti oltre a play, avanti e indietro: mi piace, shuffle, ripetizione.
   *
   * Sono quel che l'auto mostra accanto ai comandi del lettore, e quel che la
   * notifica mostra quando la si apre. Si rifanno solo quando cambia quel che
   * mostrano: ogni lista nuova ridisegna la barra dell'auto.
   */
  private fun aggiornaPulsanti(scheda: Scheda) {
    val sessione = libreria ?: return
    val c = contesto ?: return
    val firma = listOf(scheda.brano, scheda.piace, scheda.mescola, scheda.ripeti)
    if (pulsanti == firma) return
    pulsanti = firma
    val elenco = mutableListOf<CommandButton>()
    if (scheda.brano) {
      elenco += CommandButton.Builder(
        if (scheda.piace) CommandButton.ICON_HEART_FILLED else CommandButton.ICON_HEART_UNFILLED
      )
        .setDisplayName(c.getString(if (scheda.piace) R.string.auto_non_piace else R.string.auto_piace))
        .setSessionCommand(SessionCommand(COMANDO_PIACE, Bundle.EMPTY))
        .setSlots(CommandButton.SLOT_OVERFLOW)
        .build()
    }
    elenco += CommandButton.Builder(
      if (scheda.mescola) CommandButton.ICON_SHUFFLE_ON else CommandButton.ICON_SHUFFLE_OFF
    )
      .setDisplayName(c.getString(if (scheda.mescola) R.string.auto_mescola_spento else R.string.auto_mescola))
      .setSessionCommand(SessionCommand(COMANDO_MESCOLA, Bundle.EMPTY))
      .setSlots(CommandButton.SLOT_OVERFLOW)
      .build()
    elenco += CommandButton.Builder(
      when (scheda.ripeti) {
        "one" -> CommandButton.ICON_REPEAT_ONE
        "all" -> CommandButton.ICON_REPEAT_ALL
        else -> CommandButton.ICON_REPEAT_OFF
      }
    )
      .setDisplayName(
        c.getString(
          when (scheda.ripeti) {
            "one" -> R.string.auto_ripeti_uno
            "all" -> R.string.auto_ripeti_tutto
            else -> R.string.auto_ripeti_spento
          }
        )
      )
      .setSessionCommand(SessionCommand(COMANDO_RIPETI, Bundle.EMPTY))
      .setSlots(CommandButton.SLOT_OVERFLOW)
      .build()
    sessione.setMediaButtonPreferences(elenco)
  }

  /**
   * L'audio focus: una chiamata, un navigatore, un'altra app che suona.
   *
   * Una perdita breve mette in pausa e, al ritorno, riprende; una perdita per
   * sempre mette in pausa e basta. Mentre si aspetta il ritorno il focus non si
   * lascia: lasciandolo, il ritorno non arriverebbe mai.
   *
   * Il navigatore in auto chiede un focus «abbassabile» (`MAY_DUCK`): lì
   * Android abbassa il volume da sé, e la musica non si ferma.
   */
  private fun fuoco(contesto: Context, suona: Boolean) {
    val audio = contesto.getSystemService(AudioManager::class.java) ?: return
    if (suona) {
      riprendiDopo = false
      if (richiesta != null) return
      if (!chiediIlFuoco(audio)) Log.i(PAUSA, "si suona senza il fuoco audio")
    } else if (!riprendiDopo) {
      lasciaIlFuoco(audio)
    }
  }

  /** Chiede il fuoco audio, e lo tiene in [richiesta] se glielo danno. */
  private fun chiediIlFuoco(audio: AudioManager): Boolean {
    val nuova = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
      .setAudioAttributes(
        AudioAttributes.Builder()
          .setUsage(AudioAttributes.USAGE_MEDIA)
          .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
          .build()
      )
      .setOnAudioFocusChangeListener({ cambio ->
        when (cambio) {
          AudioManager.AUDIOFOCUS_LOSS -> {
            Log.i(PAUSA, "pausa: il fuoco audio è passato a un'altra app, per sempre")
            riprendiDopo = false
            gesto("pausa")
          }
          AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
            Log.i(PAUSA, "pausa: il fuoco audio è passato a un'altra app, per poco")
            riprendiDopo = true
            gesto("pausa")
          }
          AudioManager.AUDIOFOCUS_GAIN -> if (riprendiDopo) {
            riprendiDopo = false
            gesto("riprendi")
          }
        }
      }, principale)
      .build()
    val esito = audio.requestAudioFocus(nuova)
    if (esito != AudioManager.AUDIOFOCUS_REQUEST_GRANTED) {
      // `AUDIOFOCUS_REQUEST_FAILED` è lo stesso esito di una chiamata in corso, di
      // un'app che non cede il fuoco e — da Android 17 con `targetSdk` 37 — del
      // sistema che lo nega a chi non ha un servizio in primo piano con la
      // capacità «while-in-use» (S6-3): l'API non li distingue, e il codice non
      // dice quale dei tre è. Dice l'esito, il livello di Android e dove guardare.
      Log.i(
        PAUSA,
        "il fuoco audio non è stato concesso (esito $esito, Android API ${Build.VERSION.SDK_INT}): " +
          "una chiamata, un'altra app, o dal 17 il sistema (cerca «AudioHardening» nel logcat)",
      )
      return false
    }
    richiesta = nuova
    return true
  }

  private fun lasciaIlFuoco(audio: AudioManager) {
    richiesta?.let { audio.abandonAudioFocusRequest(it) }
    richiesta = null
  }

  /**
   * Il fuoco audio per **far partire** la musica da soli — «Parti con l'auto»,
   * vedi [Presenza] —: lo si chiede prima del «riprendi», come vuole Android
   * per chi suona, invece di prenderlo quando la scheda dice che suona. Se Android
   * non lo concede, `false`: non si parte. Non si sa perché: una chiamata in
   * corso, un'altra app che non lo cede e — da Android 17 — la restrizione
   * sull'audio dallo sfondo (un servizio avviato all'accensione o
   * dall'aggiornamento non ha la capacità «while-in-use») tornano tutte
   * `AUDIOFOCUS_REQUEST_FAILED`, e nessuna API le distingue ([chiediIlFuoco]).
   *
   * Se poi la musica non parte — un errore in Rust, una coda che non si apre —
   * nessuna scheda con «suona» arriva a dire che il fuoco serve ancora, e
   * tenerlo vorrebbe dire zittire per sempre l'app che prima parlava: dopo
   * [ATTESA_PARTENZA_MS] lo si lascia se ancora non suona. Sul filo principale.
   */
  fun fuocoPerPartire(contesto: Context): Boolean {
    val audio = contesto.getSystemService(AudioManager::class.java) ?: return false
    if (richiesta == null && !chiediIlFuoco(audio)) return false
    principale.postDelayed({
      if (!suonaOra && !riprendiDopo) lasciaIlFuoco(audio)
    }, ATTESA_PARTENZA_MS)
    return true
  }

  /** Quanto si aspetta che la musica parta, dopo aver preso il fuoco per partire. */
  private const val ATTESA_PARTENZA_MS = 15_000L

  private val rumore = object : BroadcastReceiver() {
    override fun onReceive(contesto: Context, intento: Intent) {
      if (intento.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) {
        Log.i(PAUSA, "pausa: le cuffie si sono staccate")
        gesto("pausa")
      }
    }
  }

  /** Le cuffie staccate mettono in pausa: la musica non passa all'altoparlante. */
  private fun cuffie(contesto: Context, suona: Boolean) {
    if (suona && !ascoltaCuffie) {
      ContextCompat.registerReceiver(
        contesto,
        rumore,
        IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY),
        ContextCompat.RECEIVER_NOT_EXPORTED
      )
      ascoltaCuffie = true
    } else if (!suona && ascoltaCuffie) {
      contesto.unregisterReceiver(rumore)
      ascoltaCuffie = false
    }
  }

  /** Quanto resta sullo schermo dell'auto un avviso. */
  private const val AVVISO_MS = 6_000L

  /**
   * L'etichetta del logcat per ogni pausa che non viene dalla finestra: il
   * fuoco audio, le cuffie, un controllore della sessione (col suo pacchetto:
   * vedi [chiChiede]). Una musica che si ferma da sola —
   * il «Play» del primo avvio che resta a 0:00 (R18-5) — si spiega così:
   * `adb logcat -s RymembraPausa`. Nel diario no: i nomi dei pacchetti delle
   * altre app non ci vanno.
   */
  const val PAUSA = "RymembraPausa"

  /**
   * Il controllore che sta chiedendo qualcosa al giocatore adesso, per il
   * logcat: pacchetto e uid. Lo sa la sessione, sul filo principale, mentre
   * esegue il comando.
   */
  fun chiChiede(): String =
    libreria?.controllerForCurrentRequest?.let { "${it.packageName} (uid ${it.uid})" }
      ?: "un controllore che la sessione non nomina"
}
