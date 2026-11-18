package io.github.federicobaratti.rymembra

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.graphics.drawable.Icon
import android.util.Log
import androidx.core.app.NotificationManagerCompat

/**
 * La notifica «Pronto da installare»: l'aggiornamento è scaricato e verificato,
 * e aspetta un tocco su «Installa».
 *
 * Sul telefono l'installazione non parte da sola: la conferma la chiede Android,
 * e la finestra di sistema deve comparire quando chi usa il telefono la sta
 * aspettando. Lo scaricamento invece può finire con l'app già in tasca, e senza
 * un avviso la scheda con «Installa» resta dietro a un'app che nessuno riapre.
 * Questa notifica dice che c'è, e toccarla — o il suo tasto «Apri» — riporta
 * l'app davanti: la scheda dell'aggiornamento sta nella pila delle notifiche
 * della pagina, e con il pacchetto pronto mostra «Installa».
 *
 * Non installa niente: l'installatore si apre solo dal tocco su «Installa»
 * dentro l'app (`Installatore.installa`), che ripete il controllo del
 * certificato.
 *
 * # Quando c'è
 *
 * - **Solo nella distribuzione `github`.** Nel Play Store non c'è l'aggiornatore
 *   (`BuildConfig.AGGIORNATORE_APK`), e un APK da installare non esiste.
 * - **Solo con l'app fuori dalla vista.** Se la pagina è aperta, la scheda è lì
 *   davanti, e una notifica sarebbe la stessa notizia due volte. Quando l'app
 *   torna in vista la notifica se ne va ([togli], da `MainActivity.onStart`).
 * - **Solo se le notifiche sono permesse.** Non si chiedono: l'aggiornamento non
 *   le merita, e chi le ha negate ha già detto quel che pensa del chiedere. Non
 *   mostrarla non è un declassamento nascosto ma la sua scelta; la scheda resta
 *   in app. Da Android 13 `POST_NOTIFICATIONS` si chiede al primo bisogno vero
 *   (`Permessi.notificheAlBisogno`, per «Scarica qui» e per il sottofondo).
 * - **Finché il pacchetto è in attesa.** Il nucleo la spegne quando lo butta
 *   (`mobile/aggiornamenti.rs`, `scarta_pronto`): «Non installare», «Non ora»,
 *   una versione più nuova, un APK che Android rifiuterebbe.
 *
 * Il canale è suo e a bassa importanza: niente suono e niente comparsa sullo
 * schermo, e chi non la vuole la spegne da solo nelle impostazioni di Android.
 *
 * Il pacchetto pronto vive solo in memoria, e una notifica sopravvive al
 * processo: se Android chiude Rymembra con la notifica in vista, il tocco la
 * riapre senza la scheda «Installa», e il controllo d'avvio riporta «Aggiorna»,
 * che riusa l'APK già sceso.
 *
 * Lo accende e lo spegne il nucleo, col comando `sistema.aggiornamentoPronto`.
 */
object AggiornamentoPronto {
  private const val ETICHETTA = "Rymembra"

  /** Fra quelli di `ConsensoInAttesa`, `ScaricoInCorso` e `Presenza`. */
  private const val ID = 7434
  private const val CANALE = "aggiornamento"

  /** Un codice suo per il `PendingIntent`: non si confonde con le altre notifiche. */
  private const val CODICE = 7434

  /**
   * Accende la notifica di `versione`, o la spegne. Non lancia mai per una
   * notifica che non si può mostrare: lo si scrive nel registro di Android.
   */
  fun imposta(contesto: Context, acceso: Boolean, versione: String) {
    if (!acceso) return togli(contesto)
    Installatore.soloFuoriDalPlay()
    if (Ponte.inVista) return
    if (!NotificationManagerCompat.from(contesto).areNotificationsEnabled()) {
      Log.i(ETICHETTA, "l'aggiornamento è pronto, ma le notifiche sono spente: resta la scheda dell'app")
      return
    }
    try {
      val gestore = contesto.getSystemService(NotificationManager::class.java)
      gestore.createNotificationChannel(
        NotificationChannel(
          CANALE,
          contesto.getString(R.string.aggiornamento_canale),
          NotificationManager.IMPORTANCE_LOW,
        ).apply { setShowBadge(false) },
      )
      val apri = PendingIntent.getActivity(
        contesto,
        CODICE,
        Intent(contesto, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE,
      )
      gestore.notify(
        ID,
        Notification.Builder(contesto, CANALE)
          .setSmallIcon(R.drawable.notifica_scarico)
          .setContentTitle(contesto.getString(R.string.aggiornamento_titolo))
          .setContentText(contesto.getString(R.string.aggiornamento_testo, versione))
          .setContentIntent(apri)
          .setAutoCancel(true)
          .setOnlyAlertOnce(true)
          .addAction(
            Notification.Action.Builder(
              Icon.createWithResource(contesto, R.drawable.notifica_scarico),
              contesto.getString(R.string.aggiornamento_apri),
              apri,
            ).build(),
          )
          .build(),
      )
    } catch (e: Exception) {
      Log.w(ETICHETTA, "la notifica dell'aggiornamento pronto non si mostra", e)
    }
  }

  /** Toglie la notifica, se c'è. L'app è in vista, o il pacchetto non c'è più. */
  fun togli(contesto: Context) {
    try {
      contesto.getSystemService(NotificationManager::class.java).cancel(ID)
    } catch (e: Exception) {
      Log.w(ETICHETTA, "la notifica dell'aggiornamento pronto non si toglie", e)
    }
  }
}
