package io.github.federicobaratti.rymembra

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.content.pm.ServiceInfo
import android.graphics.drawable.Icon
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.provider.Settings
import android.util.Log
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

/**
 * Il servizio in primo piano di «Resta attiva in sottofondo»: tiene vivo il
 * processo, e con lui il nucleo e il collegamento via internet, anche con
 * l'app tolta dalle recenti e lo schermo spento. Così i PC trovano il telefono
 * quando lo cercano, e non solo quando lo si sta usando.
 *
 * # Perché non [ServizioMusica]
 *
 * `mediaPlayback` è per «continuare la riproduzione dallo sfondo»: senza una
 * riproduzione in corso non ne ha il motivo, e Google Play chiede che quella
 * riproduzione sia avviata dall'utente. Questo servizio ha un motivo suo e un
 * tipo suo.
 *
 * # Perché `connectedDevice`
 *
 * La documentazione ufficiale lo descrive come «interazioni con dispositivi
 * esterni che richiedono una connessione Bluetooth, NFC, IR, USB o **di
 * rete**»: è quel che fa Rymembra, tenere aperto il collegamento con i PC di chi
 * la usa. Per dichiararlo basta che il manifest abbia uno dei permessi di
 * rete che il tipo richiede da Android 14 (qui `CHANGE_WIFI_MULTICAST_STATE`,
 * che «Ascolta insieme» già chiede), non ha un tempo massimo come `dataSync`, e
 * non è fra i tipi che da Android 15 un `BOOT_COMPLETED` non può avviare —
 * `dataSync`, `mediaPlayback`, `camera`, `phoneCall`, `mediaProjection`,
 * `microphone` —: serve alla fase dell'avvio all'accensione. Google Play
 * rivede ogni tipo, e per questo chiede un video del collegamento fatto dentro
 * l'app — «mostra l'utente che si collega a un dispositivo esterno», che qui è
 * il QR di «Collega un dispositivo»: che accetti un servizio che parla con PC
 * raggiunti su internet è un rischio di revisione, non una certezza.
 * `specialUse` si è tenuto come ripiego, non come scelta: è «per i casi non
 * coperti dagli altri tipi», e questo sembra coperto.
 *
 * # Cosa non garantisce
 *
 * Un servizio in primo piano non esenta dal Doze: con lo schermo spento e il
 * telefono fermo il sistema sospende la rete delle app che non sono fuori dal
 * risparmio batteria, anche con questo servizio acceso. Per questo le
 * impostazioni portano alla pagina di sistema in cui togliere il risparmio
 * (vedi [apriRisparmio]): non si chiede l'esenzione diretta, che la policy di
 * Google Play riserva alle app il cui compito si rompe senza.
 *
 * # Senza il nucleo
 *
 * `START_STICKY`: se il sistema chiude il processo, riporta il servizio. Ma il
 * nucleo Rust parte con l'attività (vedi [Nucleo]), e da Android 10 un'app
 * dietro non può aprirla: il servizio risorto da solo non ha nessuno dietro
 * di sé. Dirlo è il minimo — la notifica diventa «a riposo», e toccarla apre
 * l'app, che rimette il nucleo e la notifica com'era (vedi [titolo]). Un
 * nucleo che parte senza attività è la fase dell'avvio all'accensione.
 *
 * # Chi lo accende e lo spegne
 *
 * Il nucleo, col comando `sistema.sottofondo` (vedi `lontano/filo.rs`): a ogni
 * giro dice se è voluto e a chi si è collegati, e il servizio rifà la notifica
 * solo se cambia la frase. «Spegni» sulla notifica fa da sé quel che può — ferma
 * il servizio e ricorda la scelta — e la dice al nucleo alla prima occasione
 * ([prendiLoSpegnimentoDellaNotifica]), che la scrive nelle impostazioni.
 */
