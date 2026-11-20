package io.github.federicobaratti.rymembra

import android.content.Context
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Log
import android.view.HapticFeedbackConstants
import org.json.JSONObject
import java.lang.ref.WeakReference

/**
 * La porta da cui Rust entra in Kotlin.
 *
 * Un metodo solo, [chiama], con argomenti e risposta in JSON: il perché sta in
 * `rymembra-android/src/lib.rs`. Lo chiama `jvm.rs`, sul filo Rust che ha
 * bisogno della risposta — qualunque filo, compreso quello principale — e per
 * questo i comandi che devono aspettare una persona (un selettore, un dialogo
 * dei permessi) rifiutano di partire dal filo principale invece di aspettare se
 * stessi.
 *
 * La risposta è sempre un oggetto: `{"ok": valore}` o `{"errore": "perché"}`.
 * Nessuna eccezione esce da qui: una eccezione Java che arriva in Rust è una
 * chiamata JNI fallita, e il perché si perderebbe.
 */
/**
 * Quanto la pagina deve stare lontana dai bordi, in pixel CSS.
 *
 * `basso` è già il più alto fra la barra dei gesti e la tastiera; `tastiera`
 * dice a parte quanta ne è aperta, perché la pagina con la tastiera su toglie
 * di mezzo la barra delle schede e il mini lettore (`riquadri.ts`).
 */
data class Riquadri(
  val alto: Float,
  val destra: Float,
  val basso: Float,
  val sinistra: Float,
  val tastiera: Float = 0f,
)

object Ponte {
  /** Gli ultimi riquadri misurati da `MainActivity`. */
  @Volatile var riquadri = Riquadri(0f, 0f, 0f, 0f)

  @Volatile private var contesto: Context? = null
  @Volatile private var attivita = WeakReference<MainActivity>(null)
  private val principale = Handler(Looper.getMainLooper())

  /**
   * L'attività è in vista: fra `onStart` e `onStop`. Con la musica che suona
   * è il «telefono in uso» del collegamento via internet (`sistema.inUso`).
   */
  @Volatile var inVista = false

  /**
   * L'ultimo fondo detto dalla pagina per le barre di sistema (`sistema.barre`):
   * chiaro, scuro, o non ancora detto. Lo si tiene qui perché un'attività
   * rinata lo rimetta prima che la pagina nuova faccia in tempo a ridirlo.
   */
  @Volatile var fondoChiaro: Boolean? = null

  /** L'attività è nata: da qui in poi si può rispondere. */
  fun aggancia(attivita: MainActivity) {
    contesto = attivita.applicationContext
    this.attivita = WeakReference(attivita)
    Rete.osserva(attivita.applicationContext)
  }

  /** L'attività se ne va: i comandi che la vogliono rispondono di no. */
  fun sgancia(attivita: MainActivity) {
    if (this.attivita.get() === attivita) this.attivita = WeakReference(null)
  }

  // ── Il nucleo che parte da un servizio, prima di ogni attività ───────────────
  //
  // Le tre funzioni sono di Rust (`rymembra-android/src/jvm.rs`, esportate dal
  // crate dell'app con `esporta_il_nucleo!`). Il perché di ogni regola sta nel
  // `diario-f8.md` della prova di fattibilità, e in `jvm.rs`: qui basta dire
  // che **a decidere** se `ndk-context` lo mette il servizio è Kotlin, perché
  // quello di `tao` non si può mettere due volte e `catch_unwind` non salva.

  /** Il servizio avvia il nucleo: carica la libreria ed entra in Rust. Vedi [avviaIlNucleo]. */
  @JvmStatic private external fun avviaNucleo(contesto: Context, dati: String, inizializzaContesto: Boolean)

  /** Prima della prima attività: Rust toglie da `ndk-context` il contesto del servizio, se c'è. */
  @JvmStatic private external fun rilasciaIlContestoDelServizio()

  /** Dopo `super.onCreate` della prima attività: `tao` ha messo il suo contesto, Rust lo può leggere. */
  @JvmStatic private external fun contestoDiTaoPronto()

  /**
   * Un'attività è già nata in questo processo: `tao` ha già messo `ndk-context`
   * (per sempre: il suo `OnceLock`), il servizio non deve toccarlo, e il nucleo
   * lo avvierà il `setup` di Tauri. Si legge e si scrive solo sul filo
   * principale, dove girano `MainActivity.onCreate` e `Presenza.onStartCommand`.
   */
  private var attivitaGiaNata = false

  /**
   * Il servizio ha già fatto entrare Rust per questo tentativo: finché non dice «non parte» e
   * riprovabile ([segnaNucleoNonParte]) non si rientra. Filo principale.
   */
  private var nucleoAvviatoDalServizio = false

