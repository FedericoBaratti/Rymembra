package io.github.federicobaratti.rymembra

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Log

/**
 * Un servizio in primo piano, breve, mentre Rymembra aspetta nel browser il
 * consenso di un servizio da collegare (OneDrive, Dropbox, Google Drive).
 *
 * Il consenso torna a Rymembra su un indirizzo di questo telefono
 * (`127.0.0.1`), che il nucleo ascolta. Con l'app dietro al browser Android di
 * solito la lascia vivere, ma sotto pressione di memoria — o con le regole più
 * strette di alcuni produttori — la può congelare o chiudere, e allora il
 * browser non trova nessuno a cui riportare il consenso. Un servizio in primo
 * piano lo impedisce, e la notifica dice perché c'è.
 *
 * `shortService` da Android 14: nessun permesso in più, e al più tre minuti,
 * che sono anche il tempo che il nucleo aspetta (`oauth::ATTESA_CONSENSO`).
 * Prima di Android 14 un servizio in primo piano non ha bisogno di un tipo.
 *
 * Lo accende e lo spegne il nucleo, col comando `sistema.attesaConsenso`
 * (vedi `browser::in_attesa_del_consenso`): prima di aprire il browser, e al
 * ritorno, all'«Annulla» o alla scadenza.
 */
class ConsensoInAttesa : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    vivo = this
    // Prima di tutto `startForeground`: un servizio avviato con
    // `startForegroundService` che non lo chiama entro pochi secondi fa
    // chiudere l'app, anche se nel frattempo nessuno lo vuole più.
    try {
      val notifica = notifica(this)
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
        startForeground(ID, notifica, ServiceInfo.FOREGROUND_SERVICE_TYPE_SHORT_SERVICE)
      } else {
        startForeground(ID, notifica)
      }
    } catch (e: Exception) {
      Log.w(ETICHETTA, "il servizio del consenso non va in primo piano", e)
      stopSelf()
      return START_NOT_STICKY
    }
    // Spento prima ancora di partire: il consenso è già tornato.
    if (!voluto) ferma()
    return START_NOT_STICKY
  }

  /** Android 14: i tre minuti sono finiti. Senza fermarsi, il sistema chiude l'app. */
  override fun onTimeout(startId: Int) = ferma()

  /** Android 15 chiama questa al posto dell'altra. */
  override fun onTimeout(startId: Int, fgsType: Int) = ferma()

  override fun onDestroy() {
    if (vivo === this) vivo = null
    super.onDestroy()
  }

  private fun ferma() {
    stopForeground(STOP_FOREGROUND_REMOVE)
    stopSelf()
  }

  companion object {
    private const val ETICHETTA = "Rymembra"
    private const val ID = 7431
    private const val CANALE = "consenso"

    @Volatile private var voluto = false
    @Volatile private var vivo: ConsensoInAttesa? = null

    /** Acceso prima di aprire il browser, spento al ritorno. Non lancia mai. */
    fun imposta(contesto: Context, acceso: Boolean) {
      voluto = acceso
      if (acceso) {
        try {
          contesto.startForegroundService(Intent(contesto, ConsensoInAttesa::class.java))
        } catch (e: Exception) {
          // Per esempio con l'app già dietro a qualcos'altro: senza servizio
          // il consenso torna lo stesso quasi sempre, e non vale un errore.
          Log.w(ETICHETTA, "il servizio del consenso non parte", e)
        }
      } else {
        // Solo se è partito: fermarlo prima del suo `startForeground` farebbe
        // chiudere l'app. Se non è ancora partito, si ferma da sé appena parte.
        val servizio = vivo ?: return
        Handler(Looper.getMainLooper()).post { servizio.ferma() }
      }
    }

    private fun notifica(contesto: Context): Notification {
      val gestore = contesto.getSystemService(NotificationManager::class.java)
      gestore.createNotificationChannel(
        NotificationChannel(
          CANALE,
          contesto.getString(R.string.consenso_canale),
          NotificationManager.IMPORTANCE_LOW,
        ),
      )
      val apri = PendingIntent.getActivity(
        contesto,
        0,
        Intent(contesto, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE,
      )
      return Notification.Builder(contesto, CANALE)
        .setSmallIcon(R.drawable.notifica_consenso)
        .setContentTitle(contesto.getString(R.string.consenso_titolo))
        .setContentText(contesto.getString(R.string.consenso_testo))
        .setContentIntent(apri)
        .setOngoing(true)
        .build()
    }
  }
}
