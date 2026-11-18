package io.github.federicobaratti.rymembra

import android.content.Context
import android.os.Build
import android.os.Environment
import android.os.Looper
import android.provider.Settings
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

  /**
   * Il nucleo è partito, in questo processo: l'attività si è aperta almeno una
   * volta, e con lei Tauri. Resta vero anche quando l'attività se ne va — il
   * nucleo no. Vedi [Nucleo].
   */
  val agganciato: Boolean get() = contesto != null

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
      "sessione.pronta" -> Nucleo.pronta(contesto(), a.getString("copertine")).let { null }
      "sessione.errore" -> Sessione.errore(contesto(), a.optString("codice")).let { null }
      "sessione.libreria" -> Sessione.nucleoPronto().let { null }

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
      "sistema.installaApk" -> Installatore.installa(attivita(), a.getString("percorso"))
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
        Presenza.imposta(contesto(), acceso, testi(a, "collegati"), dallaPersona).let { null }
      }
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