  /**
   * La libreria nativa è stata caricata dal servizio, e Rust **può** aver messo
   * `ndk-context`: si alza prima della chiamata che lo mette, e non si abbassa. È
   * questo, e non la riuscita dell'avvio, a decidere se l'attività deve rilasciare
   * il contesto: se la chiamata fallisce dopo che Rust l'ha messo (il riferimento
   * globale della classe, il filo che non parte), il contesto del servizio resta, e
   * una prima attività che non lo rilasciasse abortirebbe il processo in
   * `Rust.onCreate`. Il rilascio, in Rust, non fa niente se non c'è niente da
   * rilasciare. Filo principale.
   */
  private var nativaDelServizio = false

  /** L'avvio dal servizio è fallito: il nucleo non c'è, e lo dice la notifica. */
  @Volatile var nucleoNonParte = false
    private set

  /**
   * Il nucleo è caduto **a metà** (un panico dopo che la libreria era aperta): ha fili
   * vivi e stati registrati, riprovare lo raddoppierebbe, e non si rifà senza chiudere il
   * processo. La notifica lo dice, e dice cosa fare; la pagina non monta (`nonParte` in
   * `libreria_fase`). Se è falso e [nucleoNonParte] è vero, l'avvio non ha lasciato niente
   * e si può rifare.
   *
   * **Non guarisce**: una volta alzato non si abbassa per tutta la vita del processo — né un
   * [segnaNucleoParte] né un [segnaNucleoNonParte] riprovabile arrivati dopo lo toccano. Il
   * «pronto» può essere arrivato **prima** (`sessione.pronta` parte a metà di
   * `avvia_il_nucleo`, da `media::avvia`, e un panico dopo dà un nucleo rotto che ha già
   * detto di esserci), e [Presenza] lo guarda prima di [Nucleo.pronto].
   */
  @Volatile var nucleoRotto = false
    private set

  /**
   * Rust dice che il nucleo non è partito (`nucleo.nonParte`).
   *
   * `riprovabile`: la libreria non si è aperta e l'avvio non ha lasciato niente di vivo.
   * Allora il segno «già avviato dal servizio» si toglie, perché il tentativo seguente
   * — una sveglia ([Nucleo.sveglia]), che non è più di una ogni venti secondi — lo
   * rilanci davvero: senza, il guasto di un avvio durava quanto il processo. Il
   * segno è del filo principale, e sul filo principale si toglie: dopo che
   * [avviaIlNucleo], che lo ha appena alzato, è tornata.
   *
   * Niente si riprova da solo: l'avvio si rifà quando qualcuno lo chiede (l'attività che
   * nasce, o una sveglia), e un avvio che fallisce sempre — un disco pieno — non tiene
   * il processo in un ciclo.
   */
  fun segnaNucleoNonParte(riprovabile: Boolean) {
    // Un nucleo già rotto resta rotto: un «non parte» riprovabile che arriva dopo non lo
    // declassa, né rimette in gioco il segno «già avviato». Arriva dopo davvero: la guardia del
    // servizio chiude l'avvio **prima** di avvisare (`GuardiaDellAvvio`), e in quel
    // frattempo la finestra, che aspettava la fine dell'avvio, può aver già detto «rotto»
    // (`avvisa_se_la_libreria_manca`, `rifai_l_avvio`) prima che il «fallito» del servizio
    // raggiunga Kotlin.
    if (nucleoRotto) return
    nucleoRotto = !riprovabile
    nucleoNonParte = true
    if (riprovabile) principale.post { nucleoAvviatoDalServizio = false }
  }

  /**
   * Il nucleo c'è ([Nucleo.pronta]): niente più «non parte» di un tentativo fallito. Un
   * nucleo **rotto** no: ha detto «pronto» a metà del suo avvio e poi è caduto, e il
   * «pronto» ancora in coda sul filo principale non deve cancellare quel che la caduta ha
   * già detto ([nucleoRotto]).
   */
  fun segnaNucleoParte() {
    if (nucleoRotto) return
    nucleoNonParte = false
  }

