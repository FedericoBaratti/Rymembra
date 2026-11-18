package io.github.federicobaratti.rymembra

import android.content.Context
import android.content.Intent
import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaButtonReceiver

/**
 * Il ricevitore dei tasti multimediali di Media3, per far ripartire la musica
 * quando l'app non suona: il play di una cuffia o dell'auto, e la scheda di
 * ripresa che il sistema mostra fra le impostazioni rapide.
 *
 * # Perché è spento di serie, e si accende solo con il sottofondo
 *
 * Il ricevitore fa nascere il servizio della musica in primo piano da un tasto
 * play, e Android lo chiude se entro pochi secondi non suona niente. A nucleo
 * spento non può suonare niente — Rust parte con l'attività, e da Android 10 un
 * servizio non può aprirla (vedi [Nucleo]) —: un tasto che fa cadere l'app è
 * peggio di nessun tasto. Per questo il manifesto lo dichiara `enabled="false"`,
 * e lo accende [Presenza] — `PackageManager.setComponentEnabledSetting` — solo
 * quando «Resta attiva in sottofondo» è acceso: lì il processo c'è, e con lui
 * il nucleo.
 *
 * # Perché una sottoclasse, e non quello di Media3 com'è
 *
 * Perché lo stato di un componente sopravvive al riavvio del telefono, e il
 * sottofondo no: dopo un riavvio il servizio non riparte da solo (lo farà
 * l'avvio all'accensione), e il primo play dell'auto o delle cuffie arriverebbe
 * qui con il ricevitore acceso e il nucleo no — il guasto di sopra, esatto. La
 * sottoclasse usa il punto che Media3 lascia apposta: `shouldStartForegroundService`.
 * Senza nucleo che legge i gesti, o col sottofondo spento nel frattempo, il tasto
 * non parte il servizio, e Media3 lo scrive nel logcat.
 */
@OptIn(UnstableApi::class)
class RicevitoreTasti : MediaButtonReceiver() {
  override fun shouldStartForegroundService(context: Context, intent: Intent): Boolean =
    Nucleo.pronto && Presenza.voluto(context)
}