class Presenza : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == AZIONE_SPEGNI) {
      // `commit` e non `apply`: fermato il servizio il processo può essere
      // chiuso subito, e il segno per il nucleo — che lo scrive nelle
      // impostazioni — non deve restare a metà strada verso il disco.
      preferenze(this)
        .edit()
        .putBoolean(VOLUTO, false)
        .putBoolean(SPENTO_DALLA_NOTIFICA, true)
        .commit()
      ferma()
      return START_NOT_STICKY
    }
    vivo = this
    // Prima di tutto `startForeground`: un servizio avviato con
    // `startForegroundService` che non lo chiama entro pochi secondi fa
    // chiudere l'app, anche se nel frattempo nessuno lo vuole più.
    try {
      val notifica = notifica(this)
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        startForeground(ID, notifica, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
      } else {
        startForeground(ID, notifica)
      }
    } catch (e: Exception) {
      // Android 12+ non lascia andare in primo piano un servizio che parte
      // dallo sfondo (`ForegroundServiceStartNotAllowedException`). Non si
      // riprova finché l'app non torna in vista: vedi [imposta].
      Log.w(ETICHETTA, "il servizio di presenza non va in primo piano", e)
      rifiutato = true
      stopSelf()
      return START_NOT_STICKY
    }
    rifiutato = false
    // Spento prima ancora di partire, o risorto dopo uno spegnimento.
    if (!voluto(this)) {
      ferma()
      return START_NOT_STICKY
    }
    return START_STICKY
  }

  override fun onDestroy() {
    if (vivo === this) {
      vivo = null
      mostrata = null
    }
    super.onDestroy()
  }

  private fun ferma() {
    stopForeground(STOP_FOREGROUND_REMOVE)
    stopSelf()
  }

  companion object {
    private const val ETICHETTA = "Rymembra"
    private const val ID = 7433
    private const val CANALE = "presenza"

    /** Il file delle preferenze: lo legge anche chi non ha il nucleo, come `Ricordo`. */
    private const val FILE = "presenza"

    /** Il sottofondo è acceso, secondo l'ultimo ordine del nucleo. */
    private const val VOLUTO = "voluto"

    /** «Spegni» sulla notifica, e il nucleo non lo sa ancora. */
    private const val SPENTO_DALLA_NOTIFICA = "spentoDallaNotifica"

    /** Il tasto della notifica: chiede a questo stesso servizio di fermarsi. */
    private const val AZIONE_SPEGNI = "io.github.federicobaratti.rymembra.SPEGNI_PRESENZA"

    @Volatile private var vivo: Presenza? = null

    /** A chi si è collegati adesso, come lo dice il nucleo. */
    @Volatile private var collegati: List<String> = emptyList()

    /** La frase della notifica com'è adesso: per rifarla solo se cambia. */
    @Volatile private var mostrata: String? = null

    /** Android ha detto di no a un avvio dallo sfondo: si riprova con l'app in vista. */
    @Volatile private var rifiutato = false

    private fun preferenze(contesto: Context): SharedPreferences =
      contesto.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    /**
     * Il sottofondo è acceso. Lo leggono `ServizioMusica` — tolta l'app dalle
     * recenti non deve fermarsi — e il servizio stesso, senza il nucleo.
     */
    fun voluto(contesto: Context): Boolean = preferenze(contesto).getBoolean(VOLUTO, false)

    /**
     * Acceso o spento, e a chi si è collegati. Il nucleo lo dice a ogni giro:
     * se il servizio c'è basta rifare la notifica, e solo se la frase cambia.
     * Non lancia mai.
     *
     * `dallaPersona` è l'interruttore delle impostazioni, e non il giro:
     * annulla uno «Spegni» che il nucleo non ha ancora ricevuto — la scelta
     * più recente vince — e chiede le notifiche, perché è lì che chi lo accende
     * sa perché arriva la domanda. Il giro invece non rimette in piedi quel
     * che «Spegni» ha appena fermato.
     */
    fun imposta(contesto: Context, acceso: Boolean, collegati: List<String>, dallaPersona: Boolean) {
      val app = contesto.applicationContext
      val p = preferenze(app)
      if (dallaPersona) {
        p.edit().putBoolean(SPENTO_DALLA_NOTIFICA, false).apply()
      } else if (acceso && p.getBoolean(SPENTO_DALLA_NOTIFICA, false)) {
        return
      }
      p.edit().putBoolean(VOLUTO, acceso).apply()
      this.collegati = collegati
      if (acceso) {
        val servizio = vivo
        if (servizio != null) {
          aggiornaLaNotifica(app)
          return
        }
        // Dopo un no di Android, dallo sfondo non si riprova a ogni giro del
        // nucleo: si aspetta che l'app sia in vista, e allora è permesso.
        if (rifiutato && !Ponte.inVista) return
        try {
          app.startForegroundService(Intent(app, Presenza::class.java))
        } catch (e: Exception) {
          if (!rifiutato) Log.w(ETICHETTA, "il servizio di presenza non parte", e)
          rifiutato = true
        }
      } else {
        // Solo se è partito: fermarlo prima del suo `startForeground` farebbe
        // chiudere l'app. Se non è ancora partito, si ferma da sé appena parte.
        val servizio = vivo ?: return
        Handler(Looper.getMainLooper()).post { servizio.ferma() }
      }
    }

    /**
     * «Spegni» è stato premuto sulla notifica e il nucleo non lo sa: la prima
     * volta dice sì, e da lì no. Il nucleo lo scrive nelle impostazioni.
     */
    fun prendiLoSpegnimentoDellaNotifica(contesto: Context): Boolean {
      val p = preferenze(contesto)
      if (!p.getBoolean(SPENTO_DALLA_NOTIFICA, false)) return false
      p.edit().putBoolean(SPENTO_DALLA_NOTIFICA, false).apply()
      return true
    }

    /**
     * La frase della notifica. Senza il nucleo (`Ponte.agganciato` è falso: il
     * servizio è risorto da solo, e l'attività non c'è mai stata in questo
     * processo) dice la verità, che è «a riposo».
     */
    private fun titolo(contesto: Context): String {
      if (!Ponte.agganciato) return contesto.getString(R.string.presenza_titolo_riposo)
      val nomi = collegati
      return when (nomi.size) {
        0 -> contesto.getString(R.string.presenza_titolo)
        1 -> contesto.getString(R.string.presenza_titolo_uno, nomi[0])
        else -> contesto.getString(R.string.presenza_titolo_piu, nomi.size)
      }
    }

    private fun aggiornaLaNotifica(contesto: Context) {
      if (titolo(contesto) == mostrata) return
      try {
        contesto.getSystemService(NotificationManager::class.java).notify(ID, notifica(contesto))
      } catch (e: Exception) {
        Log.w(ETICHETTA, "la notifica di presenza non si aggiorna", e)
      }
    }

    private fun notifica(contesto: Context): Notification {
      val gestore = contesto.getSystemService(NotificationManager::class.java)
      gestore.createNotificationChannel(
        NotificationChannel(
          CANALE,
          contesto.getString(R.string.presenza_canale),
          NotificationManager.IMPORTANCE_LOW,
        ).apply { setShowBadge(false) },
      )
      val apri = PendingIntent.getActivity(
        contesto,
        0,
        Intent(contesto, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE,
      )
      val spegni = PendingIntent.getService(
        contesto,
        0,
        Intent(contesto, Presenza::class.java).setAction(AZIONE_SPEGNI),
        PendingIntent.FLAG_IMMUTABLE,
      )
      val frase = titolo(contesto)
      mostrata = frase
      val costruttore = Notification.Builder(contesto, CANALE)
        .setSmallIcon(R.drawable.notifica_presenza)
        .setContentTitle(frase)
        .setContentIntent(apri)
        .setOngoing(true)
        .setOnlyAlertOnce(true)
        .setCategory(Notification.CATEGORY_SERVICE)
        // Sullo schermo di blocco niente: dice a chi è collegato il telefono.
        .setVisibility(Notification.VISIBILITY_SECRET)
        .addAction(
          Notification.Action.Builder(
            Icon.createWithResource(contesto, R.drawable.notifica_presenza),
            contesto.getString(R.string.presenza_spegni),
            spegni,
          ).build(),
        )
      if (!Ponte.agganciato) {
        costruttore.setContentText(contesto.getString(R.string.presenza_testo_riposo))
      }
      return costruttore.build()
    }

    /** L'app è fuori dal risparmio batteria: la rete non si sospende col telefono fermo. */
    fun senzaRisparmio(contesto: Context): Boolean =
      contesto.getSystemService(PowerManager::class.java)
        .isIgnoringBatteryOptimizations(contesto.packageName)

    /**
     * Apre la pagina di sistema in cui si toglie il risparmio batteria a
     * Rymembra, e dice com'è andata.
     *
     * Non `ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`, e non il permesso
     * `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` che lo permette: la policy di
     * Google Play vieta la richiesta diretta a chi non rompe il proprio compito
     * senza. La pagina con l'elenco delle app la può aprire chiunque. Se un
     * telefono non ce l'ha, la scheda di Rymembra nelle impostazioni, da cui si
     * arriva alla batteria.
     *
     * Dal filo che aspetta, mai dal principale: lo si apre lì.
     */
    fun apriRisparmio(attivita: MainActivity): Boolean {
      val esito = CompletableFuture<Boolean>()
      attivita.runOnUiThread {
        val intenti = listOf(
          Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
          Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", attivita.packageName, null)),
        )
        esito.complete(
          intenti.any { intento ->
            try {
              attivita.startActivity(intento)
              true
            } catch (e: Exception) {
              Log.w(ETICHETTA, "la pagina del risparmio batteria non si apre: ${e.javaClass.simpleName}")
              false
            }
          },
        )
      }
      return esito.get(5, TimeUnit.SECONDS)
    }
  }
}