  /**
   * Avvia il nucleo Rust da un servizio, senza attività: la libreria nativa, il
   * contesto, la libreria dei brani, il lettore, la sessione e il collegamento
   * con gli altri dispositivi (`lib.rs`, `avvia_il_nucleo`).
   *
   * Solo dal filo principale, e solo se **nessuna attività è mai nata** in
   * questo processo: se è nata, il nucleo lo avvia o l'ha già avviato Tauri.
   * Non lancia mai: un guasto si scrive nel registro e nella notifica.
   */
  fun avviaIlNucleo(applicazione: Context) {
    check(Looper.myLooper() == Looper.getMainLooper()) { "solo dal filo principale: il flag è suo" }
    if (attivitaGiaNata || nucleoAvviatoDalServizio) return
    try {
      System.loadLibrary("rymembra_lib")
      nativaDelServizio = true
      // Come farebbe l'attività, ma col contesto dell'applicazione: i comandi che
      // non vogliono un'attività (`sistema.cartellaCache`, la sessione, i segreti)
      // rispondono già.
      if (contesto == null) contesto = applicazione
      Rete.osserva(applicazione)
      // Un tentativo nuovo: il segno di quello di prima si toglie **prima** di entrare in
      // Rust, che può dire di nuovo «non parte» da un filo suo prima che la chiamata torni
      // (e un segno tolto dopo cancellerebbe quel «non parte»).
      nucleoNonParte = false
      nucleoRotto = false
      avviaNucleo(applicazione, applicazione.dataDir.absolutePath, true)
      nucleoAvviatoDalServizio = true
    } catch (e: Throwable) {
      nucleoNonParte = true
      Log.e("Rymembra", "il nucleo non parte dal servizio", e)
    }
  }

  /**
   * Dall'attività, prima di `super.onCreate`: se è la prima del processo e un
   * servizio ha messo il suo `ndk-context`, lo si toglie, perché `tao` in
   * `super.onCreate` asserisce che sia vuoto. Da qui `tao` ha il contesto, per
   * sempre.
   */
  fun preparaLAttivita() {
    if (attivitaGiaNata) return
    attivitaGiaNata = true
    if (!nativaDelServizio) return
    rilasciaIlContestoDelServizio()
  }

  /** Dall'attività, dopo `super.onCreate`: se il contesto era stato rilasciato, `tao` lo ha rimesso. */
  fun attivitaNata() {
    if (nativaDelServizio) contestoDiTaoPronto()
  }

  private fun contesto(): Context = contesto ?: error("il ponte non è ancora agganciato")

  private fun attivita(): MainActivity = attivita.get() ?: error("nessuna attività aperta")

  /**
   * L'attività, per un lavoro che aspetta una persona. Mai dal filo principale:
   * il selettore si apre lì, e chi lo aspettasse da lì aspetterebbe se stesso.
   */
  private fun attivitaPerAspettare(): MainActivity {
    check(Looper.myLooper() != Looper.getMainLooper()) {
      "non dal filo principale: aspetterebbe se stesso"
    }
    return attivita()
  }

  /**
   * Come si chiama questo telefono per chi lo cerca: il «Nome del dispositivo»
   * delle impostazioni di Android, quello che vedono anche il Bluetooth e
   * l'hotspot; se manca, il modello.
   */
  private fun nomeDelTelefono(): String =
    Settings.Global.getString(contesto().contentResolver, Settings.Global.DEVICE_NAME)
      ?.trim()?.takeIf { it.isNotEmpty() }
      ?: Build.MODEL

  /**
   * Gli avvisi legali che viaggiano nell'APK, in `assets/legale/`: li mette lì
   * il compito Gradle `avvisiLegali` (`build.gradle.kts`). Un elenco chiuso:
   * il nome arriva dalla pagina, e un percorso qualunque negli asset non è
   * affar suo.
   */
  private val AVVISI_LEGALI = setOf(
    "LICENSE.txt",
    "THIRD-PARTY-NOTICES.md",
    "PRIVACY.md",
    "TERMS.md",
    "font-OFL.txt",
  )

  /**
   * Il testo di un avviso legale, per leggerlo senza rete (D-10d): `null` se
   * l'APK non lo porta, e allora la pagina apre il sito.
   */
  private fun avvisoLegale(nome: String): String? {
    require(nome in AVVISI_LEGALI) { "«$nome» non è un avviso legale dell'app" }
    return try {
      contesto().assets.open("legale/$nome").bufferedReader(Charsets.UTF_8).use { it.readText() }
    } catch (_: java.io.FileNotFoundException) {
      null
    }
  }

  private fun testi(a: JSONObject, nome: String): List<String> {
    val elenco = a.optJSONArray(nome) ?: return emptyList()
    return (0 until elenco.length()).map { elenco.getString(it) }
  }

  @JvmStatic
  fun chiama(comando: String, argomenti: String): String =
    try {
      val a = if (argomenti.isBlank() || argomenti == "null") JSONObject() else JSONObject(argomenti)
      JSONObject().put("ok", esegui(comando, a) ?: JSONObject.NULL).toString()
    } catch (t: Throwable) {
      JSONObject().put("errore", t.message ?: t.toString()).toString()
    }

