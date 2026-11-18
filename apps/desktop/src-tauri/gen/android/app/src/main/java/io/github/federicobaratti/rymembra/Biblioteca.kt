package io.github.federicobaratti.rymembra

import android.content.Context
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.annotation.OptIn
import androidx.core.content.ContextCompat
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaConstants
import androidx.media3.session.MediaLibraryService.LibraryParams
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSession.ConnectionResult
import androidx.media3.session.MediaSession.MediaItemsWithStartPosition
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionError
import androidx.media3.session.SessionResult
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong

/**
 * La libreria come la vede l'auto: l'albero che Android Auto sfoglia, la
 * ricerca, e quel che si sceglie di suonare.
 *
 * Il primo livello — la radice e le sue quattro schede — si costruisce qui,
 * con i nomi delle risorse, nella lingua del telefono: l'auto lo chiede appena
 * si collega, anche quando il nucleo non è ancora partito. Da lì in giù ogni
 * domanda va a Rust (`src/auto.rs`), per la stessa strada dei gesti: una coda
 * che un filo Rust svuota ([prossimaRichiesta]), e una risposta ([risposta])
 * che completa la promessa data a Media3.
 *
 * Se il nucleo non risponde in tempo perché non c'è — l'app chiusa, e Android
 * che non la lascia partire da sola — l'auto riceve un errore con un pulsante,
 * «Apri Rymembra»: vedi [senzaNucleo].
 */
@OptIn(UnstableApi::class)
class Biblioteca(private val contesto: Context) : MediaLibrarySession.Callback {
  /** I risultati di una ricerca, tenuti fra `onSearch` e `onGetSearchResult`. */
  private val ricerche = ConcurrentHashMap<String, List<MediaItem>>()

  /**
   * Chi entra ha tutto — la libreria, la ricerca, la coda, «mi piace» — e
   * per questo entra solo chi è fidato: vedi [Controllori]. Gli altri sono
   * rifiutati, e non arrivano a nessuna delle domande qui sotto.
   */
  override fun onConnect(
    session: MediaSession,
    controller: MediaSession.ControllerInfo,
  ): ConnectionResult {
    if (!Controllori.fidato(contesto, controller)) {
      Log.i("Rymembra", "controllore rifiutato: ${controller.packageName}")
      return ConnectionResult.reject()
    }
    val comandi = ConnectionResult.DEFAULT_SESSION_AND_LIBRARY_COMMANDS.buildUpon()
      .apply { Sessione.COMANDI.forEach { add(SessionCommand(it, Bundle.EMPTY)) } }
      .build()
    return ConnectionResult.AcceptedResultBuilder(session)
      .setAvailableSessionCommands(comandi)
      .build()
  }

  override fun onCustomCommand(
    session: MediaSession,
    controller: MediaSession.ControllerInfo,
    customCommand: SessionCommand,
    args: Bundle,
  ): ListenableFuture<SessionResult> {
    when (customCommand.customAction) {
      Sessione.COMANDO_PIACE -> Sessione.gesto("piace")
      Sessione.COMANDO_MESCOLA -> Sessione.gesto("mescola")
      Sessione.COMANDO_RIPETI -> Sessione.gesto("ripeti")
      else -> return Futures.immediateFuture(SessionResult(SessionError.ERROR_NOT_SUPPORTED))
    }
    return Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
  }

