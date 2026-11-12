package io.github.federicobaratti.rymembra

import android.app.SearchManager
import android.content.ComponentName
import android.content.Intent
import android.os.Bundle
import android.provider.MediaStore
import android.util.Log
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.ActivityResult
import androidx.activity.result.IntentSenderRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

/**
 * L'attività: la finestra di Rymembra sul telefono.
 *
 * Oltre a quel che fa `TauriActivity` — la WebView, il nucleo Rust — tiene i
 * due lanciatori con cui il ponte apre i selettori e il dialogo dei permessi,
 * e si collega al [ServizioMusica], che senza un controllore collegato non
 * partirebbe.
 */
class MainActivity : TauriActivity() {
  private var risultatoInAttesa: CompletableFuture<ActivityResult>? = null
  private val lanciatore =
    registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { risultato ->
      risultatoInAttesa?.complete(risultato)
      risultatoInAttesa = null
    }

  private var permessiInAttesa: CompletableFuture<Map<String, Boolean>>? = null
  private val lanciatorePermessi =
    registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { risultato ->
      permessiInAttesa?.complete(risultato)
      permessiInAttesa = null
    }

  private var mittenteInAttesa: CompletableFuture<ActivityResult>? = null
  private val lanciatoreMittente =
    registerForActivityResult(ActivityResultContracts.StartIntentSenderForResult()) { risultato ->
      mittenteInAttesa?.complete(risultato)
      mittenteInAttesa = null
    }

  private var controllore: ListenableFuture<MediaController>? = null

  /**
   * Il tasto Indietro lo decide la pagina, non la cronologia della WebView:
   * Rymembra è una pagina sola, e un `goBack` la ricaricherebbe da capo.
   */
  override val handleBackNavigation: Boolean = false

