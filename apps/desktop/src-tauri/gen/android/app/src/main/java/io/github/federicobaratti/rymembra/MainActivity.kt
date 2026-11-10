package io.github.federicobaratti.rymembra

import android.app.SearchManager
import android.content.ComponentName
import android.content.Intent
import android.os.Bundle
import android.provider.MediaStore
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
    // Aperta per svegliare il nucleo, perché l'ha chiesto l'auto con l'app
    // chiusa (vedi `Nucleo`): il nucleo parte in `super.onCreate`, e la
    // schermata torna dietro prima di farsi vedere.
    if (savedInstanceState == null && intent?.getBooleanExtra(Nucleo.SFONDO, false) == true) {
      moveTaskToBack(true)
    }
    cercaDaIntento(intent)
    collegaDaIntento(intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    cercaDaIntento(intent)
    collegaDaIntento(intent)
  }

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
    // Una volta sola: un'attività ricreata non lo ripete.
    intento.action = null
  }

  /**
   * «Metti i Queen su Rymembra», detto all'assistente fuori dall'auto: arriva
   * come intento all'attività, e diventa la stessa ricerca a voce che arriva
   * dall'auto per la sessione.
   */
  private fun cercaDaIntento(intento: Intent?) {
    if (intento?.action != MediaStore.INTENT_ACTION_MEDIA_PLAY_FROM_SEARCH) return
    Sessione.cerca(intento.getStringExtra(SearchManager.QUERY) ?: "", intento.extras)
    // Una volta sola: un'attività ricreata — una rotazione — non la ripete.
    intento.action = null
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
