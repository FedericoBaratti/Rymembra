package io.github.federicobaratti.rymembra

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * L'accensione del telefono, e l'aggiornamento dell'app: se «Resta attiva in
 * sottofondo» era acceso, il servizio [Presenza] riparte da solo, e con lui il
 * nucleo — la libreria, il lettore, la sessione per l'auto, il collegamento con
 * i PC — senza che nessuno apra l'app (vedi [Presenza] e [Nucleo]).
 *
 * # L'aggiornamento: `MY_PACKAGE_REPLACED`
 *
 * Quando Android sostituisce l'app — un aggiornamento automatico del Play di
 * notte, o l'APK nuovo che la persona ha installato e poi chiuso col «Fine» del
 * sistema, senza «Apri» — ferma il processo, e con lui [Presenza], il nucleo, la
 * notifica e il collegamento con i PC. Il sottofondo restava «acceso» nelle
 * impostazioni e spento nei fatti, finché qualcuno non apriva l'app (S6-2).
 * `ACTION_MY_PACKAGE_REPLACED` lo manda il sistema, solo all'app sostituita, ed è
 * fra le esenzioni dal divieto di avviare un servizio in primo piano dallo sfondo
 * (developer.android.com/develop/background-work/services/fgs/restrictions-bg-start,
 * «Exemptions from background start restrictions»). La restrizione di Android 15
 * sui tipi vale per `BOOT_COMPLETED` e `connectedDevice` non ne fa parte: qui non
 * cambia niente. Le preferenze (`presenza`) sopravvivono alla sostituzione.
 *
 * # `BOOT_COMPLETED`, e non `LOCKED_BOOT_COMPLETED`
 *
 * Il database della libreria, le preferenze (`presenza`) e il Keystore con i
 * segreti dei collegamenti stanno nello storage con le credenziali, che da
 * Android 7 è disponibile solo **dopo il primo sblocco** (developer.android.com/
 * privacy-and-security/direct-boot: «Credential encrypted storage ... is the
 * default storage location and only available after the user has unlocked the
 * device»). `LOCKED_BOOT_COMPLETED` arriva prima, e solo ai componenti
 * `directBootAware`: questo non lo è — e non va dichiarato tale — perché a
 * quell'ora non avrebbe niente da aprire. `BOOT_COMPLETED` arriva dopo lo
 * sblocco, ed è l'unico che serve.
 *
 * # Perché può avviare il servizio
 *
 * Android 12+ vieta di avviare un servizio in primo piano dallo sfondo, ma esenta
 * chi lo avvia dopo `BOOT_COMPLETED` o `MY_PACKAGE_REPLACED`; Android 15 vieta, da
 * un ricevitore di `BOOT_COMPLETED`, solo alcuni tipi (`dataSync`, `mediaPlayback`, `camera`, `phoneCall`,
 * `mediaProjection`, `microphone`): [Presenza] è `connectedDevice`, che può
 * (developer.android.com/develop/background-work/services/fgs/restrictions-bg-start,
 * e service-types).
 *
 * # Solo se il sottofondo è voluto
 *
 * Il segno è `Presenza.voluto`, scritto dal nucleo a ogni giro e da «Spegni» sulla
 * notifica. Spento il sottofondo, o mai acceso, all'accensione e all'aggiornamento
 * non succede niente: il ricevitore non avvia nulla, e non tocca la libreria.
 *
 * Un servizio avviato così parte dallo sfondo, e da Android 17 (`targetSdk` 37)
 * senza la capacità «while-in-use» che serve a chiedere l'audio: vedi
 * [Sessione.fuocoPerPartire], e «Parti con l'auto».
 */
class AvvioAccensione : BroadcastReceiver() {
  override fun onReceive(contesto: Context, intento: Intent) {
    // Le due notizie che il sistema manda da sé, e che permettono di avviare un
    // servizio in primo piano dallo sfondo. Ogni altra azione non fa niente.
    if (intento.action != Intent.ACTION_BOOT_COMPLETED &&
      intento.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
    if (!Presenza.voluto(contesto)) return
    try {
      contesto.applicationContext.startForegroundService(Intent(contesto, Presenza::class.java))
    } catch (e: Exception) {
      // Se Android dice di no, non c'è altro da fare: il servizio ripartirà quando
      // la persona aprirà l'app, che lo riaccende da sé.
      Log.w("Rymembra", "il sottofondo non riparte (${intento.action})", e)
    }
  }
}