  override fun onWebViewCreate(webView: WebView) {
    // Quel che la pagina non usa, spento: un indirizzo `content://` nella
    // pagina (le copertine passano da uno schema nostro, non da lì), la
    // posizione, e le finestre aperte da uno script. Uno script che girasse
    // nella pagina — metadati, testi, una skin — non trova queste porte aperte;
    // i permessi che chiede (fotocamera, microfono) li nega Rust, con il
    // gestore della finestra (`telefono::crea_la_finestra`). Qui gira dopo le
    // impostazioni di wry, che quindi non le rimette.
    webView.settings.allowContentAccess = false
    webView.settings.setGeolocationEnabled(false)
    webView.settings.javaScriptCanOpenWindowsAutomatically = false

    // Le barre di sistema, il ritaglio della fotocamera e la tastiera: la
    // pagina disegna fin sotto di loro (edge-to-edge) e deve sapere quanto
    // stare lontana dai bordi. `env(safe-area-inset-*)` nella WebView non lo
    // dice su tutte le versioni: lo si misura qui, lo si tiene nel ponte, e la
    // pagina lo rilegge quando le si dice che è cambiato (`riquadri.ts`).
    ViewCompat.setOnApplyWindowInsetsListener(webView) { vista, riquadri ->
      val barre = riquadri.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      )
      val tastiera = riquadri.getInsets(WindowInsetsCompat.Type.ime())
      val densita = resources.displayMetrics.density
      Ponte.riquadri = Riquadri(
        alto = barre.top / densita,
        destra = barre.right / densita,
        basso = maxOf(barre.bottom, tastiera.bottom) / densita,
        sinistra = barre.left / densita,
        tastiera = tastiera.bottom / densita,
      )
      vista.post { webView.evaluateJavascript(RIQUADRI, null) }
      // E anche alla WebView, come se l'ascoltatore non ci fosse: `env()`
      // dove funziona continua a funzionare.
      ViewCompat.onApplyWindowInsets(vista, riquadri)
    }

    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        // La pagina chiude quel che ha aperto — un pannello, un menu, una
        // vista dentro un'altra — e risponde `true`; dalla prima schermata
        // risponde `false`, e l'app va sullo sfondo come col tasto Home. Mai
        // chiusa: la musica continua, e la si ritrova dalla notifica.
        webView.evaluateJavascript(INDIETRO) { risposta ->
          if (risposta != "true") moveTaskToBack(true)
        }
      }
    })
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    // Prima di chiunque lo legga: `WryActivity` ne legge un extra già in
    // `super.onCreate`. Vedi [leggibile].
    intent?.let { setIntent(leggibile(it)) }
    // Il ponte **prima** di `super.onCreate`: è lì che parte il nucleo Rust, e
    // il nucleo può chiedere un segreto già nel suo `setup`.
    Ponte.aggancia(this)
    enableEdgeToEdge()
    // Un'attività rinata — dopo uno scorrimento via dalle recenti, con la
    // musica che continua — riprende il fondo che la pagina aveva detto, invece
    // di quello del tema del telefono, finché la pagina nuova non lo ridice.
    Ponte.fondoChiaro?.let { coloraLeBarre(it) }
    super.onCreate(savedInstanceState)
    controllore = MediaController.Builder(this, SessionToken(this, ComponentName(this, ServizioMusica::class.java)))
      .buildAsync()
    // L'intento con cui l'attività è nata si esegue una volta sola. Non quando
    // rinasce dallo stato salvato — il processo morto e l'attività riaperta
    // dalle Recenti, che Android ricrea con l'intento di allora — e non quando
    // la si riapre dalle Recenti senza stato (dopo un riavvio), che Android
    // rilancia con lo stesso intento e `FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY`.
    // Altrimenti «metti i Queen» di giorni prima rimpiazzerebbe la coda, e un
    // invito a collegarsi già scaduto tornerebbe a chiedere.
    if (savedInstanceState == null && !dalleRecenti(intent)) {
      // Aperta per svegliare il nucleo, perché l'ha chiesto l'auto con l'app
      // chiusa (vedi `Nucleo`): il nucleo parte in `super.onCreate`, e la
      // schermata torna dietro prima di farsi vedere.
      if (intent?.getBooleanExtra(Nucleo.SFONDO, false) == true) {
        moveTaskToBack(true)
      }
      cercaDaIntento(intent)
      collegaDaIntento(intent)
    }
  }

  override fun onNewIntent(intent: Intent) {
    val sano = leggibile(intent)
    super.onNewIntent(sano)
    if (dalleRecenti(sano)) return
    cercaDaIntento(sano)
    collegaDaIntento(sano)
  }

  /**
   * L'intento, se i suoi extra si leggono tutti; se no lo stesso intento senza
   * extra. L'attività è esportata, e un'altra app può metterle negli extra un
   * `Parcelable` di una classe che c'è solo nel suo APK: fino ad Android 12 il
   * primo `get…Extra` apre tutto il pacchetto, e la `BadParcelableException`
   * farebbe cadere Rymembra — e con lei la musica e il nucleo. Da Android 13
   * si apre una voce alla volta e il guasto resta su quella: per questo qui le
   * si leggono tutte, una per una.
   */
  @Suppress("DEPRECATION")
  private fun leggibile(intento: Intent): Intent =
    try {
      intento.extras?.let { extra -> extra.keySet().forEach { extra.get(it) } }
      intento
    } catch (e: RuntimeException) {
      Log.w("Rymembra", "un intento con extra illeggibili: li si ignora (${e.javaClass.simpleName})")
      Intent(intento).apply { replaceExtras(null as Bundle?) }
    }

  /** Riaperta dalle Recenti: l'intento è quello di allora, già eseguito. */
  private fun dalleRecenti(intento: Intent?): Boolean =
    intento != null && (intento.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) != 0

  override fun onStart() {
    super.onStart()
    Ponte.inVista = true
  }

  override fun onStop() {
    Ponte.inVista = false
    super.onStop()
  }

  /**
   * Un QR di Rymembra letto con la fotocamera del sistema apre questo link:
   * lo si passa a Rust, e la finestra chiede se collegarsi. Solo lo schema e
   * l'host giusti; il resto lo controlla Rust.
   */
  private fun collegaDaIntento(intento: Intent?) {
    if (intento?.action != Intent.ACTION_VIEW) return
    val link = intento.data ?: return
    if (link.scheme != "rymembra" || link.host != "collega") return
    Sessione.collega(link.toString())
  }

  /**
   * «Metti i Queen su Rymembra», detto all'assistente fuori dall'auto: arriva
   * come intento all'attività, e diventa la stessa ricerca a voce che arriva
   * dall'auto per la sessione.
   */
  private fun cercaDaIntento(intento: Intent?) {
    if (intento?.action != MediaStore.INTENT_ACTION_MEDIA_PLAY_FROM_SEARCH) return
    Sessione.cerca(intento.getStringExtra(SearchManager.QUERY) ?: "", intento.extras)
  }

  override fun onDestroy() {
    controllore?.let { MediaController.releaseFuture(it) }
    controllore = null
    Ponte.sgancia(this)
    super.onDestroy()
  }

  /**
   * Le icone delle barre di sistema su un fondo `chiaro` (scure) o scuro
   * (chiare). Dal filo principale.
   */
  fun coloraLeBarre(chiaro: Boolean) {
    val controllo = WindowCompat.getInsetsController(window, window.decorView)
    controllo.isAppearanceLightStatusBars = chiaro
    controllo.isAppearanceLightNavigationBars = chiaro
  }

  /**
   * Lancia un'attività di sistema — un selettore — e ne aspetta il risultato.
   * Da un filo che non sia il principale: è lì che il risultato arriva.
   */
  fun aspettaRisultato(intento: Intent): ActivityResult {
    val futuro = CompletableFuture<ActivityResult>()
    runOnUiThread {
      if (risultatoInAttesa != null) {
        futuro.completeExceptionally(IllegalStateException("c'è già una scelta aperta"))
        return@runOnUiThread
      }
      risultatoInAttesa = futuro
      try {
        lanciatore.launch(intento)
      } catch (e: Exception) {
        risultatoInAttesa = null
        futuro.completeExceptionally(e)
      }
    }
    return futuro.get(ATTESA_MINUTI, TimeUnit.MINUTES)
  }

  /**
   * Lancia la richiesta di un'altra app di sistema — il Cestino di
   * `MediaStore` — e ne aspetta la risposta. Da un filo che non sia il
   * principale, come [aspettaRisultato].
   */
  fun aspettaIntentSender(richiesta: IntentSenderRequest): ActivityResult {
    val futuro = CompletableFuture<ActivityResult>()
    runOnUiThread {
      if (mittenteInAttesa != null) {
        futuro.completeExceptionally(IllegalStateException("c'è già una richiesta aperta"))
        return@runOnUiThread
      }
      mittenteInAttesa = futuro
      try {
        lanciatoreMittente.launch(richiesta)
      } catch (e: Exception) {
        mittenteInAttesa = null
        futuro.completeExceptionally(e)
      }
    }
    return futuro.get(ATTESA_MINUTI, TimeUnit.MINUTES)
  }

  /** Chiede dei permessi con il dialogo di sistema e ne aspetta la risposta. */
  fun aspettaPermessi(permessi: Array<String>): Map<String, Boolean> {
    val futuro = CompletableFuture<Map<String, Boolean>>()
    runOnUiThread {
      if (permessiInAttesa != null) {
        futuro.completeExceptionally(IllegalStateException("c'è già una richiesta di permessi aperta"))
        return@runOnUiThread
      }
      permessiInAttesa = futuro
      try {
        lanciatorePermessi.launch(permessi)
      } catch (e: Exception) {
        permessiInAttesa = null
        futuro.completeExceptionally(e)
      }
    }
    return futuro.get(ATTESA_MINUTI, TimeUnit.MINUTES)
  }

  companion object {
    /** Quanto si aspetta una persona davanti a un selettore, prima di rinunciare. */
    private const val ATTESA_MINUTI = 10L

    /** Il segnale alla pagina che i riquadri sono cambiati: vedi `riquadri.ts`. */
    private const val RIQUADRI = "window.dispatchEvent(new Event('rymembra:riquadri'))"

    /** La domanda alla pagina: vedi `indietro.ts`. */
    private const val INDIETRO = "(typeof window.rymembraIndietro === 'function' && window.rymembraIndietro()) === true"
  }
}
