package io.github.federicobaratti.rymembra

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.content.pm.PackageManager
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
import androidx.car.app.connection.CarConnection
import androidx.lifecycle.Observer
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
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
 * # Il nucleo parte da qui
 *
 * `START_STICKY`: se il sistema chiude il processo, riporta il servizio, e il
 * servizio **avvia il nucleo Rust** ([Ponte.avviaIlNucleo]) senza aspettare
 * un'attività, che da Android 10 un'app dietro non può aprire. Vale a ogni
 * partenza del servizio: dopo un `kill`, dopo un riavvio del telefono
 * ([AvvioAccensione]), dopo che la persona ha acceso il sottofondo con l'app
 * chiusa. Quando poi l'attività nasce, Tauri si aggancia al nucleo che c'è
 * già. Finché il nucleo non ha detto di esserci la notifica dice che si sta
 * avviando; se non parte (vedi [titolo]) dice «a riposo», con l'invito ad aprire
 * l'app — e aprirla **serve**: la libreria che non si apriva si riapre, e il
 * nucleo si rifà da zero —, o, se è caduto a metà o la libreria non si apre
 * nemmeno dall'attività, di chiudere l'app dalle impostazioni e riaprirla: un
 * nucleo a pezzi non si rifà senza ripartire, e il processo che c'è già non
 * riapre la libreria da sé.
 * Niente si riprova da solo: un avvio che fallisce sempre (un disco pieno) non
 * deve tenere il processo in un ciclo.
 *
 * # Chi lo accende e lo spegne
 *
 * Il nucleo, col comando `sistema.sottofondo` (vedi `lontano/filo.rs`): a ogni
 * giro dice se è voluto e a chi si è collegati, e il servizio rifà la notifica
 * solo se cambia la frase. «Spegni» sulla notifica fa da sé quel che può — ferma
 * il servizio e ricorda la scelta — e la dice al nucleo alla prima occasione
 * ([prendiLoSpegnimentoDellaNotifica]), che la scrive nelle impostazioni.
 *
 * # Il ricevitore dei tasti
 *
 * Il sottofondo acceso accende anche [RicevitoreTasti] (vedi [ricevitoreTasti]):
 * il play di una cuffia o dell'auto, e la scheda di ripresa del sistema, che
 * riaccendono la musica quando l'app non suona. Spento il sottofondo si spegne
 * anche lui, e torna la ragione per cui non c'era: a nucleo spento il tasto
 * farebbe cadere l'app.
 *
 * # «Parti con l'auto»
 *
 * Spenta di serie, e solo col sottofondo acceso: è questo servizio a guardare
 * `CarConnection(…).type` (Car App Library, `androidx.car.app`), e
 * `CONNECTION_TYPE_PROJECTION` è Android Auto. Quando l'auto **diventa**
 * collegata — la prima notizia è solo un punto di partenza — e c'è un brano
 * ricordato, si chiede il fuoco audio e si manda il gesto «riprendi»: la
 * stessa strada del tasto play, e la musica riparte dalla coda salvata, al
 * posto dell'ultima app usata. Il Bluetooth dell'auto non si guarda.
 *
 * Google Play, per le app multimediali delle auto, chiede di non partire da
 * sole (`MA-1`, «The app must not autoplay on startup or without user
 * initiated action to select the app or app media»): una sola costante di
 * build, `PARTENZA_CON_AUTO`, toglie la funzione dalla variante che esce là.
 * Dove è falsa nessuno osserva l'auto e la scelta non esiste.
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
        .putBoolean(PARTI_CON_AUTO, false)
        .putBoolean(SPENTO_DALLA_NOTIFICA, true)
        .commit()
      ricevitoreTasti(this, false)
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
    guardaLAuto()
    // Il nucleo, se nessuna attività è mai nata in questo processo: dopo un
    // riavvio, o un servizio risorto da un `kill`, non c'è nessuno davanti. Con
    // un'attività già nata non fa niente (lo avvia Tauri).
    Ponte.avviaIlNucleo(applicationContext)
    return START_STICKY
  }

  override fun onDestroy() {
    smettiDiGuardareLAuto()
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

  // ── «Parti con l'auto» ─────────────────────────────────────────────────────
  //
  // Tutto sul filo principale: il servizio ci vive, e `LiveData.observeForever`
  // lo pretende.

  /** Chi guarda l'auto, e a cosa: vedi [guardaLAuto]. */
  private var collegamentoAuto: CarConnection? = null
  private var osservatoreAuto: Observer<Int>? = null

  /**
   * L'auto era collegata all'ultima notizia, o `null` se non è ancora arrivata
   * nessuna. La prima notizia è un punto di partenza e non fa partire niente:
   * accendere la scelta con l'auto già collegata, o far risorgere il servizio
   * dentro l'auto, non deve far suonare il telefono senza che nessuno abbia
   * fatto niente. Parte solo chi vede l'auto **diventare** collegata.
   */
  private var autoCollegata: Boolean? = null

  /**
   * Il controllore con cui Presenza tiene in piedi la sessione della musica
   * mentre l'auto è collegata: senza di lui nessuno è collegato a
   * [ServizioMusica] (l'attività, che di solito lo fa, non c'è), la sessione
   * non esiste, e l'auto non troverebbe Rymembra anche se la musica suona.
   */
  private var sessione: ListenableFuture<MediaController>? = null

  /**
   * Guarda se l'auto si collega, se serve: sottofondo acceso, «Parti con
   * l'auto» accesa e una build che la offre. Si chiama a ogni cambio di questi,
   * ed è idempotente. Fuori da qui nessuno osserva niente: spenta la scelta, il
   * telefono non sa se è in un'auto.
   */
  private fun guardaLAuto() {
    val serve = voluto(this) && partiConAuto(this)
    if (serve && osservatoreAuto == null) {
      autoCollegata = null
      val osservatore = Observer<Int> { tipo ->
        cambioDiAuto(tipo == CarConnection.CONNECTION_TYPE_PROJECTION)
      }
      osservatoreAuto = osservatore
      collegamentoAuto = CarConnection(this).also { it.type.observeForever(osservatore) }
    } else if (!serve && osservatoreAuto != null) {
      smettiDiGuardareLAuto()
    }
  }

  private fun smettiDiGuardareLAuto() {
    osservatoreAuto?.let { collegamentoAuto?.type?.removeObserver(it) }
    osservatoreAuto = null
    collegamentoAuto = null
    autoCollegata = null
    rilasciaLaSessione()
  }

  /** `CONNECTION_TYPE_PROJECTION` è Android Auto: tutto il resto è «non collegata». */
  private fun cambioDiAuto(collegata: Boolean) {
    val prima = autoCollegata
    autoCollegata = collegata
    if (!collegata) {
      rilasciaLaSessione()
      return
    }
    if (prima == false) riprendiConLAuto()
  }

  /**
   * L'auto si è collegata: se c'è qualcosa da riprendere, riparte. Il fuoco
   * audio si chiede **prima** — se Android non lo concede (una chiamata in corso,
   * un'altra app che non lo cede, da Android 17 la restrizione sull'audio per i
   * servizi partiti dallo sfondo: vedi [Sessione.fuocoPerPartire]) non si
   * parte — e poi il gesto è lo stesso del tasto play, per la stessa strada:
   * la musica la fa Rust, e il fuoco già preso lo trova la sessione quando
   * arriva la scheda che dice «suona».
   *
   * Senza il nucleo — ancora in avvio, o non partito — non c'è niente da
   * riprendere: si scrive nel logcat e basta. Il servizio lo avvia già da sé
   * (vedi [onStartCommand]), quindi dopo qualche secondo c'è.
   */
  private fun riprendiConLAuto() {
    val perche = when {
      !Nucleo.pronto -> "il nucleo non c'è"
      Sessione.suonaOra -> "Rymembra suona già"
      Ricordo.voce(this) == null -> "non c'è un brano ricordato"
      else -> null
    }
    if (perche != null) {
      Log.i(ETICHETTA, "«Parti con l'auto» non parte: $perche")
      return
    }
    if (sessione == null) {
      sessione = MediaController.Builder(
        this,
        SessionToken(this, ComponentName(this, ServizioMusica::class.java)),
      ).buildAsync()
    }
    if (!Sessione.fuocoPerPartire(this)) {
      Log.i(ETICHETTA, "«Parti con l'auto» non parte: Android non ha concesso il fuoco audio (il perché, se c'è, sta in RymembraPausa)")
      return
    }
    Sessione.gesto("riprendi")
  }

  private fun rilasciaLaSessione() {
    sessione?.let { MediaController.releaseFuture(it) }
    sessione = null
  }

  companion object {
    private const val ETICHETTA = "Rymembra"
    private const val ID = 7433
    private const val CANALE = "presenza"

    /** Il file delle preferenze: lo legge anche chi non ha il nucleo, come `Ricordo`. */
    private const val FILE = "presenza"

    /** Il sottofondo è acceso, secondo l'ultimo ordine del nucleo. */
    private const val VOLUTO = "voluto"

    /**
     * «Parti con l'auto» è accesa, secondo l'ultimo ordine del nucleo. Vera
     * solo insieme a [VOLUTO]: lo scrive [imposta] così, e «Spegni» la porta
     * giù con lui.
     */
    private const val PARTI_CON_AUTO = "partiConAuto"

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

    /** L'ultimo valore di «Parti con l'auto» detto al servizio: lo si dice di nuovo solo se cambia. */
    @Volatile private var guardaAuto = false

    private fun preferenze(contesto: Context): SharedPreferences =
      contesto.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    /**
     * Il sottofondo è acceso. Lo leggono `ServizioMusica` — tolta l'app dalle
     * recenti non deve fermarsi — e il servizio stesso, senza il nucleo.
     */
    fun voluto(contesto: Context): Boolean = preferenze(contesto).getBoolean(VOLUTO, false)

    /**
     * «Parti con l'auto» è accesa **e** questa build la offre. La costante di
     * build `PARTENZA_CON_AUTO` è l'unico punto che la toglie: la variante per
     * Google Play, che chiede alle app per l'auto di non partire da sole, la
     * mette a falso, e da lì in giù — la scelta, l'osservazione dell'auto, la
     * riga nelle impostazioni — la funzione non esiste.
     */
    fun partiConAuto(contesto: Context): Boolean =
      BuildConfig.PARTENZA_CON_AUTO && preferenze(contesto).getBoolean(PARTI_CON_AUTO, false)

    /**
     * Acceso o spento, a chi si è collegati, e se «Parti con l'auto» è
     * accesa. Il nucleo lo dice a ogni giro: se il servizio c'è basta rifare la
     * notifica, e solo se la frase cambia. Non lancia mai.
     *
     * `dallaPersona` è l'interruttore delle impostazioni, e non il giro:
     * annulla uno «Spegni» che il nucleo non ha ancora ricevuto — la scelta
     * più recente vince — e chiede le notifiche, perché è lì che chi lo accende
     * sa perché arriva la domanda. Il giro invece non rimette in piedi quel
     * che «Spegni» ha appena fermato.
     *
     * `partiConAuto` vale solo col sottofondo acceso, qualunque cosa dica il
     * chiamante: il servizio è l'unico che guarda l'auto.
     */
    fun imposta(
      contesto: Context,
      acceso: Boolean,
      collegati: List<String>,
      dallaPersona: Boolean,
      partiConAuto: Boolean,
    ) {
      val app = contesto.applicationContext
      val p = preferenze(app)
      if (dallaPersona) {
        p.edit().putBoolean(SPENTO_DALLA_NOTIFICA, false).apply()
      } else if (acceso && p.getBoolean(SPENTO_DALLA_NOTIFICA, false)) {
        return
      }
      val guardare = acceso && partiConAuto
      p.edit().putBoolean(VOLUTO, acceso).putBoolean(PARTI_CON_AUTO, guardare).apply()
      ricevitoreTasti(app, acceso)
      this.collegati = collegati
      if (acceso) {
        val servizio = vivo
        if (servizio != null) {
          aggiornaLaNotifica(app)
          if (guardare != guardaAuto) {
            guardaAuto = guardare
            Handler(Looper.getMainLooper()).post { servizio.guardaLAuto() }
          }
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
        guardaAuto = false
        // Solo se è partito: fermarlo prima del suo `startForeground` farebbe
        // chiudere l'app. Se non è ancora partito, si ferma da sé appena parte.
        val servizio = vivo ?: return
        Handler(Looper.getMainLooper()).post { servizio.ferma() }
      }
    }

    /**
     * Accende o spegne il ricevitore dei tasti multimediali ([RicevitoreTasti]):
     * acceso solo col sottofondo, perché a nucleo spento un tasto play farebbe
     * nascere il servizio della musica senza che niente suoni, e Android
     * chiuderebbe l'app. Lo stato di serie è quello del manifesto, spento.
     *
     * Idempotente, e si chiama a ogni giro del nucleo: il sistema si cambia
     * solo se lo stato è diverso. Con `DONT_KILL_APP`: senza, il processo — e
     * con lui il nucleo — morirebbe a ogni cambio. Non lancia mai.
     */
    private fun ricevitoreTasti(contesto: Context, acceso: Boolean) {
      try {
        val gestore = contesto.packageManager
        val nome = ComponentName(contesto, RicevitoreTasti::class.java)
        val voluto =
          if (acceso) PackageManager.COMPONENT_ENABLED_STATE_ENABLED
          else PackageManager.COMPONENT_ENABLED_STATE_DEFAULT
        if (gestore.getComponentEnabledSetting(nome) == voluto) return
        gestore.setComponentEnabledSetting(nome, voluto, PackageManager.DONT_KILL_APP)
      } catch (e: Exception) {
        Log.w(ETICHETTA, "il ricevitore dei tasti non cambia stato", e)
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
     * Il nucleo, per la notifica, non c'è: non ha ancora detto di esserci, o è caduto a
     * metà. Il «rotto» sta **prima** di [Nucleo.pronto]: `sessione.pronta` parte a metà di
     * `avvia_il_nucleo` (`media::avvia`), e un panico dopo — lo scrobbling, «Ascolta
     * insieme», il collegamento via internet — lascia un nucleo rotto che «pronto» lo ha già
     * detto. Senza questa precedenza la notifica resterebbe «è attiva» su un nucleo che la
     * pagina dice non partito, e il «pronto» ancora in coda non può cancellarlo
     * ([Ponte.segnaNucleoParte]).
     */
    private fun nucleoManca(): Boolean = Ponte.nucleoRotto || !Nucleo.pronto

    /**
     * La frase della notifica, che dice la verità sul nucleo: finché non ha detto
     * di esserci («sessione.pronta», vedi [Nucleo.pronta]) si sta avviando; se
     * l'avvio da qui è fallito ([Ponte.nucleoNonParte]) è «a riposo», e toccare la
     * notifica apre l'app, che il nucleo lo rifà da sé; se è caduto a metà
     * ([Ponte.nucleoRotto]) aprire non serve, e la frase dice di chiudere l'app — anche se
     * aveva già detto di esserci: vedi [nucleoManca].
     */
    private fun titolo(contesto: Context): String {
      // Il nucleo che c'è vince su un tentativo fallito di prima: l'attività può
      // averlo avviato dopo che il servizio non c'era riuscito.
      if (nucleoManca()) {
        return contesto.getString(
          when {
            Ponte.nucleoRotto -> R.string.presenza_titolo_rotto
            Ponte.nucleoNonParte -> R.string.presenza_titolo_riposo
            else -> R.string.presenza_titolo_avvio
          },
        )
      }
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

    /**
     * Il nucleo ha cambiato stato — è partito — e la frase della notifica con lui:
     * si rifà solo se il servizio c'è e la frase è cambiata. Dal filo principale.
     */
    fun rinfresca(contesto: Context) {
      if (vivo == null) return
      aggiornaLaNotifica(contesto.applicationContext)
    }

    /**
     * Le notifiche sono state concesse mentre il servizio era già in primo piano:
     * la notifica si rifà adesso.
     *
     * Chi accende il sottofondo su un telefono nuovo vede la domanda delle
     * notifiche **dopo** che `startForeground` è già partito (`Permessi.
     * notificheAlBisogno` non aspetta la risposta): Android tiene il servizio in
     * primo piano senza mostrare niente, e dopo il sì la notifica già postata non
     * compare da sé. [aggiornaLaNotifica] non la rifà, perché la frase è la
     * stessa — è quel confronto che la teneva nascosta fino al primo cambio di
     * collegamento. Qui si salta il confronto; senza servizio vivo o senza il
     * permesso non c'è niente da rifare. Non lancia mai.
     */
    fun ripubblica(contesto: Context) {
      if (vivo == null) return
      try {
        val gestore = contesto.applicationContext.getSystemService(NotificationManager::class.java)
        if (!gestore.areNotificationsEnabled()) return
        gestore.notify(ID, notifica(contesto.applicationContext))
      } catch (e: Exception) {
        Log.w(ETICHETTA, "la notifica di presenza non si rifà", e)
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
      if (nucleoManca()) {
        when {
          Ponte.nucleoRotto -> costruttore.setContentText(contesto.getString(R.string.presenza_testo_rotto))
          Ponte.nucleoNonParte -> costruttore.setContentText(contesto.getString(R.string.presenza_testo_riposo))
        }
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
