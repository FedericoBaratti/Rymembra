package io.github.federicobaratti.rymembra

import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

/**
 * Il browser del telefono, per un indirizzo web: le pagine del progetto, le
 * donazioni, il consenso di un servizio da collegare.
 *
 * Sul computer lo apre `tauri-plugin-opener`, che lancia un programma; su
 * Android lanciare un programma non si può (`Permission denied`), e un
 * indirizzo si apre con un `Intent` che il sistema dà al browser scelto da chi
 * usa il telefono. Solo `https` e `http`: un altro schema — `file:`,
 * `intent:`, `content:` — qui non ha niente da fare, e aprirlo vorrebbe dire
 * lasciare a una pagina la scelta di cosa lanciare.
 *
 * Aspetta che il sistema abbia accettato, così un telefono senza browser è un
 * errore che arriva a chi ha toccato il tasto, non un tocco che non fa niente.
 */
object Browser {
  fun apri(attivita: MainActivity, indirizzo: String) {
    val uri = Uri.parse(indirizzo)
    require(uri.scheme == "https" || uri.scheme == "http") { "non è un indirizzo web: ${uri.scheme}" }
    val intento = Intent(Intent.ACTION_VIEW, uri).addCategory(Intent.CATEGORY_BROWSABLE)
    val esito = CompletableFuture<Unit>()
    // Dal filo principale `runOnUiThread` esegue subito, quindi l'attesa qui
    // sotto non si blocca mai su sé stessa.
    attivita.runOnUiThread {
      try {
        attivita.startActivity(intento)
        esito.complete(Unit)
      } catch (e: ActivityNotFoundException) {
        esito.completeExceptionally(IllegalStateException("sul telefono non c'è un browser che apra $indirizzo"))
      } catch (e: Exception) {
        esito.completeExceptionally(e)
      }
    }
    esito.get(ATTESA_SECONDI, TimeUnit.SECONDS)
  }

  private const val ATTESA_SECONDI = 10L
}
