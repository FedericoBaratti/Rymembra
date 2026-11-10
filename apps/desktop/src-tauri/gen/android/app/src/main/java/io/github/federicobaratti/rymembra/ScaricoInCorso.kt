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
 * Un servizio in primo piano mentre «Scarica qui» copia i brani di un altro
 * dispositivo collegato via internet.
 *
 * Senza, un'app in secondo piano che non sta suonando viene congelata dal
 * sistema dopo pochi secondi: lo scaricamento di un album si fermerebbe a metà
 * appena il telefono va in tasca. La notifica dice cosa sta scendendo, e
 * toccandola si torna a Rymembra.
 *
 * Tipo `dataSync`: è un trasferimento che la persona ha chiesto e che finisce
 * da sé. Da Android 15 il sistema gliene concede al più sei ore al giorno:
 * allo scadere ([onTimeout]) il servizio si ferma, e lo scaricamento continua
 * finché il sistema lo lascia fare.
 *
 * Lo accende, lo aggiorna e lo spegne il nucleo, col comando `sistema.scarico`
 * (vedi `lontano/scarico.rs`): alla partenza della coda, a ogni brano, alla
 * fine.
 */
class ScaricoInCorso : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    vivo = this
    // Prima di tutto `startForeground`: un servizio avviato con
    // `startForegroundService` che non lo chiama entro pochi secondi fa
    // chiudere l'app, anche se nel frattempo nessuno lo vuole più.
    try {
      val notifica = notifica(this)
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        startForeground(ID, notifica, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
      } else {
        startForeground(ID, notifica)
      }
    } catch (e: Exception) {
      Log.w(ETICHETTA, "il servizio dello scaricamento non va in primo piano", e)
      stopSelf()
      return START_NOT_STICKY
    }
    // Spento prima ancora di partire: la coda è già finita.
    if (!voluto) ferma()
    return START_NOT_STICKY
  }

  /** Android 15: le ore concesse sono finite. Senza fermarsi, il sistema chiude l'app. */
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
    private const val ID = 7432
    private const val CANALE = "scarico"

    @Volatile private var voluto = false
    @Volatile private var vivo: ScaricoInCorso? = null

    /** A che punto è la coda: il brano `numero` di `di`, e il suo titolo. */
    @Volatile private var numero = 0
    @Volatile private var di = 0
    @Volatile private var titolo = ""

    /**
     * Acceso alla partenza della coda e a ogni brano (aggiorna la notifica),
     * spento alla fine. Non lancia mai.
     */
    fun imposta(contesto: Context, acceso: Boolean, numero: Int, di: Int, titolo: String) {
      voluto = acceso
      this.numero = numero
      this.di = di
      this.titolo = titolo
      if (acceso) {
        val servizio = vivo
        if (servizio != null) {
          // Già in primo piano: basta la notifica nuova.
          try {
            contesto.getSystemService(NotificationManager::class.java).notify(ID, notifica(contesto))
          } catch (e: Exception) {
            Log.w(ETICHETTA, "la notifica dello scaricamento non si aggiorna", e)
          }
          return
        }
        try {
          contesto.startForegroundService(Intent(contesto, ScaricoInCorso::class.java))
        } catch (e: Exception) {
          // Per esempio con l'app già in secondo piano: lo scaricamento va
          // avanti lo stesso finché il sistema lo lascia fare.
          Log.w(ETICHETTA, "il servizio dello scaricamento non parte", e)
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
          contesto.getString(R.string.scarico_canale),
          NotificationManager.IMPORTANCE_LOW,
        ),
      )
      val apri = PendingIntent.getActivity(
        contesto,
        0,
        Intent(contesto, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE,
      )
      val costruttore = Notification.Builder(contesto, CANALE)
        .setSmallIcon(R.drawable.notifica_scarico)
        .setContentTitle(contesto.getString(R.string.scarico_titolo))
        .setContentIntent(apri)
        .setOngoing(true)
        .setOnlyAlertOnce(true)
      if (di > 0) {
        costruttore
          .setContentText(contesto.getString(R.string.scarico_testo, numero, di, titolo))
          .setProgress(di, (numero - 1).coerceIn(0, di), false)
      }
      return costruttore.build()
    }
  }
}