  private fun esegui(comando: String, a: JSONObject): Any? =
    when (comando) {
      "segreto.leggi" -> Segreti.leggi(contesto(), a.getString("chiave"))
      "segreto.scrivi" -> Segreti.scrivi(contesto(), a.getString("chiave"), a.getString("valore")).let { null }
      "segreto.cancella" -> Segreti.cancella(contesto(), a.getString("chiave")).let { null }

      "sessione.aggiorna" -> Sessione.aggiorna(contesto(), Scheda.da(a)).let { null }
      "sessione.stacca" -> Sessione.stacca(contesto()).let { null }
      "sessione.gesto" -> Sessione.prossimoGesto(a.optLong("entroMs", 5_000))
      "sessione.coda" -> Sessione.coda(contesto(), VoceCoda.elenco(a)).let { null }
      "sessione.pronta" ->
        Nucleo.pronta(contesto(), a.getString("copertine"), a.optString("lontane", "").ifEmpty { null }).let { null }
      "sessione.errore" -> Sessione.errore(contesto(), a.optString("codice")).let { null }
      "sessione.libreria" -> Sessione.nucleoPronto().let { null }
      // Il nucleo avviato dal servizio non è partito (la libreria non si apre, o è
      // caduto): la notifica del sottofondo non dice «si sta avviando» per sempre.
      "nucleo.nonParte" -> {
        segnaNucleoNonParte(a.optBoolean("riprovabile", true))
        val app = contesto()
        principale.post { Presenza.rinfresca(app) }
        null
      }

      "auto.richiesta" -> Biblioteca.prossimaRichiesta(a.optLong("entroMs", 5_000))
      "auto.risposta" -> Biblioteca.risposta(a).let { null }

      "sistema.permessi" -> Permessi.stato(contesto())
      "sistema.chiediPermessi" -> Permessi.chiedi(attivitaPerAspettare())
      "sistema.chiediTuttiIFile" -> Permessi.chiediTuttiIFile(attivita()).let { null }
      "sistema.cartellaMusica" ->
        Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MUSIC).absolutePath
      "sistema.scegliCartella" -> Documenti.scegliCartella(attivitaPerAspettare())
      "sistema.apriDocumento" -> {
        val tipi = a.optJSONArray("tipi")
        val elenco = (0 until (tipi?.length() ?: 0)).map { tipi!!.getString(it) }
        Documenti.apri(attivitaPerAspettare(), elenco)
      }
      "sistema.salvaDocumento" ->
        Documenti.salva(attivitaPerAspettare(), a.getString("nome"), a.getString("tipo"), a.getString("sorgente"))
      "sistema.cartellaCache" -> contesto().cacheDir.absolutePath
      "sistema.condividi" ->
        Condividi.file(attivita(), a.getString("percorso"), a.getString("tipo"), a.getString("titolo")).let { null }
      "sistema.distribuzione" -> BuildConfig.DISTRIBUZIONE
      // Il contesto e non l'attività: «Verifico» finisce quando il download finisce,
      // e chi ha toccato «Aggiorna» può già essere in un'altra app, col servizio che
      // tiene vivo il processo e nessuna attività. Solo «Installa» ne ha bisogno.
      "sistema.controllaApk" -> Installatore.controlla(contesto(), a.getString("percorso"))
      "sistema.installaApk" -> Installatore.installa(attivita(), a.getString("percorso"))
      // La notifica «Pronto da installare»: col contesto, perché l'APK è pronto
      // anche con l'app in tasca. Non chiede le notifiche (vedi `AggiornamentoPronto`).
      "sistema.aggiornamentoPronto" ->
        AggiornamentoPronto.imposta(contesto(), a.getBoolean("acceso"), a.optString("versione")).let { null }
      "sistema.firmatari" -> Installatore.firmatari(contesto())
      "sistema.apriIndirizzo" -> Browser.apri(attivita(), a.getString("indirizzo")).let { null }
      "legale.leggi" -> avvisoLegale(a.getString("nome"))
      "sistema.attesaConsenso" -> {
        val acceso = a.getBoolean("acceso")
        // Prima di aprire il browser, che coprirebbe il dialogo: vedi
        // `Permessi.notificheAlBisogno`.
        if (acceso) Permessi.notificheAlBisogno(attivita.get(), aspetta = true)
        ConsensoInAttesa.imposta(contesto(), acceso).let { null }
      }
      "sistema.scarico" -> {
        val acceso = a.getBoolean("acceso")
        if (acceso) Permessi.notificheAlBisogno(attivita.get(), aspetta = false)
        ScaricoInCorso.imposta(
          contesto(),
          acceso,
          a.getInt("numero"),
          a.getInt("di"),
          a.getString("titolo"),
        ).let { null }
      }
      "sistema.sottofondo" -> {
        val acceso = a.getBoolean("acceso")
        val dallaPersona = a.optBoolean("dallaPersona", false)
        // Le notifiche si chiedono quando chi accende l'interruttore sa perché
        // arriva la domanda, e non a ogni giro del nucleo.
        if (acceso && dallaPersona) Permessi.notificheAlBisogno(attivita.get(), aspetta = false)
        Presenza.imposta(
          contesto(),
          acceso,
          testi(a, "collegati"),
          dallaPersona,
          a.optBoolean("partiConAuto", false),
        ).let { null }
      }
      // Questa build offre «Parti con l'auto»? La costante di build che la
      // toglie dalla variante per Google Play: la finestra non disegna la riga.
      "sistema.partenzaConAuto" -> BuildConfig.PARTENZA_CON_AUTO
      "sistema.sottofondoSpento" -> Presenza.prendiLoSpegnimentoDellaNotifica(contesto())
      "sistema.senzaRisparmio" -> Presenza.senzaRisparmio(contesto())
      "sistema.apriRisparmio" -> {
        check(Presenza.apriRisparmio(attivitaPerAspettare())) {
          "nessuna pagina del risparmio batteria si apre"
        }
        null
      }
      "sistema.cestinabili" -> org.json.JSONArray(Cestino.cestinabili(contesto(), testi(a, "percorsi")))
      "sistema.cestina" -> Cestino.cestina(attivitaPerAspettare(), testi(a, "percorsi"))
      "sistema.multicast" -> Rete.multicast(contesto(), a.getBoolean("acceso")).let { null }
      "sistema.wifi" -> Rete.wifi(contesto(), a.getString("chi"), a.getBoolean("acceso")).let { null }
      "sistema.uscitaAudio" -> Uscita.adesso(contesto())
      "sistema.reteLocale" -> Permessi.reteLocale(contesto())
      "sistema.chiediReteLocale" -> Permessi.chiediReteLocale(attivitaPerAspettare())
      "sistema.nomeDelTelefono" -> nomeDelTelefono()
      "sistema.versione" -> JSONObject()
        .put("android", Build.VERSION.RELEASE)
        .put("api", Build.VERSION.SDK_INT)
        .put("modello", Build.MODEL)
      "sistema.riquadri" -> riquadri.let {
        JSONObject()
          .put("alto", it.alto.toDouble())
          .put("destra", it.destra.toDouble())
          .put("basso", it.basso.toDouble())
          .put("sinistra", it.sinistra.toDouble())
          .put("tastiera", it.tastiera.toDouble())
      }
      "sistema.attivitaAperta" -> attivita.get()?.let { !it.isFinishing && !it.isDestroyed } ?: false
      "sistema.inUso" -> inVista || Sessione.suonaOra
      "sistema.scansionaQr" -> ScansioneQr.scansiona(attivitaPerAspettare())
      "sistema.reteAConsumo" -> Rete.aConsumo(contesto())
      "sistema.vibra" -> {
        // La vibrazione del sistema, non un motore acceso a mano: segue
        // l'impostazione «Feedback al tocco» del telefono, non chiede nessun
        // permesso, e ha la forma che il telefono dà a quel gesto — un tocco
        // lungo vibra come un tocco lungo in ogni altra app.
        val costante = when (a.optString("tipo")) {
          "tenuta" -> HapticFeedbackConstants.LONG_PRESS
          "conferma" ->
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) HapticFeedbackConstants.CONFIRM
            else HapticFeedbackConstants.VIRTUAL_KEY
          else -> HapticFeedbackConstants.CLOCK_TICK
        }
        val att = attivita()
        att.runOnUiThread { att.window.decorView.performHapticFeedback(costante) }
        null
      }
      "sistema.barre" -> {
        // L'ora, la batteria e la barra dei gesti sul fondo dell'app, non su
        // quello del telefono: `enableEdgeToEdge` le sceglie sul tema del
        // sistema, e un'app scura su un telefono chiaro le aveva nere su nero.
        val chiaro = a.getBoolean("chiaro")
        fondoChiaro = chiaro
        val att = attivita()
        att.runOnUiThread { att.coloraLeBarre(chiaro) }
        null
      }
      "sistema.secondoPiano" -> {
        val att = attivita()
        att.runOnUiThread { att.moveTaskToBack(true) }
        null
      }

      else -> throw IllegalArgumentException("comando sconosciuto: $comando")
    }
}