  override fun onGetLibraryRoot(
    session: MediaLibrarySession,
    browser: MediaSession.ControllerInfo,
    params: LibraryParams?,
  ): ListenableFuture<LibraryResult<MediaItem>> {
    // «L'ultima cosa suonata»: un albero di una voce sola, per chi lo chiede.
    //
    // La scheda di ripresa delle impostazioni rapide arriva fin qui solo se
    // Media3 trova un `MediaButtonReceiver` **abilitato**: `RicevitoreTasti`
    // c'è nel manifesto, ma lo accende `Presenza` soltanto con «Resta attiva in
    // sottofondo» acceso. Il ricevitore fa nascere il servizio in primo piano
    // dal tasto play, e Android lo chiude se entro pochi secondi non suona
    // niente: col sottofondo il processo c'è, e con lui il nucleo che suona;
    // a sottofondo spento il nucleo non c'è, e Android non lascia aprire
    // l'attività che lo accenderebbe (vedi `Nucleo`). Una scheda il cui play
    // fa cadere l'app è peggio di nessuna scheda: a sottofondo spento il
    // ricevitore resta spento, e questo ramo non lo chiede nessuno.
    if (params?.isRecent == true) {
      return Futures.immediateFuture(
        LibraryResult.ofItem(cartella(RECENTE, contesto.getString(R.string.auto_recenti)), params)
      )
    }
    val extras = Bundle().apply {
      putBoolean(RICERCA_POSSIBILE, true)
      putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE, MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM)
      putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_PLAYABLE, MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM)
    }
    return Futures.immediateFuture(
      LibraryResult.ofItem(
        cartella(RADICE, contesto.getString(R.string.app_name)),
        LibraryParams.Builder().setExtras(extras).build(),
      )
    )
  }

  override fun onGetChildren(
    session: MediaLibrarySession,
    browser: MediaSession.ControllerInfo,
    parentId: String,
    page: Int,
    pageSize: Int,
    params: LibraryParams?,
  ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> =
    when (parentId) {
      RADICE -> Futures.immediateFuture(LibraryResult.ofItemList(if (page == 0) schede() else emptyList(), params))
      RECENTE -> Futures.immediateFuture(
        LibraryResult.ofItemList(if (page == 0) listOfNotNull(ultimo()) else emptyList(), params)
      )
      else -> chiedi(
        JSONObject().put("tipo", "figli").put("genitore", parentId).put("pagina", page).put("quante", pageSize),
        params,
      )
    }

  override fun onGetItem(
    session: MediaLibrarySession,
    browser: MediaSession.ControllerInfo,
    mediaId: String,
  ): ListenableFuture<LibraryResult<MediaItem>> {
    if (mediaId == RADICE) {
      return Futures.immediateFuture(LibraryResult.ofItem(cartella(RADICE, contesto.getString(R.string.app_name)), null))
    }
    schede().firstOrNull { it.mediaId == mediaId }?.let {
      return Futures.immediateFuture(LibraryResult.ofItem(it, null))
    }
    if (mediaId == Giocatore.ATTUALE) {
      return Futures.immediateFuture(
        ultimo()?.let { LibraryResult.ofItem(it, null) } ?: LibraryResult.ofError(SessionError.ERROR_BAD_VALUE)
      )
    }
    val elenco = chiedi(JSONObject().put("tipo", "voce").put("voce", mediaId), null)
    return Futures.transform(elenco, { risultato ->
      val voce = risultato?.value?.firstOrNull()
      when {
        voce != null -> LibraryResult.ofItem(voce, null)
        risultato?.resultCode == LibraryResult.RESULT_SUCCESS -> LibraryResult.ofError(SessionError.ERROR_BAD_VALUE)
        else -> LibraryResult.ofError(risultato?.sessionError ?: SessionError(SessionError.ERROR_UNKNOWN, ""))
      }
    }, ContextCompat.getMainExecutor(contesto))
  }

  override fun onSearch(
    session: MediaLibrarySession,
    browser: MediaSession.ControllerInfo,
    query: String,
    params: LibraryParams?,
  ): ListenableFuture<LibraryResult<Void>> {
    val elenco = chiedi(
      JSONObject().put("tipo", "cerca").put("testo", query).put("pagina", 0).put("quante", PRIMA_PAGINA),
      params,
    )
    elenco.addListener({
      val voci = runCatching { elenco.get().value }.getOrNull() ?: emptyList()
      ricerche[query] = voci
      session.notifySearchResultChanged(browser, query, voci.size, params)
    }, ContextCompat.getMainExecutor(contesto))
    return Futures.immediateFuture(LibraryResult.ofVoid())
  }

  override fun onGetSearchResult(
    session: MediaLibrarySession,
    browser: MediaSession.ControllerInfo,
    query: String,
    page: Int,
    pageSize: Int,
    params: LibraryParams?,
  ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> {
    val pronti = ricerche.remove(query)
    if (pronti != null && page == 0 && pageSize >= pronti.size) {
      return Futures.immediateFuture(LibraryResult.ofItemList(pronti, params))
    }
    return chiedi(
      JSONObject().put("tipo", "cerca").put("testo", query).put("pagina", page).put("quante", pageSize),
      params,
    )
  }

  /**
   * L'auto sceglie una voce: la si consegna al giocatore tale e quale, perché
   * è il giocatore che la trasforma in un gesto per Rust (vedi
   * `Giocatore.handleSetMediaItems`). Media3 vorrebbe qui le voci «risolte» —
   * con un file da aprire — ma il file lo apre Rust.
   */
  override fun onSetMediaItems(
    mediaSession: MediaSession,
    controller: MediaSession.ControllerInfo,
    mediaItems: MutableList<MediaItem>,
    startIndex: Int,
    startPositionMs: Long,
  ): ListenableFuture<MediaItemsWithStartPosition> =
    Futures.immediateFuture(MediaItemsWithStartPosition(mediaItems, startIndex, startPositionMs))

  override fun onAddMediaItems(
    mediaSession: MediaSession,
    controller: MediaSession.ControllerInfo,
    mediaItems: MutableList<MediaItem>,
  ): ListenableFuture<MutableList<MediaItem>> = Futures.immediateFuture(mediaItems)

  /**
   * «Riprendi» con il lettore vuoto: dopo un riavvio, dalla scheda di ripresa o
   * dall'auto. Si risponde con l'ultimo brano ricordato; il «riprendi» che
   * segue sveglia il nucleo, che rimette la coda di ieri al suo posto.
   */
  override fun onPlaybackResumption(
    mediaSession: MediaSession,
    controller: MediaSession.ControllerInfo,
  ): ListenableFuture<MediaItemsWithStartPosition> {
    val voce = ultimo()
      ?: return Futures.immediateFailedFuture(UnsupportedOperationException("niente da riprendere"))
    Nucleo.sveglia(contesto)
    return Futures.immediateFuture(MediaItemsWithStartPosition(listOf(voce), 0, Ricordo.posizioneMs(contesto)))
  }

  // ── le voci fatte qui ──────────────────────────────────────────────────────

  private fun cartella(id: String, titolo: String, extras: Bundle? = null, icona: String? = null) =
    MediaItem.Builder()
      .setMediaId(id)
      .setMediaMetadata(
        MediaMetadata.Builder()
          .setTitle(titolo)
          .setIsBrowsable(true)
          .setIsPlayable(false)
          .setMediaType(MediaMetadata.MEDIA_TYPE_FOLDER_MIXED)
          .apply { icona?.let { setArtworkUri(risorsa(it)) } }
          .apply { extras?.let { setExtras(it) } }
          .build()
      )
      .build()

  /** Le quattro schede: l'auto ne mostra al più quattro, in alto. */
  private fun schede(): List<MediaItem> = listOf(
    cartella(
      "recenti",
      contesto.getString(R.string.auto_recenti),
      stile(giocabili = ELENCO, sfogliabili = GRIGLIA),
      "auto_recenti",
    ),
    cartella("playlist", contesto.getString(R.string.auto_playlist), stile(sfogliabili = ELENCO), "auto_playlist"),
    cartella("album", contesto.getString(R.string.auto_album), stile(sfogliabili = GRIGLIA), "auto_album"),
    cartella("artisti", contesto.getString(R.string.auto_artisti), stile(sfogliabili = ELENCO), "auto_artisti"),
  )

  /** L'ultimo brano: quello che suona, o il ricordo di quello di ieri. */
  private fun ultimo(): MediaItem? = Ricordo.voce(contesto)

  private fun risorsa(nome: String): Uri =
    Uri.parse("android.resource://${contesto.packageName}/drawable/$nome")

  private fun stile(giocabili: Int? = null, sfogliabili: Int? = null) = Bundle().apply {
    giocabili?.let { putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_PLAYABLE, it) }
    sfogliabili?.let { putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE, it) }
  }

  // ── le voci che arrivano da Rust ───────────────────────────────────────────

  /** Una `Voce` di `src/auto.rs`, nella voce di Media3. */
  private fun voce(v: JSONObject): MediaItem {
    val id = v.getString("id")
    val tipo = v.optString("tipo")
    val artista = v.stringa("artista")
    val quanti = if (v.isNull("quanti") || !v.has("quanti")) null else v.optInt("quanti")
    val risorse = contesto.resources
    val titolo = when (tipo) {
      "preferiti" -> contesto.getString(R.string.auto_preferiti)
      "tutto" -> contesto.getString(R.string.auto_suona_tutto)
      else -> v.optString("titolo")
    }
    val sottotitolo = when (tipo) {
      "brano", "album" -> artista
      "artista" -> quanti?.let { risorse.getQuantityString(R.plurals.auto_n_album, it, it) }
      "playlist", "tutto" -> quanti?.let { risorse.getQuantityString(R.plurals.auto_n_brani, it, it) }
      "lettera" -> quanti?.let {
        if (id.startsWith("lettera-artisti:")) risorse.getQuantityString(R.plurals.auto_n_artisti, it, it)
        else risorse.getQuantityString(R.plurals.auto_n_album, it, it)
      }
      else -> null
    }
    val extras = when (tipo) {
      "album", "playlist", "preferiti" -> stile(giocabili = ELENCO)
      "artista" -> stile(sfogliabili = GRIGLIA)
      "lettera" -> stile(sfogliabili = if (id.startsWith("lettera-artisti:")) ELENCO else GRIGLIA).apply {
        // In una griglia di copertine una lettera è una riga: senza copertina,
        // un riquadro vuoto con una «A» sotto non lo legge nessuno.
        putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_SINGLE_ITEM, MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_CATEGORY_LIST_ITEM)
      }
      // Una riga anche in una griglia di copertine: è un pulsante, non un disco.
      "tutto" -> Bundle().apply {
        putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_SINGLE_ITEM, MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM)
      }
      else -> Bundle()
    }
    when (v.stringa("gruppo")) {
      "ascoltati" -> extras.putString(
        MediaConstants.EXTRAS_KEY_CONTENT_STYLE_GROUP_TITLE,
        contesto.getString(R.string.auto_gruppo_ascoltati),
      )
      "aggiunti" -> extras.putString(
        MediaConstants.EXTRAS_KEY_CONTENT_STYLE_GROUP_TITLE,
        contesto.getString(R.string.auto_gruppo_aggiunti),
      )
    }
    val copertina = v.stringa("copertina")?.let { Copertine.indirizzo(it) }
      ?: when (tipo) {
        "preferiti" -> risorsa("auto_preferiti")
        "playlist" -> risorsa("auto_playlist")
        "artista" -> risorsa("auto_artisti")
        "album" -> risorsa("auto_album")
        "tutto" -> risorsa("auto_suona")
        else -> null
      }
    val metadati = MediaMetadata.Builder()
      .setTitle(titolo)
      .setSubtitle(sottotitolo)
      .setArtist(artista)
      .setAlbumTitle(v.stringa("album"))
      // Una voce si apre o suona, mai tutte e due: vedi `Nodo::Tutto` in Rust.
      .setIsBrowsable(tipo != "brano" && tipo != "tutto")
      .setIsPlayable(tipo == "brano" || tipo == "tutto")
      .setMediaType(
        when (tipo) {
          "brano" -> MediaMetadata.MEDIA_TYPE_MUSIC
          "album" -> MediaMetadata.MEDIA_TYPE_ALBUM
          "artista" -> MediaMetadata.MEDIA_TYPE_ARTIST
          "playlist", "preferiti", "tutto" -> MediaMetadata.MEDIA_TYPE_PLAYLIST
          else -> MediaMetadata.MEDIA_TYPE_FOLDER_MIXED
        }
      )
      .apply { copertina?.let { setArtworkUri(it) } }
      .apply { if (v.has("anno") && !v.isNull("anno")) setReleaseYear(v.optInt("anno")) }
      .apply { if (v.has("durataMs") && !v.isNull("durataMs")) setDurationMs(v.optLong("durataMs")) }
      .setExtras(extras)
      .build()
    return MediaItem.Builder().setMediaId(id).setMediaMetadata(metadati).build()
  }

  private fun voci(elenco: JSONArray): List<MediaItem> =
    (0 until elenco.length()).mapNotNull { runCatching { voce(elenco.getJSONObject(it)) }.getOrNull() }

  /** Chiede a Rust, e dà a Media3 la promessa della risposta. */
  private fun chiedi(
    domanda: JSONObject,
    params: LibraryParams?,
  ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> {
    val futuro = SettableFuture.create<LibraryResult<ImmutableList<MediaItem>>>()
    val id = numeri.incrementAndGet()
    domanda.put("id", id)
    inAttesa[id] = { risposta ->
      principale.post {
        val errore = risposta.stringa("errore")
        futuro.set(
          if (errore != null) {
            LibraryResult.ofError(SessionError(SessionError.ERROR_BAD_VALUE, errore))
          } else {
            LibraryResult.ofItemList(voci(risposta.optJSONArray("voci") ?: JSONArray()), params)
          }
        )
      }
    }
    richieste.offer(domanda)
    Nucleo.sveglia(contesto)
    val entro = if (Nucleo.pronto) ATTESA_MS else ATTESA_A_FREDDO_MS
    principale.postDelayed({
      if (inAttesa.remove(id) != null) futuro.set(senzaNucleo())
    }, entro)
    return futuro
  }

  /**
   * Il nucleo non ha risposto.
   *
   * Se c'è, è una lettura andata storta: un errore e basta. Se non c'è, è
   * perché Android non ha lasciato partire l'app da sola — da Android 10 un'app
   * sullo sfondo non può aprire la sua schermata — e allora l'auto mostra un
   * pulsante che la apre: la promessa che l'auto manda al telefono quando lo
   * si tocca la può aprire.
   *
   * Il codice è `ERROR_SESSION_AUTHENTICATION_EXPIRED` non perché ci sia un
   * accesso scaduto, ma perché è uno dei due che Media3 riporta anche ai
   * controllori di vecchia generazione — l'auto lo è — come errore della
   * sessione, con il pulsante (`MediaLibrarySessionImpl.isReplicationErrorCode`).
   * Il primo risultato riuscito lo toglie da sé.
   */
  private fun senzaNucleo(): LibraryResult<ImmutableList<MediaItem>> {
    if (Nucleo.pronto) return LibraryResult.ofError(SessionError(SessionError.ERROR_IO, ""))
    val extras = Bundle().apply {
      putString(
        MediaConstants.EXTRAS_KEY_ERROR_RESOLUTION_ACTION_LABEL_COMPAT,
        contesto.getString(R.string.auto_apri_azione),
      )
      putParcelable(MediaConstants.EXTRAS_KEY_ERROR_RESOLUTION_ACTION_INTENT_COMPAT, Sessione.ritorno(contesto))
    }
    return LibraryResult.ofError(
      SessionError(SessionError.ERROR_SESSION_AUTHENTICATION_EXPIRED, contesto.getString(R.string.auto_apri), extras)
    )
  }

  companion object {
    const val RADICE = "radice"

    /** Le quattro schede, per identificativo: vedi [schede]. */
    private val SCHEDE = listOf("recenti", "playlist", "album", "artisti")

    /** Il nucleo è partito: le schede che l'auto ha già aperto si rileggono. */
    fun nucleoPronto(sessione: MediaLibrarySession) {
      sessione.clearReplicatedLibraryError()
      (listOf(RADICE) + SCHEDE).forEach { sessione.notifyChildrenChanged(it, Int.MAX_VALUE, null) }
    }

    /** La radice per la scheda di ripresa del sistema: vedi [onGetLibraryRoot]. */
    const val RECENTE = "recente-sistema"

    /** Come la radice dice all'auto che si può cercare. */
    private const val RICERCA_POSSIBILE = "android.media.browse.SEARCH_SUPPORTED"

    private const val ELENCO = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM
    private const val GRIGLIA = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM

    /** Quanti risultati di una ricerca si chiedono per primi. */
    private const val PRIMA_PAGINA = 50

    /** Quanto si aspetta il nucleo che c'è: una lettura della libreria. */
    private const val ATTESA_MS = 8_000L

    /** Quanto si aspetta il nucleo che deve ancora partire. */
    private const val ATTESA_A_FREDDO_MS = 15_000L

    private val principale = Handler(Looper.getMainLooper())
    private val richieste = LinkedBlockingQueue<JSONObject>()
    private val inAttesa = ConcurrentHashMap<Long, (JSONObject) -> Unit>()
    private val numeri = AtomicLong()

    /** La prossima domanda, aspettandola al più `entroMs`. Da un filo Rust. */
    fun prossimaRichiesta(entroMs: Long): JSONObject? = richieste.poll(entroMs, TimeUnit.MILLISECONDS)

    /** Una risposta da Rust. Una domanda che nessuno aspetta più cade qui. */
    fun risposta(a: JSONObject) {
      inAttesa.remove(a.getLong("id"))?.invoke(a)
    }
  }
}

/** Il testo di un campo, o `null` se manca o è `null`. */
internal fun JSONObject.stringa(nome: String): String? =
  if (!has(nome) || isNull(nome)) null else optString(nome)
